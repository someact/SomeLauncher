#!/usr/bin/env bash
# Development entry point. Runs everything from the repo, never the installed copy.
#   scripts/dev.sh build     build the Rust core (debug)
#   scripts/dev.sh test      cargo tests
#   scripts/dev.sh daemon    run somelauncherd in the foreground (logs to the terminal)
#   scripts/dev.sh ui [page] run the UI from ui/ against the debug daemon
#   scripts/dev.sh check     load the UI for a few seconds and print QML warnings and errors
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
export SOMELAUNCHER_DAEMON="$root/core/target/debug/somelauncherd"

build() { cargo build --manifest-path "$root/core/Cargo.toml" "$@"; }

case "${1:-}" in
    build) shift; build "$@" ;;
    test) shift; cargo test --manifest-path "$root/core/Cargo.toml" "$@" ;;
    daemon) build -q; SOMELAUNCHER_LOG="${SOMELAUNCHER_LOG:-debug}" exec "$SOMELAUNCHER_DAEMON" ;;
    ui) build -q; SOMELAUNCHER_PAGE="${2:-}" exec qs -p "$root/ui/shell.qml" ;;
    check)
        build -q
        timeout "${2:-8}" qs -p "$root/ui/shell.qml" 2>&1 | grep -E "WARN|ERROR" | grep -vE "qt.qpa.wayland.textinput|ServerNotFoundError|dropped operation" || true ;;
    *) sed -n '2,8p' "$0"; exit 1 ;;
esac
