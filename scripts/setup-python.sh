#!/bin/sh
# Creates the Python environment that mao's streamer uses, with the pinned versions.
# Set MAO_PYTHON_BASE to choose the interpreter (default: Homebrew Python 3.12).
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
BASE=${MAO_PYTHON_BASE:-/opt/homebrew/bin/python3.12}
VENV="$HOME/Library/Application Support/au.com.lateralblast.mao/venv"

# Keep pip's "new version available" notice out of the output.
export PIP_DISABLE_PIP_VERSION_CHECK=1

"$BASE" -m venv "$VENV"
"$VENV/bin/pip" install --quiet --upgrade pip
"$VENV/bin/pip" install --quiet -r "$ROOT/python/requirements.txt"
echo "python environment ready: $VENV"
