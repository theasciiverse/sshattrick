# ssHattrick (Asciiverse packaging)

ricott1's 1-on-1 ice hockey in Rust and Ratatui (after Bally Midway's Hat
Trick, 1984), upstream an SSH server only; packaged for the Asciiverse
catalog with local play, a computer player and a collision fix. Not our
code: keep changes small, list every one in PATCHES.md, and offer fixes
upstream.

- Remotes: `upstream` is github.com/ricott1/sshattrick, `origin` is
  github.com/theasciiverse/sshattrick.
- Branch `asciiverse` = upstream main (0.2.1) plus our commits.
- Release: tag `v<version>-av.<n>` and push it.
- Workspace: `crates/sshattrick-core` (engine, MIT or Apache-2.0) and
  `crates/sshattrick` (server, lobby, our local play and computer player,
  GPL-3.0-only).
- Run tests and recordings under `systemd-run --user --scope -p
  MemoryMax=4G`. The computer player's tests play real time:
  `cargo test --release -p sshattrick --lib -- --ignored` (90 s).
- Fixed 160x50 screen (`UI_SCREEN_SIZE`); the catalog preview is cropped
  to 100x30 from the centre.
- Logs and the host key live in the config dir (`store_path`), e.g.
  ~/.config/sshattrick/; `RUST_LOG=debug` for keys and snapshots.
- Store copy says local network only (internet play is not settled).
