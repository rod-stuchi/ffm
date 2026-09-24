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
dev: css-build
    cargo watch -x 'run --features dev-reload'

# Same loop, reachable from the rest of the house. Binds every interface instead
# of loopback, so a phone or a second laptop on the same LAN/Tailscale can open
# it — the boot log prints the address to type. Deliberately a separate recipe:
# `just dev` staying on 127.0.0.1 means nothing is exposed by forgetting a flag.
#
# There is no auth layer yet, so only run this on a network you trust — which is
# the same deployment assumption the auth design makes (see design/spec.md).
dev-lan: css-build
    FFM_ADDR=0.0.0.0:3000 cargo watch -x 'run --features dev-reload'

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
#
# It also writes _sqlx_migrations itself, with the same SHA-384 over the file
# that sqlx computes, so the migrator at startup agrees the migration is already
# applied instead of trying to re-create every table. Change a migration and the
# checksums disagree on purpose — that is sqlx telling you to `just db-reset`,
# which is fine only while no household database exists anywhere.
db:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p "$(dirname {{db_path}})"
    for f in migrations/*.sql; do
        base=$(basename "$f" .sql)
        version=${base%%_*}
        description=${base#*_}
        sum=$(shasum -a 384 "$f" | cut -d' ' -f1)
        # Idempotent: applying a migration twice is an error, not a no-op, so
        # ask whether this version is already recorded before running it.
        done_already=$(sqlite3 {{db_path}} "SELECT count(*) FROM sqlite_master
            WHERE type='table' AND name='_sqlx_migrations';" | grep -q 1 &&
            sqlite3 {{db_path}} "SELECT count(*) FROM _sqlx_migrations WHERE version=$version;" || echo 0)
        if [ "$done_already" = "1" ]; then
            echo "· $base already applied"
            continue
        fi
        sqlite3 {{db_path}} < "$f"
        sqlite3 {{db_path}} "
            CREATE TABLE IF NOT EXISTS _sqlx_migrations (
                version BIGINT PRIMARY KEY,
                description TEXT NOT NULL,
                installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                success BOOLEAN NOT NULL,
                checksum BLOB NOT NULL,
                execution_time BIGINT NOT NULL);
            INSERT OR REPLACE INTO _sqlx_migrations
                (version, description, success, checksum, execution_time)
            VALUES ($version, '$description', 1, X'$sum', 0);"
    done
    echo "✓ {{db_path}}"

# Throw it away and rebuild. Safe while no household database exists anywhere;
# the migration's own PRE-RELEASE note stops applying the moment one does.
db-reset:
    rm -f {{db_path}} {{db_path}}-wal {{db_path}}-shm
    @just db

# A household to look at. Content is Brazilian-household Portuguese on purpose
# — it is what a household typed into its own ledger, not app copy, so it never
# passes through the translation catalog.
#
# It depends on db-reset, not db: the seed file opens by deleting every table,
# and `change`/`change_field` carry BEFORE DELETE triggers that RAISE(ABORT) —
# the log is append-only, on purpose. Starting from an empty database is the
# same intent with no trigger to fight and no second copy of that DDL to drift.
seed: db-reset
    sqlite3 -bail {{db_path}} < seeds/household.sql
    @echo "✓ seeded {{db_path}}"

# Refresh .sqlx/ — the offline copy of every checked query's result shape, so a
# clean checkout (and CI) can build with no database at all. Run this whenever a
# query or the schema changes, and commit what it writes.
prepare: db
    cargo sqlx prepare

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

# Prove the crate still builds with no database in sight, which is what CI and
# a fresh checkout actually do. Run `just prepare` first if a query changed.
offline-check:
    SQLX_OFFLINE=true cargo check --all-targets

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
        # tokens.css is where color is allowed to be literal; build.css and
        # app.css are generated / Tailwind's own, not hand-written components.
        case "$f" in */tokens.css|*/build.css|*/app.css) continue;; esac
        stripped=$(perl -0777 -pe 's{/\*.*?\*/}{}gs' "$f")
        if echo "$stripped" | grep -nE '#[0-9a-fA-F]{3,8}\b' >/dev/null; then
            echo "✗ $f:"
            echo "$stripped" | grep -nE '#[0-9a-fA-F]{3,8}\b'
            found=1
        fi
    done
    if [ "$found" = 1 ]; then
        echo "✗ hardcoded color outside tokens.css" >&2
        exit 1
    fi
    echo "✓ no hardcoded colors outside tokens.css"

# Askama HTML-escapes every expression, including the json filter's output —
# and inside a <script> that turns `"` into `&#34;` and kills the whole block,
# taking the keymap with it silently. `|json|safe` is the correct pairing: the
# json filter already escapes the characters that could end the script tag, so
# `safe` is suppressing the WRONG escaping, not all of it.
tpl-check:
    #!/usr/bin/env bash
    set -euo pipefail
    if grep -rn '|json[^|]' templates/ | grep -v '|json|safe'; then
        echo "✗ a |json expression is missing |safe — it will render &#34; inside <script>" >&2
        exit 1
    fi
    echo "✓ every |json is paired with |safe"

# Alpine merges a component's data and its methods into ONE object, so a
# method named after a data property silently replaces it. `pane: 0` and
# `pane(url)` cost an afternoon: `panes[this.pane]` started indexing an array
# with a function and every row-based keystroke stopped working, with nothing
# in the console. This finds a name declared twice at the top level of a
# component object.
js-check:
    #!/usr/bin/env python3
    import re, sys, pathlib
    bad = False
    for path in pathlib.Path("templates").rglob("*.html"):
        text = path.read_text()
        for fn in re.finditer(r"function (\w+)\(\) \{\s*return \{(.*?)\n  \};", text, re.S):
            name, body = fn.group(1), fn.group(2)
            seen = {}
            for line in body.splitlines():
                m = re.match(r"    (\w+)\s*[:(]", line)
                if not m:
                    continue
                key = m.group(1)
                if key in seen:
                    print(f"\u2717 {path}: {name}() declares `{key}` twice — "
                          f"Alpine merges data and methods into one object, so "
                          f"the second silently replaces the first")
                    bad = True
                seen[key] = True
    if bad:
        sys.exit(1)
    print("\u2713 no shadowed names in an Alpine component")

# Every command the `:` bar advertises, every command a message tells people to
# type, and every command the KEYMAP documents must actually be handled — and
# every command that is handled must be documented.
#
# The hint is built in Rust, the dispatcher is JavaScript in a template and the
# keymap is prose in design/spec.md, so nothing else can hold the three
# together. All three have drifted already:
#
#   - the hint was copied from the mock and offered `add`, `in`, `move`, `undo`
#     and `help`, none of which existed;
#   - `not-a-command` said "(try help)" while `help` was not a case, so the app
#     answered "not a command: help (try help)";
#   - the keymap's Type table listed `:add`, `:in` and `:move` for months
#     (abandoned now, see TODO_2.md §2) and never listed `:grep`, which has
#     worked the whole time.
#
# The last of those is why this reads the spec in BOTH directions. A promise
# nobody kept is the bug that started this recipe; a command nobody wrote down
# is the one that hides.
cmd-check:
    #!/usr/bin/env python3
    import re, sys, pathlib
    rs = pathlib.Path("src/web/ledger.rs").read_text()
    m = re.search(r"fn command_hint\(.*?\n\}", rs, re.S)
    if not m:
        print("\u2717 command_hint() not found in src/web/ledger.rs"); sys.exit(1)
    advertised = set(re.findall(r'"(\w+)"', m.group(0)))
    html = pathlib.Path("templates/pages/ledger.html").read_text()
    handled = set(re.findall(r"case '(\w+)':", html))
    missing = sorted(advertised - handled)
    if missing:
        print("\u2717 the command bar offers what runCommand does not handle: "
              + ", ".join(missing))
        sys.exit(1)

    # A command named in a MESSAGE counts as advertised too. `not-a-command`
    # reads "not a command: { $name } (try help)" in both catalogs, and `help`
    # was not a case for months \u2014 so the app answered "not a command: help
    # (try help)". The hint check could not see that, because the name was in
    # the copy rather than in the hint.
    told = set()
    for ftl in pathlib.Path("i18n").glob("*.ftl"):
        told |= set(re.findall(r"\((?:try|tente) (\w+)\)", ftl.read_text()))
    circular = sorted(told - handled)
    if circular:
        print("\u2717 a message tells people to type a command that does not "
              "exist: " + ", ".join(circular))
        sys.exit(1)

    # The keymap's own Type table, both ways. It is prose, so nothing compiled
    # it against anything — which is exactly how it came to promise three
    # commands the app refuses and to omit one it has always handled.
    #
    # The whole row is scanned, not just its key cell: an alias is often named
    # in the description (`:config` is an alias`), and it is a real command.
    spec = pathlib.Path("design/spec.md").read_text()
    m = re.search(r"\n### Type\n(.*?)(?=\n#{2,3} )", spec, re.S)
    if not m:
        print("\u2717 the Type table was not found in design/spec.md"); sys.exit(1)
    rows = [l for l in m.group(1).split("\n") if l.startswith("|")]
    documented = {n for l in rows for n in re.findall(r"`:(\w+)", l)}

    promised = sorted(documented - handled)
    if promised:
        print("\u2717 the keymap documents a command runCommand does not "
              "handle: " + ", ".join(promised))
        sys.exit(1)

    undocumented = sorted(handled - documented)
    if undocumented:
        print("\u2717 runCommand handles a command the keymap does not "
              "document: " + ", ".join(undocumented))
        sys.exit(1)

    print(f"\u2713 all {len(advertised)} advertised commands are handled, "
          f"the {len(told)} named in messages exist, and the keymap "
          f"documents exactly the {len(handled)} that are")

check: fmt-check lint test offline-check db-check css-check tpl-check js-check cmd-check

# --- docs --------------------------------------------------------------------

md:
    mdserve -p 8080 .
