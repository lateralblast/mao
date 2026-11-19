#!/bin/sh
# Freezes python/mao_stream.py into a single executable with PyInstaller, so the app
# bundle does not need Python. The output is src-tauri/binaries/mao-stream-<triple>,
# which Tauri places in the bundle as an external binary.
#
# The build uses the mao Python environment (scripts/setup-python.sh). Set MAO_PYTHON
# to use another interpreter.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TRIPLE=$(rustc -vV | sed -n 's/^host: //p')
PYTHON="${MAO_PYTHON:-$HOME/Library/Application Support/au.com.lateralblast.mao/venv/bin/python}"
WORK="$ROOT/src-tauri/target/streamer"
OUT="$ROOT/src-tauri/binaries/mao-stream-$TRIPLE"

[ -x "$PYTHON" ] || { echo "build-streamer: no Python environment at $PYTHON: run scripts/setup-python.sh" >&2; exit 1; }

"$PYTHON" -m pip install --quiet -r "$ROOT/python/requirements-build.txt"

rm -rf "$WORK"
mkdir -p "$WORK" "$(dirname "$OUT")"
"$PYTHON" -m PyInstaller --onefile --name mao-stream --noconfirm --clean --log-level WARN \
    --distpath "$WORK/dist" --workpath "$WORK/build" --specpath "$WORK" \
    "$ROOT/python/mao_stream.py"

cp "$WORK/dist/mao-stream" "$OUT"
echo "built $OUT"
