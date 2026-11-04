#!/usr/bin/env bash
# The browser tests, in a container, so several worktrees can run at once.
#
#   scripts/e2e.sh run [playwright args]   build, generate the data, test
#   scripts/e2e.sh report                  serve the last run's report
#   scripts/e2e.sh clean                   this worktree's build volume and output
#
# The runtime is Apple's `container` on macOS and `podman` on Linux, or
# whatever FFM_E2E_RUNTIME names. Only flags both have are used.
#
# Environment: E2E_SEED (the data, random if unset), E2E_TODAY, E2E_WORKERS,
# E2E_CPUS, E2E_MEMORY, E2E_DNS.
set -euo pipefail

repo=$(git rev-parse --show-toplevel)
cd "$repo"

runtime=${FFM_E2E_RUNTIME:-}
if [ -z "$runtime" ]; then
    case "$(uname -s)" in
    Darwin) runtime=container ;;
    *) runtime=podman ;;
    esac
fi
if ! command -v "$runtime" >/dev/null 2>&1; then
    case "$runtime" in
    container) hint="brew install container && container system start" ;;
    podman) hint="install podman with your distro's package manager" ;;
    *) hint="set FFM_E2E_RUNTIME to a runtime that is installed" ;;
    esac
    echo "✗ $runtime is not installed — $hint" >&2
    exit 1
fi

# One name per worktree: the folder alone could repeat across checkouts.
sha() { shasum -a 256 2>/dev/null || sha256sum; }
base=$(basename "$repo" | tr -c 'a-zA-Z0-9\n' '-' | tr 'A-Z' 'a-z')
slug="$base-$(printf %s "$repo" | sha | cut -c1-8)"
name="ffm-e2e-$slug"
# Holds the build and the Cargo registry together. An Apple volume is a block
# device attached to one VM, so two worktrees cannot share a registry volume.
volume="ffm-e2e-$slug"
run_dir="$repo/e2e/.run"

# SELinux relabels a bind mount for Podman; Apple's runtime has no such option.
bind() {
    if [ "$runtime" = podman ]; then printf '%s:%s:Z' "$1" "$2"; else printf '%s:%s' "$1" "$2"; fi
}

# Apple's gateway resolver does not answer on every network, so name one.
dns_flags=()
if [ "$runtime" = container ]; then
    dns_flags=(--dns "${E2E_DNS:-1.1.1.1}")
elif [ -n "${E2E_DNS:-}" ]; then
    dns_flags=(--dns "$E2E_DNS")
fi

ensure_service() {
    if [ "$runtime" = container ] && ! container system status >/dev/null 2>&1; then
        echo "→ starting the container service"
        container system start --enable-kernel-install >/dev/null
    fi
    # The builder VM asks for Rosetta by default and will not start without it,
    # and this image is native. Read at service start, hence the restart.
    if [ "$runtime" = container ] && [ ! -e /Library/Apple/usr/libexec/oah/libRosettaRuntime ] &&
        container system property list | grep -q '^rosetta = true'; then
        echo "✗ Apple's image builder needs Rosetta, which is not installed. Turn that off:" >&2
        echo "    mkdir -p ~/.config/container && printf '[build]\\nrosetta = false\\n' >> ~/.config/container/config.toml" >&2
        echo "    container system stop && container system start" >&2
        exit 1
    fi
}

# The tag is a hash of everything in the image, so worktrees that agree on it
# share one image and a change to any of it builds a new one.
ensure_image() {
    local pw rust tw tag
    pw=$(sed -n 's/.*"@playwright\/test": "\([^"]*\)".*/\1/p' e2e/image/package.json)
    rust=$(rustc --version | cut -d' ' -f2)
    tw=$(sed -n 's/.*tailwindcss" = "\([^"]*\)".*/\1/p' mise.toml)
    tag=$(cat e2e/image/Containerfile e2e/image/package.json e2e/image/package-lock.json |
        { sha; printf '%s %s\n' "$rust" "$tw"; } | sha | cut -c1-12)
    image="ffm-e2e:$tag"
    if "$runtime" image inspect "$image" >/dev/null 2>&1; then
        return
    fi
    echo "→ building the image $image (playwright $pw · rust $rust · tailwind $tw)"
    # Only e2e/image goes to the builder — never the repository.
    "$runtime" build "${dns_flags[@]}" -t "$image" \
        --build-arg "PLAYWRIGHT_VERSION=$pw" \
        --build-arg "RUST_VERSION=$rust" \
        --build-arg "TAILWIND_VERSION=$tw" \
        -f e2e/image/Containerfile e2e/image
}

# What a run may see, by name. A worktree also holds ffm.db, receipts/, .env
# and _backups/ — real bills, some of it not even ignored by git — so this is
# an allow-list and never the repository with exclusions. `-a` keeps mtimes, so
# Cargo stays incremental.
stage() {
    mkdir -p "$run_dir/src" "$run_dir/out"
    rsync -a --delete \
        --exclude '/static/css/build.css' \
        --exclude '/e2e/.run' \
        Cargo.toml Cargo.lock build.rs .sqlx src migrations templates i18n static seeds e2e \
        "$run_dir/src/"
}

cmd=${1:-run}
[ $# -gt 0 ] && shift

case "$cmd" in
run)
    ensure_service
    ensure_image
    stage
    "$runtime" volume inspect "$volume" >/dev/null 2>&1 || "$runtime" volume create "$volume" >/dev/null

    # A name still taken here is either a crashed run, which can go, or a live
    # one: `rm` refuses that, and `run` then says the name is in use.
    "$runtime" rm "$name" >/dev/null 2>&1 || true

    # /work is tmpfs: SQLite's WAL and the watcher's inotify do not belong on a
    # shared filesystem. Nothing is published, so no two runs can collide.
    exec "$runtime" run --rm --init --name "$name" \
        "${dns_flags[@]}" \
        --cpus "${E2E_CPUS:-6}" --memory "${E2E_MEMORY:-6g}" --shm-size 1g \
        --tmpfs /work \
        -v "$(bind "$run_dir/src" /src)" \
        -v "$(bind "$run_dir/out" /out)" \
        -v "$volume:/cache" \
        -e "E2E_SEED=${E2E_SEED:-}" \
        -e "E2E_TODAY=${E2E_TODAY:-}" \
        -e "E2E_WORKERS=${E2E_WORKERS:-}" \
        "$image" bash /src/e2e/container-entry.sh "$@"
    ;;
report)
    [ -f "$run_dir/out/report/index.html" ] || {
        echo "✗ no report yet — run \`just e2e\` first" >&2
        exit 1
    }
    # Served, because a trace cannot be opened from a file:// page. Port 0 picks
    # a free one, so two worktrees can each show their own.
    exec python3 -m http.server 0 --bind 127.0.0.1 -d "$run_dir/out/report"
    ;;
clean)
    "$runtime" volume rm "$volume" >/dev/null 2>&1 || "$runtime" volume delete "$volume" >/dev/null 2>&1 || true
    rm -rf "$run_dir"
    echo "✓ removed $volume and e2e/.run"
    ;;
*)
    echo "usage: scripts/e2e.sh run|report|clean [args]" >&2
    exit 2
    ;;
esac
