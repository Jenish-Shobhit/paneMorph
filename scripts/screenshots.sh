#!/bin/sh
# Regenerate the README screenshots in docs/assets/.
#
#   scripts/screenshots.sh          windows over the demo session, plus a
#                                   real herdr capture when herdr is on PATH
#   scripts/screenshots.sh --no-live  skip the herdr capture
#
# The herdr capture runs in a throwaway session with a private HOME and
# XDG directories; it never touches your own herdr session.
set -eu
cd "$(dirname "$0")/.."

live="--live"
if [ "${1:-}" = "--no-live" ]; then
    live=""
elif ! command -v "${HERDR_BIN:-herdr}" >/dev/null 2>&1; then
    echo "herdr not found: rendering the windows only" >&2
    live=""
fi

if [ -n "$live" ]; then
    cargo build --release --locked
fi
cargo run --locked --example screenshots -- $live
