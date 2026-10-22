# Multiplayer

Aetherfall plays online through a small server (`af_server`, in
`crates/af_server`) that runs happily on a Raspberry Pi. Two kinds of play:

* **Casual rooms**: up to six friends (plus computer players) in one match.
  The server only relays; everyone's game runs the match itself.
* **Ranked**: 1v1, 1v1v1 and 2v2 with accounts, Elo ratings and a
  leaderboard, stored in a Supabase project. One match of each kind runs
  at a time, and the server runs and referees every one of them.

The client side lives in `crates/af_net` (the wire protocol, the lockstep
client plugin, the account-service client); the screens are in
`crates/af_ui/src/multiplayer.rs`. In the game: **Multiplayer** on the
menu (or `M`), type the server's address, pick a name, Connect.

## How a match runs

The simulation is deterministic (a fixed 30 Hz step, a seeded RNG, entity
ids the client cannot shift, trigonometry from `libm`), so every game that
plays the same commands in the same ticks plays the same match. That is
what the server arranges:

1. Every tick the server gathers the commands it received since the last
   tick, in arrival order, stamps each with the sender's slot and
   broadcasts them as that tick's **turn**, empty or not.
2. A client runs sim tick `n` once it holds turn `n`
   (`af_net::lockstep`), feeding the turn into `CommandInbox` just before
   the tick. The local player's own commands never enter the sim directly:
   they go to the server and come back in a turn like everyone else's.
3. The fixed clock advances only as far as the turns in hand allow
   (`TimeUpdateStrategy::ManualDuration`): a client whose turns are late
   waits, one that fell behind catches up at 1.5x, one that rejoins after
   a dropped connection replays the backlog at full speed. Nobody holds
   the others up; the server's clock is the match clock. Pause and game
   speed are off while online.
4. Every ten ticks each client sends `af_core::hash::sim_hash` of its
   world. In casual rooms the server compares the players with each
   other and tells everyone who disagrees; in ranked play it compares
   each player with its own referee simulation.
5. A dropped connection is retried for two minutes; on reconnection the
   client asks for the turns it missed (`Resume`) and carries on.

A player who leaves a match (Menu > Leave) resigns: a `Resign` command
goes into the turn stream, their every unit and building dies, and the
victory rule takes it from there. In ranked play that is a loss; a player
gone for longer than `AF_FORFEIT_AFTER` forfeits the same way.

Command latency is one round trip plus a tick or two: fine for an RTS.
Bandwidth is a few KB/s per player.

### What the ranked server guarantees, and what it does not

The ranked server holds the authoritative match: it validates every
command against its own world (a player may only command what they own),
paces time, catches a tampered client by its hash (the match is voided,
or with `AF_DESYNC_FORFEITS=1` the tampered player forfeits), decides the
result itself and writes the ratings. What it cannot do is hide
information: like every lockstep RTS, each client holds the whole match
state, so a modified client could reveal the fog of war. Streaming
per-player views from the server would close that and is the natural next
step if it ever matters.

### Content and protocol versions

On connect the client sends `af_net::protocol::PROTOCOL_VERSION` and a hash
of its `assets/data` (`af_data::content_hash`). The server refuses a
mismatch with a clear message: both sides must run the same content and
protocol, or they would simulate different games. Bump the protocol
version whenever a message changes shape.

## Victory rules

* **Last standing** (default): a player is out with no living unit and no
  complete building that matters.
* **Town halls** (ranked 2v2, and an option for casual rooms): a team is
  out once none of its players owns a complete main hall (a
  `TownCenter`-tagged building). A player whose own hall fell stays in
  while a teammate holds one.

Teams share vision and never fire on each other; players only command
their own units and buildings.

## Ratings

Everyone starts at 1600. A result moves ratings by the Elo rule with
K = 32 (`crates/af_server/src/elo.rs`): the expected score of `a` against
`b` is `1 / (1 + 10^((b - a) / 400))`, and the change is
`K * (score - expected)`. Beating a stronger player pays more; losing to a
weaker one costs more. Teams are rated by their average; in 1v1v1 the
winner is rated against each loser in turn (a win over two equals pays
about twice a duel). The sum of all changes is zero when teams are the
same size.

## Accounts

Players sign up with a username and a password. The account service is
Supabase Auth: the client talks to it directly over HTTPS (so the password
never crosses the game's TCP link), using the project URL and
publishable key the server hands out on connect. Supabase wants an email,
so the username becomes `<username>@<AF_EMAIL_DOMAIN>` (a domain that
never receives mail). The access token that comes back is what the game
sends the server; the server asks Supabase who it belongs to and keeps a
row per player in `af_players`.

In the Supabase dashboard, under **Authentication > Providers > Email**,
turn **Confirm email** off (nobody can read mail at that domain). If it is
on, sign-up answers "the account service wants emails confirmed".

The schema is `supabase/migrations/20261022000000_aetherfall_ranked.sql`:
`af_players` (rating, wins, losses), `af_matches`, `af_match_players`, and
the function `af_record_match` the server calls to store a result and move
the ratings in one transaction. Row-level security lets anyone read (the
leaderboard) and only the service role write.

## Running the server

```
af_server --help
```

Settings come from the command line or `AF_*` environment variables
(`crates/af_server/src/config.rs`). Casual rooms need nothing; ranked play
needs `AF_SUPABASE_URL`, `AF_SUPABASE_ANON_KEY` and
`AF_SUPABASE_SERVICE_KEY` (or `--ranked-memory` to try ranked play with
accounts that last until the server stops). The server needs the game's
`assets/data` folder (`--data DIR`, else found like the game finds it).

Try it on one machine:

```
cargo run -p af_server -- --ranked-memory
cargo run -p aetherfall      # Multiplayer, server 127.0.0.1
```

### On a Raspberry Pi

Measured cost of one 8-player match simulated on the server (a 2.1 GHz
Xeon core, optimised build): about 1 ms per 30 Hz tick, 93 MB. Scaled to a
Pi 5 that is roughly 8 % of a core per match, to a Pi 4 about 20 %: three
ranked matches and any number of casual rooms are well within either.

1. Install the build dependencies and Rust on the Pi (64-bit OS):
   ```
   sudo apt install -y build-essential pkg-config libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```
   (The server links the same Bevy as the game, hence the desktop
   libraries; it opens no window.)
2. Clone the repository on the Pi and run `deploy/install-pi.sh`. It
   builds with the `server` profile (optimised, no LTO, fits 8 GB; expect
   20-40 minutes on a Pi 5, longer on a Pi 4), installs the binary and the
   content under `/opt/aetherfall`, and installs the systemd unit
   (`deploy/aetherfall-server.service`, run as an unprivileged user with
   the usual hardening).
3. Put the Supabase settings in `/etc/aetherfall/server.env` (see
   `deploy/server.env.example`; `chmod 600`, it holds the service-role
   key), then `sudo systemctl enable --now aetherfall-server` and watch
   `journalctl -u aetherfall-server -f`.
4. Open TCP port 7777 to the Pi (DMZ or a port forward) and give friends
   the public address or a dynamic-DNS name. Nothing else needs to be
   reachable; keep SSH off the WAN side.

To cross-compile instead (faster), build on a PC with
[`cross`](https://github.com/cross-rs/cross):
`cross build --profile server -p af_server --target aarch64-unknown-linux-gnu`,
then copy `target/aarch64-unknown-linux-gnu/server/af_server` and
`assets/data` to the Pi. The `cross` image needs the same four
development libraries for aarch64.

Updating: build again, copy the new binary and content, restart the
service. Players need the matching game build (the content hash and the
protocol version are checked on connect).

## Security notes

* The game protocol runs over plain TCP. The password never travels on it
  (it goes to Supabase over TLS), but the short-lived access token does,
  and so do match commands. Good enough for friends; put the port behind
  a VPN if that is not good enough.
* The service-role key lives only on the Pi. Clients only ever get the
  publishable key.
* The server accepts at most 64 commands per player per tick and 512
  entities per command, refuses frames over 16 MB, and drops a client that
  sends nothing for 90 seconds.
* Ranked integrity: see "What the ranked server guarantees" above.

## Troubleshooting

* *"the game's content (assets/data) differs from the server's"*: the
  server and the game are not on the same commit; update one.
* *"this server speaks protocol N"*: same, for the code.
* *"Desync at tick ..."* in a casual room: one player's game diverged
  (different build, a modified game, or a determinism bug; the
  determinism tests in `crates/aetherfall/tests` are the place to look).
  The match goes on, but that player sees a different game.
* *"Waiting for the server..."* for more than a moment: the server's
  turns are late (the connection, or an overloaded Pi).
* The game cannot connect: is port 7777 forwarded to the Pi, and is the
  service running (`systemctl status aetherfall-server`)?
