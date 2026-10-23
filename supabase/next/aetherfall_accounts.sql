-- NOT APPLIED YET. Replaces supabase/migrations/20261022000000_aetherfall_ranked.sql
-- once the game (af_net::auth) and the server (af_server accounts/config)
-- are moved to it; apply both together. It drops the first draft's objects
-- (tables empty, never used) before creating the new ones.
--
-- Why: the project is shared with another app whose `on_auth_user_created`
-- trigger creates that app's profile rows for every Supabase Auth sign-up,
-- so game accounts must not go through Supabase Auth; and the first draft
-- had the game server hold the project's service-role key. Here:
--
-- * af_players: username, rating, record. Public (the leaderboard).
-- * af_credentials: bcrypt password hashes. No policies: nobody reads it.
-- * af_login_tickets: one-time login tickets (hashed), valid 5 minutes.
--   The game client signs in here over HTTPS and hands the ticket to the
--   game server, which redeems it once: a ticket seen on the game's plain
--   TCP link is already spent.
-- * af_server_keys: SHA-256 hashes of game-server keys. The server holds
--   the key (generated on the server, never sent anywhere); only the hash
--   is stored. The key can redeem tickets and record matches, nothing else.
-- * af_matches, af_match_players: finished ranked matches. Public.
-- * af_login_failures, af_signups: throttling.
--
-- Clients use only the project's publishable (anon) key, sending just the
-- `apikey` header. The game server uses the publishable key plus its
-- server key. No service-role key anywhere.
--
-- Register a server key (on the server: KEY=$(openssl rand -hex 32), keep
-- it in /etc/aetherfall/server.env as AF_SERVER_KEY, and register only its
-- hash, printf %s "$KEY" | sha256sum):
--   insert into public.af_server_keys (key_hash, label) values ('<sha256 hex>', 'pi');

drop function if exists public.af_record_match(text, text, integer, real, text, jsonb);
drop table if exists public.af_match_players;
drop table if exists public.af_matches;
drop table if exists public.af_players;

create table public.af_players (
  id uuid primary key default gen_random_uuid(),
  username text not null,
  elo integer not null default 1600,
  wins integer not null default 0,
  losses integer not null default 0,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint af_players_username_len check (char_length(username) between 3 and 16),
  constraint af_players_username_chars check (username ~ '^[A-Za-z0-9_]+$')
);
create unique index af_players_username_lower on public.af_players (lower(username));

create table public.af_credentials (
  player_id uuid primary key references public.af_players (id) on delete cascade,
  password_hash text not null,
  updated_at timestamptz not null default now()
);

create table public.af_login_tickets (
  ticket_hash text primary key,
  player_id uuid not null references public.af_players (id) on delete cascade,
  expires_at timestamptz not null
);
create index af_login_tickets_expires on public.af_login_tickets (expires_at);

create table public.af_login_failures (
  username_lower text not null,
  at timestamptz not null default now()
);
create index af_login_failures_user_at on public.af_login_failures (username_lower, at);

create table public.af_signups (
  at timestamptz not null default now()
);
create index af_signups_at on public.af_signups (at);

create table public.af_server_keys (
  key_hash text primary key,
  label text not null default '',
  created_at timestamptz not null default now()
);

create table public.af_matches (
  id bigint generated always as identity primary key,
  mode text not null check (mode in ('duel', 'triple', 'teams')),
  seed text not null,
  winning_team smallint,
  duration_secs real not null default 0,
  -- '' for a rated result; why the ratings did not move otherwise ("void: desync").
  note text not null default '',
  played_at timestamptz not null default now()
);

create table public.af_match_players (
  match_id bigint not null references public.af_matches (id) on delete cascade,
  player_id uuid not null references public.af_players (id) on delete cascade,
  slot smallint not null,
  team smallint not null,
  faction text not null default '',
  elo_before integer not null,
  elo_after integer not null,
  won boolean not null default false,
  primary key (match_id, player_id)
);
create index af_match_players_player on public.af_match_players (player_id);

alter table public.af_players enable row level security;
alter table public.af_credentials enable row level security;
alter table public.af_login_tickets enable row level security;
alter table public.af_login_failures enable row level security;
alter table public.af_signups enable row level security;
alter table public.af_server_keys enable row level security;
alter table public.af_matches enable row level security;
alter table public.af_match_players enable row level security;

create policy "af_players are public" on public.af_players for select to anon, authenticated using (true);
create policy "af_matches are public" on public.af_matches for select to anon, authenticated using (true);
create policy "af_match_players are public" on public.af_match_players for select to anon, authenticated using (true);

-- Internal: a fresh one-time ticket for a player (the plain ticket is
-- returned once; only its hash is kept).
create or replace function public.af_issue_ticket(p_player uuid) returns text
language plpgsql security definer set search_path = '' as $$
declare
  v_ticket text := encode(extensions.gen_random_bytes(32), 'hex');
begin
  delete from public.af_login_tickets where expires_at < now();
  insert into public.af_login_tickets (ticket_hash, player_id, expires_at)
  values (encode(sha256(convert_to(v_ticket, 'UTF8')), 'hex'), p_player, now() + interval '5 minutes');
  return v_ticket;
end $$;

-- Internal: refuse a call without a registered server key.
create or replace function public.af_check_server_key(p_key text) returns void
language plpgsql security definer set search_path = '' as $$
begin
  if not exists (
    select 1 from public.af_server_keys
    where key_hash = encode(sha256(convert_to(coalesce(p_key, ''), 'UTF8')), 'hex')
  ) then
    raise exception 'unknown game-server key' using errcode = '28000';
  end if;
end $$;

-- Create an account. Returns a login ticket.
create or replace function public.af_sign_up(p_username text, p_password text) returns text
language plpgsql security definer set search_path = '' as $$
declare
  v_id uuid;
begin
  if p_username is null or p_username !~ '^[A-Za-z0-9_]{3,16}$' then
    raise exception 'a username is 3 to 16 letters, digits or underscores';
  end if;
  if p_password is null or char_length(p_password) < 8 or char_length(p_password) > 128 then
    raise exception 'a password has 8 to 128 characters';
  end if;
  if (select count(*) from public.af_signups where at > now() - interval '10 minutes') >= 30 then
    raise exception 'too many new accounts right now; try again in a few minutes';
  end if;
  delete from public.af_signups where at < now() - interval '1 day';
  insert into public.af_signups default values;
  begin
    insert into public.af_players (username) values (p_username) returning id into v_id;
  exception when unique_violation then
    raise exception 'that username is taken';
  end;
  insert into public.af_credentials (player_id, password_hash)
  values (v_id, extensions.crypt(p_password, extensions.gen_salt('bf', 10)));
  return public.af_issue_ticket(v_id);
end $$;

-- Log in. Returns a login ticket, or null for a wrong username or
-- password (null rather than an error, so the failure stays recorded for
-- throttling: an error would roll it back).
create or replace function public.af_sign_in(p_username text, p_password text) returns text
language plpgsql security definer set search_path = '' as $$
declare
  v_lower text := lower(coalesce(p_username, ''));
  v_id uuid;
  v_hash text;
begin
  if (select count(*) from public.af_login_failures where username_lower = v_lower and at > now() - interval '15 minutes') >= 10 then
    raise exception 'too many failed logins for that username; wait 15 minutes';
  end if;
  select p.id, c.password_hash into v_id, v_hash
    from public.af_players p join public.af_credentials c on c.player_id = p.id
   where lower(p.username) = v_lower;
  if v_id is null then
    -- As slow as a real check, so timing does not tell which names exist.
    perform extensions.crypt(coalesce(p_password, ''), extensions.gen_salt('bf', 10));
  end if;
  if v_id is null or extensions.crypt(coalesce(p_password, ''), v_hash) <> v_hash then
    delete from public.af_login_failures where at < now() - interval '1 day';
    insert into public.af_login_failures (username_lower) values (v_lower);
    return null;
  end if;
  return public.af_issue_ticket(v_id);
end $$;

-- Game server: spend a login ticket, get the player.
create or replace function public.af_redeem_ticket(p_server_key text, p_ticket text)
returns table (user_id uuid, username text, elo integer, wins integer, losses integer)
language plpgsql security definer set search_path = '' as $$
declare
  v_player uuid;
begin
  perform public.af_check_server_key(p_server_key);
  delete from public.af_login_tickets
   where ticket_hash = encode(sha256(convert_to(coalesce(p_ticket, ''), 'UTF8')), 'hex')
     and expires_at > now()
  returning player_id into v_player;
  if v_player is null then
    raise exception 'that login has expired; log in again';
  end if;
  return query select p.id, p.username, p.elo, p.wins, p.losses from public.af_players p where p.id = v_player;
end $$;

-- Game server: store a finished ranked match and move the ratings, in one
-- transaction. p_players: [{user_id, slot, team, faction, elo_before, elo_after, won}, ...]
create or replace function public.af_record_match(
  p_server_key text,
  p_mode text,
  p_seed text,
  p_winning_team integer,
  p_duration_secs real,
  p_note text,
  p_players jsonb
) returns bigint
language plpgsql security definer set search_path = '' as $$
declare
  v_match_id bigint;
  v_player jsonb;
  v_rated boolean := coalesce(p_note, '') = '';
  v_won boolean;
begin
  perform public.af_check_server_key(p_server_key);
  insert into public.af_matches (mode, seed, winning_team, duration_secs, note)
  values (p_mode, p_seed, p_winning_team, coalesce(p_duration_secs, 0), coalesce(p_note, ''))
  returning id into v_match_id;
  for v_player in select * from jsonb_array_elements(coalesce(p_players, '[]'::jsonb)) loop
    v_won := coalesce((v_player->>'won')::boolean, false);
    insert into public.af_match_players (match_id, player_id, slot, team, faction, elo_before, elo_after, won)
    values (
      v_match_id,
      (v_player->>'user_id')::uuid,
      (v_player->>'slot')::smallint,
      (v_player->>'team')::smallint,
      coalesce(v_player->>'faction', ''),
      (v_player->>'elo_before')::integer,
      (v_player->>'elo_after')::integer,
      v_won
    );
    update public.af_players
       set elo = (v_player->>'elo_after')::integer,
           wins = wins + case when v_rated and v_won then 1 else 0 end,
           losses = losses + case when v_rated and not v_won then 1 else 0 end,
           updated_at = now()
     where id = (v_player->>'user_id')::uuid;
  end loop;
  return v_match_id;
end $$;

-- Only the four entry points are callable from outside; the helpers are
-- not. (Supabase grants new public functions to anon and authenticated by
-- default, hence the explicit revokes.)
revoke all on function public.af_issue_ticket(uuid) from public, anon, authenticated;
revoke all on function public.af_check_server_key(text) from public, anon, authenticated;
revoke all on function public.af_sign_up(text, text) from public;
revoke all on function public.af_sign_in(text, text) from public;
revoke all on function public.af_redeem_ticket(text, text) from public;
revoke all on function public.af_record_match(text, text, text, integer, real, text, jsonb) from public;
grant execute on function public.af_sign_up(text, text) to anon, authenticated;
grant execute on function public.af_sign_in(text, text) to anon, authenticated;
grant execute on function public.af_redeem_ticket(text, text) to anon, authenticated;
grant execute on function public.af_record_match(text, text, text, integer, real, text, jsonb) to anon, authenticated;
