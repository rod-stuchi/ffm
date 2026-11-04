#!/usr/bin/env bash
# Inside the container: compile what scripts/e2e.sh staged at /src, then test it.
set -euo pipefail

cd /src
export CARGO_HOME=/cache/cargo CARGO_TARGET_DIR=/cache/target SQLX_OFFLINE=true

# A debug build on purpose: FFM_TODAY only moves the clock in one, and every
# test depends on the date. The `anon` feature is what builds ffm-anon.
cargo build --locked --features anon --bin ffm --bin ffm-anon
tailwindcss -i static/css/app.css -o static/css/build.css --minify 2>/dev/null

tsc --noEmit -p e2e

cd e2e
exec playwright test "$@"
