-- Aetherfall ranked play: one row per player (rating, record), one per
-- finished match and one per player in it. The game server writes with
-- the service-role key; anyone may read (the leaderboard).

create table if not exists public.af_players (
  user_id uuid primary key references auth.users (id) on delete cascade,
  username text not null,
  elo integer not null default 1600,
  wins integer not null default 0,
  losses integer not null default 0,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint af_players_username_len check (char_length(username) between 3 and 16),
  constraint af_players_username_chars check (username ~ '^[A-Za-z0-9_]+$')
);
create unique index if not exists af_players_username_lower on public.af_players (lower(username));

create table if not exists public.af_matches (
  id bigint generated always as identity primary key,
  mode text not null check (mode in ('duel', 'triple', 'teams')),
  seed text not null,
  winning_team smallint,
  duration_secs real not null default 0,
  -- '' for a rated result; why the ratings did not move otherwise ("void: desync").
  note text not null default '',
  played_at timestamptz not null default now()
);

create table if not exists public.af_match_players (
  match_id bigint not null references public.af_matches (id) on delete cascade,
  user_id uuid not null references public.af_players (user_id) on delete cascade,
  slot smallint not null,
  team smallint not null,
  faction text not null default '',
  elo_before integer not null,
  elo_after integer not null,
  won boolean not null default false,
  primary key (match_id, user_id)
);
create index if not exists af_match_players_user on public.af_match_players (user_id);

alter table public.af_players enable row level security;
alter table public.af_matches enable row level security;
alter table public.af_match_players enable row level security;

drop policy if exists "af_players are public" on public.af_players;
create policy "af_players are public" on public.af_players for select using (true);
drop policy if exists "af_matches are public" on public.af_matches;
create policy "af_matches are public" on public.af_matches for select using (true);
drop policy if exists "af_match_players are public" on public.af_match_players;
create policy "af_match_players are public" on public.af_match_players for select using (true);

-- Store a finished ranked match and move the ratings, in one transaction.
-- p_players: [{user_id, slot, team, faction, elo_before, elo_after, won}, ...]
create or replace function public.af_record_match(
  p_mode text,
  p_seed text,
  p_winning_team integer,
  p_duration_secs real,
  p_note text,
  p_players jsonb
) returns bigint
language plpgsql
security definer
set search_path = public
as $$
declare
  v_match_id bigint;
  v_player jsonb;
  v_rated boolean := coalesce(p_note, '') = '';
begin
  insert into public.af_matches (mode, seed, winning_team, duration_secs, note)
  values (p_mode, p_seed, p_winning_team, coalesce(p_duration_secs, 0), coalesce(p_note, ''))
  returning id into v_match_id;
  for v_player in select * from jsonb_array_elements(coalesce(p_players, '[]'::jsonb)) loop
    insert into public.af_match_players (match_id, user_id, slot, team, faction, elo_before, elo_after, won)
    values (
      v_match_id,
      (v_player->>'user_id')::uuid,
      (v_player->>'slot')::smallint,
      (v_player->>'team')::smallint,
      coalesce(v_player->>'faction', ''),
      (v_player->>'elo_before')::integer,
      (v_player->>'elo_after')::integer,
      coalesce((v_player->>'won')::boolean, false)
    );
    update public.af_players
       set elo = (v_player->>'elo_after')::integer,
           wins = wins + case when v_rated and coalesce((v_player->>'won')::boolean, false) then 1 else 0 end,
           losses = losses + case when v_rated and not coalesce((v_player->>'won')::boolean, false) then 1 else 0 end,
           updated_at = now()
     where user_id = (v_player->>'user_id')::uuid;
  end loop;
  return v_match_id;
end
$$;

revoke all on function public.af_record_match(text, text, integer, real, text, jsonb) from public;
revoke all on function public.af_record_match(text, text, integer, real, text, jsonb) from anon, authenticated;
grant execute on function public.af_record_match(text, text, integer, real, text, jsonb) to service_role;
