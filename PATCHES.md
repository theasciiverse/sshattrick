# Changes from upstream

This is the Asciiverse packaging of [ssHattrick](https://github.com/ricott1/sshattrick)
by ricott1 (GPL-3.0; its engine, `sshattrick-core`, MIT or Apache-2.0),
built from upstream's main as of 0.2.1. Upstream is the `upstream` remote;
the `asciiverse` branch carries the changes below. The fix and the
computer player are worth offering upstream.

## Code

- **Play here.** Upstream is an SSH server only: you start it and connect
  with `ssh`. With no options this edition plays in the terminal it runs
  in: the person there joins the same lobby as anyone connecting over ssh
  (`src/local.rs`; their screen goes through `src/out.rs`, which is an SSH
  channel or this terminal), and the game opens to the local network on
  port 3020 or the next free one, the lobby showing the `ssh` line friends
  use. It waits for a window of 160x50 and says so.
- **A computer player** (`src/ai.rs`), easy, normal or hard. It plays
  through the same commands a person sends, at a key-repeat rate, so it
  accelerates, turns and shoots the way someone holding the arrows does:
  it takes a loose puck, chases a carrier to steal it, carries its own puck
  into range, faces the goal and steers the wind-up at the side the keeper
  is not covering. Real-time tests (`--ignored`): every level scores
  against an empty chair, hard beats easy, two computers never lock up.
- **Lobby choices:** `c` plays the computer (`l` changes its level), `w`
  watches two computers. For everyone, over ssh too.
- **Options:** `--cpu LEVEL`, `--practice` and `--watch` start straight in;
  `--offline` plays without opening to the network; `--join HOST[:PORT]`
  runs `ssh` to a friend's game; `--server` is upstream's server alone
  (upstream's `-p PORT` alone used to start the server; it now sets the
  port for either).
- **Fix: two skaters no longer lock together for good.** When they touched
  after every move, the collision put both back where they were and
  averaged their velocities, so neither could get away; and a skater put
  back just past the boards was bounced into the other again. Both froze
  until the clock ran out. Now the boards apply again after a player
  collision, and skaters that collide three ticks in a row get a small push
  apart (`PLAYER_UNLOCK_IMPULSE`). A single bump plays as before.
- **The host key** is kept beside the settings, not in the folder the
  server was started from (`frittura-ssh-core` writes `./keys`).
- **Session logging** honours `RUST_LOG`: at `debug`, every key in the
  lobby and the game, and a snapshot of each match twice a second, in
  `sshattrick.log` beside the settings.

## Packaging (new files)

- `.github/workflows/release.yml`: av archives for Linux (static), macOS
  and Windows, each x86-64 and ARM64, plus SHA256SUMS, and .deb, .rpm,
  .apk and Arch packages. The archives carry LICENSE and the engine's
  LICENSE-MIT as LICENSE-CORE-MIT.
- `asciiverse/nfpm.yaml`: the system packages.
- `asciiverse/record-preview.sh`: records the catalog preview
  (`asciiverse/sshattrick.cast`): two computers playing, at 160x50.

## Ours

- **How to play it, at the top of README.md.** A short note on playing it
  with av and on this edition's local play; the original text follows
  unchanged.
