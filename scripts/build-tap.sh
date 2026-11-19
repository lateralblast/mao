#!/bin/sh
# Builds the Swift system-audio helper into src-tauri/binaries, where Tauri
# finds it as an external binary.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TRIPLE=$(rustc -vV | sed -n 's/^host: //p')
OUT_DIR="$ROOT/src-tauri/binaries"
mkdir -p "$OUT_DIR"
xcrun swiftc -O "$ROOT/tap/mao-tap.swift" -o "$OUT_DIR/mao-tap-$TRIPLE"
echo "built $OUT_DIR/mao-tap-$TRIPLE"
