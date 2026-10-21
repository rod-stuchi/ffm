# FFM — task runner.
#
# `just` with no arguments lists the recipes.

set dotenv-load := true

db_path := env_var_or_default("FFM_DB_PATH", "./ffm.db")
receipts_root := env_var_or_default("FFM_RECEIPTS_ROOT", "./receipts")

default:
    @just --list

# --- dev loop ----------------------------------------------------------------

# Rebuild + restart on save. Pair with `just css` in a second pane.
# dev-reload is opt-in here rather than a default feature, so a release build
# cannot pick it up by forgetting a flag.
#
# With a date, the same loop AS OF that date: the clock is moved by whole days
# and keeps running from there (FFM_TODAY, see src/clock.rs), against a fresh
# copy of the database and the vault in ./demo — the demo materializes that
# month's rows and stamps its edits with that date, and none of it may land in
# the real ffm.db. The copy is made once per `just dev`, not on every
# cargo-watch restart, so what you do in the demo survives a save.
#
# An earlier dev server still on the port is stopped first (`_free-port`).
#
# `just dev` · `just dev 2026-10-15` runs as of that date, on a copy in ./demo
dev today="": _free-port css-build (_demo today)
    {{ if today == "" { "" } else { "FFM_TODAY=" + today + " FFM_DB_PATH=./demo/ffm.db FFM_RECEIPTS_ROOT=./demo/receipts FFM_BACKUP_DIR=./demo/backups" } }} cargo watch -x 'run --features dev-reload'

# Same loop, reachable from the rest of the house. Binds every interface instead
# of loopback, so a phone or a second laptop on the same LAN/Tailscale can open
# it — the boot log prints the address to type. Deliberately a separate recipe:
# `just dev` staying on 127.0.0.1 means nothing is exposed by forgetting a flag.
#
# There is no auth layer yet, so only run this on a network you trust — which is
# the same deployment assumption the auth design makes (see design/spec.md).
#
# `just dev-lan` · `just dev-lan 2026-10-15` runs as of that date, on a copy in ./demo
dev-lan today="":
    FFM_ADDR=0.0.0.0:3000 just dev {{today}}

# Stop an earlier dev loop — its server and its cargo watch — before this one
# starts.
#
# Left running, the old server answers every request while the new one dies on
# bind, so a `just dev 2026-10-15` looks like it worked while showing the real
# database's today, which is how this was found. And the port alone is not
# enough to look at: an old cargo watch whose server has already exited holds
# nothing, then restarts its server the moment this one's startup writes a file
# — and wins the port a second before the new server binds. So every cargo
# watch running `-x run` in THIS checkout on THIS port goes too, paused first
# so it cannot restart anything while its server is being stopped.
#
# The server gets SIGTERM, which it answers by checkpointing the WAL (see
# main.rs's shutdown) — its chance to leave ffm.db whole. Only if it still
# holds the port after 8 seconds is it killed outright. Anything on the port
# that is NOT an ffm binary is left alone: that is somebody else's program.
_free-port:
    #!/usr/bin/env bash
    set -euo pipefail
    addr="${FFM_ADDR:-127.0.0.1:3000}"; port="${addr##*:}"
    repo=$(pwd -P)
    held() { lsof -tiTCP:"$port" -sTCP:LISTEN 2>/dev/null | head -1; }
    alive() { kill -0 "$1" 2>/dev/null; }

    # Refuse before touching anything if the port is somebody else's.
    pid=$(held || true)
    if [ -n "$pid" ]; then
        cmd=$(ps -o command= -p "$pid" || true)
        case "$cmd" in
            */ffm|*/ffm\ *) ;;
            *) echo "✗ port $port is held by pid $pid ($cmd) — not an ffm server, so it is left alone; stop it yourself" >&2
               exit 1 ;;
        esac
    fi

    # This checkout's dev loops FOR THIS PORT: a cargo watch running `-x run`,
    # working here, whose FFM_ADDR names the same port. A `cargo watch -x test`
    # is not a server, and a loop on another port is not in the way; both stay.
    watchers=""
    for w in $(pgrep -f cargo-watch || true); do
        case "$(ps -o command= -p "$w" || true)" in *" -x run"*) ;; *) continue ;; esac
        cwd=$(lsof -a -p "$w" -d cwd -Fn 2>/dev/null | sed -n 's/^n//p')
        [ "$cwd" = "$repo" ] || continue
        theirs=$(ps eww -o command= -p "$w" | tr ' ' '\n' | sed -n 's/^FFM_ADDR=//p' | head -1)
        theirs="${theirs:-127.0.0.1:3000}"
        [ "${theirs##*:}" = "$port" ] && watchers="$watchers $w"
    done

    [ -z "$pid" ] && [ -z "$watchers" ] && exit 0

    for w in $watchers; do
        echo "→ pausing an earlier cargo watch (pid $w), so it cannot restart a server meanwhile"
        kill -STOP "$w" 2>/dev/null || true
    done

    if [ -n "$pid" ]; then
        echo "→ port $port is held by an earlier ffm (pid $pid) · asking it to stop (SIGTERM), up to 8s"
        kill -TERM "$pid" 2>/dev/null || true
        for _ in $(seq 16); do [ -z "$(held || true)" ] && break; sleep 0.5; done
        if [ -n "$(held || true)" ]; then
            echo "→ pid $pid still holds port $port after 8s · killing it (SIGKILL)"
            kill -KILL "$pid" 2>/dev/null || true
            sleep 0.5
        else
            echo "✓ pid $pid stopped on its own"
        fi
    fi

    for w in $watchers; do
        echo "→ stopping that cargo watch (pid $w)"
        kill -TERM "$w" 2>/dev/null || true
        kill -CONT "$w" 2>/dev/null || true
        for _ in $(seq 6); do alive "$w" || break; sleep 0.5; done
        if alive "$w"; then
            echo "→ cargo watch pid $w is still running after 3s · killing it (SIGKILL)"
            kill -KILL "$w" 2>/dev/null || true
        fi
    done

    if [ -n "$(held || true)" ]; then
        echo "✗ port $port is still held (pid $(held)) — stop it yourself" >&2
        exit 1
    fi
    echo "✓ port $port is free"

# The throwaway copy `just dev <date>` runs on. `.backup` rather than `cp`,
# because the dev server may have ffm.db open in WAL mode, and a plain copy of
# the main file alone is not the whole database. No date, nothing to do.
_demo today:
    #!/usr/bin/env bash
    set -euo pipefail
    [ -z "{{today}}" ] && exit 0
    rm -rf ./demo
    mkdir -p ./demo/backups
    sqlite3 "{{db_path}}" ".backup ./demo/ffm.db"
    if [ -d "{{receipts_root}}" ]; then cp -R "{{receipts_root}}" ./demo/receipts; else mkdir -p ./demo/receipts; fi
    echo "demo · ./demo is a fresh copy of {{db_path}} and {{receipts_root}} · today is {{today}}"

# Tailwind in watch mode. v4 is CSS-first — the tokens live in an @theme block
# in static/css/app.css, so there is deliberately no tailwind.config.js.
css:
    tailwindcss -i static/css/app.css -o static/css/build.css --watch

css-build:
    tailwindcss -i static/css/app.css -o static/css/build.css --minify

# Throw away this crate's build output — never the dependencies' — when the
# incremental cache goes stale. The symptom is a link failure, not a compile
# error: `linking with cc failed` and `Undefined symbols … drop_glue … ffm…web`
# naming our own handlers, after switching between builds with and without
# `dev-reload` while the same files kept changing. It is the cache, not the
# code; a rebuild from clean links. It also reclaims space: that cache had
# grown to 35 GiB when this was added.
#
# Clear ffm's own build cache — the cure for "Undefined symbols … drop_glue" at link time
clean:
    cargo clean -p ffm

# Re-download the self-hosted font subsets and regenerate static/css/fonts.css.
# The output is committed, so this is only needed when the weights change.
fonts:
    python3 scripts/fetch-fonts.py

# Render the home-screen install icons (the manifest's) from their SVG sources
# in design/logo/. The PNGs are committed, so this is only needed when the
# logo changes.
icons:
    for s in 192 512; do inkscape design/logo/ffm-app-icon.svg -w $s -h $s -o static/icons/icon-$s.png; done
    inkscape design/logo/ffm-app-icon-maskable.svg -w 512 -h 512 -o static/icons/icon-maskable-512.png

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

# A shuffled copy of a production database to develop against: every name,
# amount, account number and barcode made up, and a vault of stand-in files in
# place of the real bills. The source is copied byte for byte and never opened,
# and the run refuses while any process holds it. Lands in ./anon (gitignored)
# only once a scan for every original string comes back clean.
# See src/bin/ffm-anon/main.rs, or `just anon --help`.
#
# `just anon --from ~/prod/ffm.db --receipts ~/prod/receipts` · `--seed 7` · `--years 2` · `--scale 3`
anon *args:
    cargo run --quiet --features anon --bin ffm-anon -- {{args}}

# The dev loop against `just anon`'s copy. The environment wins over
# ffm.toml, so the real database is not touched.
dev-anon:
    FFM_DB_PATH=./anon/ffm.db FFM_RECEIPTS_ROOT=./anon/receipts FFM_BACKUP_DIR=./anon/backups just dev

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
    # A JSON string carries double quotes, so inside a double-quoted attribute
    # the first one ends the attribute and Alpine reads half an expression.
    if grep -rnE '="[^"]*\{\{[^}]*\|json\|safe' templates/; then
        echo "✗ a |json|safe inside a double-quoted attribute — its quotes end the attribute; use single quotes" >&2
        exit 1
    fi
    echo "✓ every |json is paired with |safe, and none sits inside double quotes"

# Words a member reads that never pass through the catalog stay English in a
# pt-BR session: `title="previous month"` sat on the month arrows for as long
# as they existed. A title, an aria-label, a placeholder or an alt with words
# in it must come from `{{ t.… }}`. Alpine bindings (`:title`) are expressions
# and are skipped; the product's name is the one literal allowed. Comments are
# stripped first — a rule that forbids a thing will quote the thing.
i18n-check:
    #!/usr/bin/env python3
    import re, sys, pathlib
    allowed = {"FFM – Family Finances Manager"}
    bad = []
    for path in sorted(pathlib.Path("templates").rglob("*.html")):
        if "sandbox" in path.parts:
            continue
        text = re.sub(r"\{#.*?#\}", lambda m: "\n" * m.group(0).count("\n"), path.read_text(), flags=re.S)
        for n, line in enumerate(text.splitlines(), 1):
            for m in re.finditer(r'(?<![:\w-])(title|aria-label|placeholder|alt)="([^"]*)"', line):
                value = m.group(2)
                if "{{{{" in value or "{%" in value or value in allowed:
                    continue
                if re.search(r"[A-Za-zÀ-ÿ]{2,}", value):
                    bad.append(f"{path}:{n}: {m.group(1)}=\"{value}\"")
    if bad:
        print("\n".join(bad))
        print("✗ words a member reads that skip the catalog — use {{{{ t.… }}", file=sys.stderr)
        sys.exit(1)
    print("✓ every title, aria-label, placeholder and alt comes from the catalog")

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
    # The command table — `web::commands::ALL` — is what the bar's hint, Tab
    # completion, its contextual help and `?`'s Type section are all rendered
    # from, so it is the one list the dispatcher is held against. Names and
    # aliases both: an alias is a real command a member types.
    rs = pathlib.Path("src/web/commands.rs").read_text()
    m = re.search(r"pub static ALL: .*?\n\];", rs, re.S)
    if not m:
        print("\u2717 commands::ALL not found in src/web/commands.rs"); sys.exit(1)
    table = m.group(0)
    advertised = set(re.findall(r'name: "(\w+)"', table))
    for group in re.findall(r"aliases: &\[(.*?)\]", table):
        advertised |= set(re.findall(r'"(\w+)"', group))
    html = pathlib.Path("templates/pages/ledger.html").read_text()
    handled = set(re.findall(r"case '(\w+)':", html))
    missing = sorted(advertised - handled)
    if missing:
        print("\u2717 the command table offers what runCommand does not handle: "
              + ", ".join(missing))
        sys.exit(1)
    # And the other way: a command the dispatcher handles but the table does
    # not name is one Tab, the hint and `?` can never show.
    unlisted = sorted(handled - advertised)
    if unlisted:
        print("\u2717 runCommand handles a command the command table does not "
              "name: " + ", ".join(unlisted))
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

# What the pre-commit hook runs: everything that takes seconds — formatting,
# the offline build (a query changed without `just prepare`), and the greps.
check-fast: fmt-check offline-check db-check css-check tpl-check i18n-check js-check cmd-check

# What the pre-push hook runs: the slow ones.
check-slow: lint test

check: check-fast check-slow

# Turn the versioned hooks in .githooks/ on for this clone. Git config is not
# versioned, so a fresh clone runs this once.
hooks:
    git config core.hooksPath .githooks
    @echo "✓ hooks on — pre-commit runs just check-fast, pre-push just check-slow"

# --- docs --------------------------------------------------------------------

md:
    mdserve -p 8080 .

# --- release -----------------------------------------------------------------

# Keep target/ from growing without bound. Cargo never deletes a superseded
# build of ffm: every feature set, every one of the test binaries and every
# version keeps its own, and `just release` bumps the version, which leaves a
# whole generation behind each time. target/ once reached 189 GB, and the
# bloated incremental cache broke linking under `just dev-lan` (undefined
# `.llvm.` symbols) until `cargo clean -p ffm`.
#
# - Over the limit (FFM_TARGET_MAX_GB, 30 by default) it runs
#   `cargo clean -p ffm`: only this package's output, in every profile and
#   target. Dependencies stay, so the next dev build is ~30 s.
# - It never runs a full `cargo clean` on its own — that is a from-scratch
#   build of every dependency. Still over after cleaning, it says so.
# - `just release` runs it first.
#
# Clean ffm's own build output when target/ is over FFM_TARGET_MAX_GB (30)
tidy:
    #!/usr/bin/env bash
    set -euo pipefail
    dir="${CARGO_TARGET_DIR:-target}"
    max="${FFM_TARGET_MAX_GB:-30}"
    [ -d "$dir" ] || { echo "✓ $dir/ does not exist — nothing to clean"; exit 0; }
    gb() { du -sk "$dir" | awk '{ printf "%d", $1 / 1048576 }'; }
    before=$(gb)
    if [ "$before" -le "$max" ] && [ "$max" -gt 0 ]; then
        echo "✓ $dir/ is $before GB, under the $max GB limit"
        exit 0
    fi
    echo "→ $dir/ is $before GB, over the $max GB limit · cleaning ffm's own build output (cargo clean -p ffm)"
    cargo clean -p ffm
    after=$(gb)
    echo "✓ $dir/ $before GB → $after GB"
    if [ "$after" -gt "$max" ] && [ "$max" -gt 0 ]; then
        echo "⚠ $dir/ is still over $max GB — the dependencies themselves; \`cargo clean\` empties it, at the cost of rebuilding them all" >&2
    fi

# Build a release into dist/v0.0.N/ — one folder and one .tar.gz per platform,
# each holding the binary, static/, ffm.example.toml,
# start.sh and README.md (from release/): everything a fresh machine needs
# besides poppler and qpdf.
#
# - macOS arm64 is built natively. Linux x86_64 and arm64 are cross-compiled
#   from this Mac with cargo-zigbuild (mise installs it and zig), for musl:
#   fully static binaries that run on any distro whatever its glibc, with no
#   container and no old-LTS build image.
# - The number is the last `v0.0.*` tag + 1, written into Cargo.toml before
#   the build so the binary reports it. Running this again before approving
#   reuses that number rather than skipping one, so tags stay gapless.
# - It refuses a dirty tree (Cargo.toml/Cargo.lock aside, which it bumps
#   itself): the tag `release-approve` makes must name the code the tested
#   binary was built from.
# - Then `just tidy`: target/ over 30 GB gets ffm's own output cleaned.
#
# Build dist/v0.0.N for macOS arm64 and Linux x86_64/arm64 · test it · then `just release-approve`
release:
    #!/usr/bin/env bash
    set -euo pipefail
    dirty=$(git status --porcelain --untracked-files=no -- . ':!Cargo.toml' ':!Cargo.lock')
    if [ -n "$dirty" ]; then
        echo "✗ uncommitted changes — a release is built from a commit, or its tag would name code the binary does not contain:" >&2
        echo "$dirty" >&2
        exit 1
    fi
    # Before the version bump below, which supersedes every ffm build in target/.
    just tidy
    last=$(git tag --list 'v0.0.*' | sed 's/^v0\.0\.//' | grep -E '^[0-9]+$' | sort -n | tail -1 || true)
    v="0.0.$(( ${last:-0} + 1 ))"
    tag="v$v"
    perl -0pi -e 's/(\[package\][^\[]*?\nversion = ")[^"]*(")/${1}'"$v"'${2}/' Cargo.toml
    prev="${last:+v0.0.$last}"
    echo "→ $tag (last tag: ${prev:-none})"

    just css-build
    export SQLX_OFFLINE=true
    tdir="${CARGO_TARGET_DIR:-target}"
    out="dist/$tag"
    rm -rf "$out"
    mkdir -p "$out"
    for t in aarch64-apple-darwin x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
        case "$t" in
            aarch64-apple-darwin)       plat=macos-arm64 ;;
            x86_64-unknown-linux-musl)  plat=linux-x86_64 ;;
            aarch64-unknown-linux-musl) plat=linux-arm64 ;;
        esac
        echo "→ building $plat ($t)"
        if [[ "$t" == *linux* ]]; then
            mise exec -- cargo zigbuild --release --quiet --target "$t"
        else
            cargo build --release --quiet --target "$t"
        fi
        name="ffm-$tag-$plat"
        dir="$out/$name"
        mkdir -p "$dir/static/css" "$dir/static/fonts"
        cp "$tdir/$t/release/ffm" "$dir/ffm"
        # Only what a page loads: build.css already inlines every source
        # stylesheet, and the sandbox's does not exist in a release build.
        cp static/css/build.css "$dir/static/css/"
        cp -R static/js static/icons "$dir/static/"
        cp static/fonts/*.woff2 static/fonts/OFL-*.txt "$dir/static/fonts/"
        cp ffm.example.toml release/start.sh release/README.md "$dir/"
        echo "$tag" > "$dir/VERSION"
        tar -czf "$out/$name.tar.gz" -C "$out" "$name"
    done
    git rev-parse HEAD > "$out/COMMIT"
    (cd "$out" && shasum -a 256 ./*.tar.gz > SHA256SUMS)
    echo "✓ $tag built from $(git rev-parse --short HEAD) — $out:"
    (cd "$out" && ls -1 ./*.tar.gz | sed 's|^\./|    |')
    echo "  test it, then \`just release-approve\` commits the version and tags $tag"

# Approve the release `just release` built: commit Cargo.toml's version and
# tag it `v0.0.N`. Nothing is pushed — `git push && git push origin v0.0.N`
# when you want it on the remote.
#
# It refuses when HEAD has moved since the build: the tag would then name code
# the tested binary does not contain.
#
# After testing what `just release` built: commit the version and tag v0.0.N (nothing pushed)
release-approve:
    #!/usr/bin/env bash
    set -euo pipefail
    v=$(perl -0ne 'print $1 if /\[package\][^\[]*?\nversion = "([^"]*)"/' Cargo.toml)
    tag="v$v"
    out="dist/$tag"
    if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
        echo "✗ $tag is already tagged — \`just release\` builds the next one" >&2
        exit 1
    fi
    if [ ! -f "$out/COMMIT" ]; then
        echo "✗ nothing built for $tag — run \`just release\` first" >&2
        exit 1
    fi
    if [ "$(cat "$out/COMMIT")" != "$(git rev-parse HEAD)" ]; then
        echo "✗ HEAD has moved since $tag was built — run \`just release\` again and test that" >&2
        exit 1
    fi
    dirty=$(git status --porcelain --untracked-files=no -- . ':!Cargo.toml' ':!Cargo.lock')
    if [ -n "$dirty" ]; then
        echo "✗ uncommitted changes besides the version — the tag must name exactly what was built:" >&2
        echo "$dirty" >&2
        exit 1
    fi
    git add Cargo.toml Cargo.lock
    if ! git diff --cached --quiet; then
        git commit -q -m "release: $tag"
    fi
    git tag -a "$tag" -m "ffm $tag"
    echo "✓ tagged $tag at $(git rev-parse --short HEAD) · not pushed — git push && git push origin $tag"
