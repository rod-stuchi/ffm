# FFM — task runner.
#
# `just` with no arguments lists the recipes.

set dotenv-load := true

db_path := env_var_or_default("FFM_DB_PATH", "./ffm.db")

default:
    @just --list

# --- dev loop ----------------------------------------------------------------

# Rebuild + restart on save. Pair with `just css` in a second pane.
# dev-reload is opt-in here rather than a default feature, so a release build
# cannot pick it up by forgetting a flag.
dev:
    cargo watch -x 'run --features dev-reload'

# Tailwind in watch mode. v4 is CSS-first — the tokens live in an @theme block
# in static/css/app.css, so there is deliberately no tailwind.config.js.
css:
    npx @tailwindcss/cli -i static/css/app.css -o static/css/build.css --watch

css-build:
    npx @tailwindcss/cli -i static/css/app.css -o static/css/build.css --minify

# Re-download the self-hosted font subsets and regenerate static/css/fonts.css.
# The output is committed, so this is only needed when the weights change.
fonts:
    python3 scripts/fetch-fonts.py

# --- database ----------------------------------------------------------------

# Create the database and apply every migration with the plain sqlite3 CLI.
#
# This exists to break a bootstrap deadlock: sqlx::query! checks queries against
# a real schema at COMPILE time, but sqlx::migrate!() only applies migrations at
# STARTUP — so a fresh checkout cannot compile a query until the schema exists.
# Doing it here with sqlite3 avoids making cargo-sqlx a prerequisite just to get
# the project to build the first time.
db:
    @mkdir -p "$(dirname {{db_path}})"
    sqlite3 {{db_path}} < migrations/0001_initial_schema.sql
    @echo "✓ {{db_path}}"

# Throw it away and rebuild. Safe while no household database exists anywhere;
# the migration's own PRE-RELEASE note stops applying the moment one does.
db-reset:
    rm -f {{db_path}} {{db_path}}-wal {{db_path}}-shm
    @just db

# Apply the migration to a scratch file inside a transaction — which is how sqlx
# runs it — and throw it away. Catches anything that cannot run in one, the way
# `PRAGMA journal_mode = WAL` could not.
db-check:
    #!/usr/bin/env bash
    set -euo pipefail
    tmp=$(mktemp -t ffm-schema-XXXX.db)
    { echo "BEGIN;"; cat migrations/*.sql; echo "COMMIT;"; } | sqlite3 "$tmp"
    echo "✓ schema applies inside a transaction"
    rm -f "$tmp"

# --- checks ------------------------------------------------------------------

test:
    cargo test

lint:
    cargo clippy --all-targets -- -D warnings

fmt:
    cargo fmt

fmt-check:
    cargo fmt --check

# No hex below tokens.css: every component rule reads var(--token), so a palette
# swap can actually reach it. This is the grep that keeps that true.
# Strips /* */ comments before grepping: a comment may legitimately say
# "never #fff", and flagging the rule that forbids a thing is not the same as
# flagging the thing. Same trap as build.rs's forbidden-content scan.
css-check:
    #!/usr/bin/env bash
    set -euo pipefail
    found=0
    for f in static/css/*.css; do
        case "$f" in */tokens.css) continue;; esac
        stripped=$(perl -0777 -pe 's{/\*.*?\*/}{}gs' "$f")
        if echo "$stripped" | grep -nE '#[0-9a-fA-F]{3,8}\b' >/dev/null; then
            echo "✗ $f:"
            echo "$stripped" | grep -nE '#[0-9a-fA-F]{3,8}\b'
            found=1
        fi
    done
    if [ "$found" = 1 ]; then
        echo "✗ hardcoded colour outside tokens.css" >&2
        exit 1
    fi
    echo "✓ no hardcoded colours outside tokens.css"

check: fmt-check lint test db-check css-check

# --- docs --------------------------------------------------------------------

md:
    mdserve -p 8080 .
