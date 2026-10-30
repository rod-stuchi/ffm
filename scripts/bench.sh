#!/usr/bin/env bash
# The performance baseline (PERF_PLAN.md, Phase 0): wall time and SQL
# statement count for the requests the ledger makes most, against a COPY of
# a database and its vault — never the live ones.
#
#   scripts/bench.sh <ffm binary> <ffm.db> <receipts dir> [member name] [password]
#
# Run it from the repository root (the server serves ./static from there).
# Build the database with `just anon --from <snapshot> --scale 2 --years 3`
# so the numbers mean something; every member's password there is `ffm`.
#
# Two servers, one after the other, on one fresh copy:
#   1. with sqlx's statement log on, to count statements per request — once
#      the startup work (materializer, vault catch-up) has gone quiet, so none
#      of it is counted against a request;
#   2. with it off, to time requests without the logging in the way.
# Startup is timed in the second: launch to the first answer on /login, on a
# copy the first already brought up to date.
set -euo pipefail

bin=${1:?ffm binary}
db=${2:?ffm.db}
vault=${3:?receipts dir}
who=${4:-caio}
pw=${5:-ffm}
port=${BENCH_PORT:-4790}
runs=${BENCH_RUNS:-7}

[ -d static ] || { echo "run from the repository root" >&2; exit 1; }
if lsof -tiTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
    echo "port $port is taken; set BENCH_PORT" >&2; exit 1
fi

work=$(mktemp -d -t ffm-bench-XXXX)
pid=""
stop() {
    [ -n "$pid" ] && kill -TERM "$pid" 2>/dev/null && wait "$pid" 2>/dev/null || true
    pid=""
}
trap 'stop; rm -rf "$work"' EXIT

period=$(date +%Y-%m)
base="http://127.0.0.1:$port"

# Fresh copy of the database and the vault in $work/run.
fresh() {
    rm -rf "$work/run"
    mkdir -p "$work/run/backups"
    sqlite3 "$db" "VACUUM INTO '$work/run/ffm.db'"
    cp -R "$vault" "$work/run/receipts"
}

# Start a server on the copy; sets $boot to the milliseconds from launch to
# the first answer. Not printed: a command substitution would run it in a
# subshell, and the server's pid would never reach `stop`.
start() {
    local log=$1 filter=$2
    local t0 t1
    t0=$(python3 -c 'import time; print(int(time.time()*1000))')
    FFM_DB_PATH="$work/run/ffm.db" FFM_RECEIPTS_ROOT="$work/run/receipts" \
        FFM_BACKUP_DIR="$work/run/backups" FFM_ADDR="127.0.0.1:$port" \
        RUST_LOG="$filter" "$bin" >"$log" 2>&1 &
    pid=$!
    for _ in $(seq 600); do
        curl -s -o /dev/null "$base/login" && break
        sleep 0.05
    done
    t1=$(python3 -c 'import time; print(int(time.time()*1000))')
    boot=$((t1 - t0))
}

login() {
    curl -s -o /dev/null -c "$work/jar" -d "name=$who&password=$pw" "$base/login"
    rg -q ffm.sid "$work/jar" || { echo "login as $who failed" >&2; exit 1; }
}

# `/undo` is POSTed, its query as the form: `u`'s first press only answers
# what it would put back and writes nothing, so it can be timed again and
# again.
get() {
    local url=$base$1 post=()
    case $1 in
    /undo\?*) url=$base${1%%\?*}; post=(--data "${1#*\?}") ;;
    esac
    curl -s -o /dev/null -b "$work/jar" -H 'HX-Request: true' "${post[@]}" \
        -w '%{time_total} %{size_download}\n' "$url"
}

# A tracked live row of the signed-in member this month, for the row fetch.
pick_ids() {
    local me
    me=$(sqlite3 "$work/run/ffm.db" "SELECT id FROM members WHERE name = lower('$who')")
    row=$(sqlite3 "$work/run/ffm.db" "
        SELECT t.id FROM transactions t JOIN recurring_rules r ON r.id = t.rule_id
        WHERE t.member_id = $me AND t.period = '$period' AND t.deleted_at IS NULL
          AND r.tracked = 1 ORDER BY t.sort_key LIMIT 1")
    member=$me
    # A varying income's rule, for `E` — its estimate box walks a year back.
    # Your own first; another member's only when you are the admin.
    vary=$(sqlite3 "$work/run/ffm.db" "
        SELECT r.id FROM recurring_rules r
        WHERE r.deleted_at IS NULL AND r.varies = 1
          AND (r.member_id = $me OR (SELECT role FROM members WHERE id = $me) = 0)
        ORDER BY r.member_id = $me DESC, r.id LIMIT 1")
}

fresh
pick_ids
paths=(
    "/?period=$period"
    "/tx/$row"
    "/panes/$member/$period"
    "/months/dot?period=$period"
    "/months?period=$period"
    "/charts?tab=month&period=$period"
    "/charts?tab=year&period=$period"
    "/charts?tab=years&period=$period"
)
[ -n "$vary" ] && paths+=("/rules/$vary/edit?period=$period")
paths+=("/undo?member=$member&period=$period" "/household")

# --- 1. statements per request ------------------------------------------------
start "$work/count.log" "warn,sqlx::query=debug"
login
# Startup leaves work running in the background (the watcher's catch-up
# reconciles every tracked row, the materializer, the backup). Its statements
# would be counted against whichever request happened to be in flight, so
# wait until the log has been quiet for a second.
quiet=0
for _ in $(seq 240); do
    c1=$(rg -c 'sqlx::query' "$work/count.log" || echo 0)
    sleep 0.5
    c2=$(rg -c 'sqlx::query' "$work/count.log" || echo 0)
    if [ "$c1" = "$c2" ]; then quiet=$((quiet + 1)); else quiet=0; fi
    [ "$quiet" -ge 2 ] && break
done
get "${paths[0]}" >/dev/null  # warm: the first request after boot may differ
declare -A stmts
for p in "${paths[@]}"; do
    before=$(rg -c 'sqlx::query' "$work/count.log" || echo 0)
    get "$p" >/dev/null
    sleep 0.2
    after=$(rg -c 'sqlx::query' "$work/count.log" || echo 0)
    stmts[$p]=$((after - before))
done
stop

# --- 2. wall time per request -------------------------------------------------
# On the same copy, which the first server has already brought up to date (a
# fresh copy has months to materialize). This boot is the steady-state restart
# a household sees; its catch-up still walks the vault, so give it a moment.
start "$work/time.log" "warn"
login
sleep 3
printf '%-40s %8s %8s %8s %10s\n' request median min stmts bytes
for p in "${paths[@]}"; do
    get "$p" >/dev/null
    times=()
    size=0
    for _ in $(seq "$runs"); do
        read -r t s < <(get "$p")
        times+=("$t"); size=$s
    done
    read -r med min < <(printf '%s\n' "${times[@]}" | sort -n |
        awk '{a[NR]=$1} END {printf "%.1f %.1f\n", a[int((NR+1)/2)]*1000, a[1]*1000}')
    printf '%-40s %6sms %6sms %8s %10s\n' "$p" "$med" "$min" "${stmts[$p]}" "$size"
done
echo "startup to first answer: ${boot}ms"
