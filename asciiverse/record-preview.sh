#!/bin/sh
# Records the catalog preview, asciiverse/sshattrick.cast, with hub's
# castrec (hub/tools/castrec.py, beside this repo in the Asciiverse tree):
# two computers playing (--watch), offline so no network address shows,
# from a throwaway home so no host key or log is left behind. Recorded at
# the game's own 160x50; the catalog crops it to 100x30 from the centre.
set -e
cd "$(dirname "$0")/.."
cargo build --release
home=$(mktemp -d)
HOME="$home" XDG_CONFIG_HOME="$home/.config" CAST_COLS=160 CAST_ROWS=50 \
PATH="$PWD/target/release:$PATH" python3 -I ../../hub/tools/castrec.py \
  asciiverse/sshattrick.cast . "w46;kesc;w1.5;kesc;w1" -- sshattrick --watch --offline
rm -rf "$home"
