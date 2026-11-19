#!/bin/sh
# mao: build and run the app.
#
#   ./mao           set up anything missing, build the app bundle and open it
#   ./mao --dev     run the development build (npm run dev)
#   ./mao --stop    quit the running app
#   ./mao --version print the version
#   ./mao --help    show this help
set -eu

ROOT=$(cd "$(dirname "$0")" && pwd)
PYTHON="$HOME/Library/Application Support/au.com.lateralblast.mao/venv/bin/python"
APP="$ROOT/src-tauri/target/release/bundle/macos/mao.app"

say() { printf 'mao: %s\n' "$*"; }
version() { sed -n 's/^  "version": "\([^"]*\)".*/\1/p' "$ROOT/package.json" | head -1; }
fail() { printf 'mao: %s\n' "$*" >&2; exit 1; }

need() {
    command -v "$1" >/dev/null 2>&1 || fail "'$1' is required. $2"
}

usage() {
    sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'
}

stop() {
    # Escape the bundle path so it matches literally in pkill's pattern.
    pattern=$(printf '%s' "$APP/Contents/MacOS/mao" | sed 's/[.[\*^$/]/\\&/g')
    if pkill -f "${pattern}\$" 2>/dev/null; then
        say "stopped"
    else
        say "not running"
    fi
}

check_tools() {
    need xcrun "Install the Xcode command line tools: xcode-select --install."
    need cargo "Install Rust from https://rustup.rs."
    need npm "Install Node.js."
}

check_python() {
    if [ ! -x "$PYTHON" ]; then
        say "setting up the Python environment (one time)"
        sh "$ROOT/scripts/setup-python.sh"
    fi
}

check_node_modules() {
    if [ ! -d "$ROOT/node_modules" ]; then
        say "installing npm packages"
        (cd "$ROOT" && npm install)
    fi
}

build_app() {
    say "building the audio tap"
    sh "$ROOT/scripts/build-tap.sh"
    say "freezing the streamer"
    sh "$ROOT/scripts/build-streamer.sh"
    say "building the app bundle"
    (cd "$ROOT" && npx tauri build --bundles app)
}

run_app() {
    [ -d "$APP" ] || fail "the app bundle was not built"
    stop >/dev/null
    say "opening $APP"
    open "$APP"
}

case "${1:-}" in
    --help|-h)
        usage
        ;;
    --version|-V)
        printf 'mao %s\n' "$(version)"
        ;;
    --stop)
        stop
        ;;
    --dev)
        check_tools
        check_python
        check_node_modules
        say "starting the development build"
        cd "$ROOT"
        exec npm run dev
        ;;
    "")
        check_tools
        check_python
        check_node_modules
        build_app
        run_app
        ;;
    *)
        fail "unknown option '$1' (try --help)"
        ;;
esac
