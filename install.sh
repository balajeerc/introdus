#!/usr/bin/env bash
# Installs create-dev-container.sh onto your PATH by symlinking it into
# ~/.local/bin, so you can run `create-dev-container.sh` from anywhere (e.g.
# from your home directory) to bootstrap and launch a new dev container.
#
# A symlink (not a copy) is used so the installed command always tracks this
# harness checkout — `git pull` here updates the command with no reinstall, and
# create-dev-container.sh resolves the symlink back to find launch.sh/sample.env.
#
# Usage:
#   ./install.sh
set -euo pipefail

HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="$HARNESS_DIR/create-dev-container.sh"
[[ -f "$SRC" ]] || { echo "error: create-dev-container.sh not found at $SRC" >&2; exit 1; }
chmod +x "$SRC"

BIN_DIR="$HOME/.local/bin"
DEST="$BIN_DIR/create-dev-container.sh"

mkdir -p "$BIN_DIR"

# Replace any existing install (a stale symlink or an older copy).
if [[ -e "$DEST" || -L "$DEST" ]]; then
    rm -f "$DEST"
fi
ln -s "$SRC" "$DEST"

echo "Installed: $DEST"
echo "        -> $SRC"
echo

# ---- PATH guidance ---------------------------------------------------------

case ":$PATH:" in
    *":$BIN_DIR:"*)
        echo "$BIN_DIR is already on your PATH — you're all set:"
        echo
        echo "    cd ~ && create-dev-container.sh"
        ;;
    *)
        echo "NOTE: $BIN_DIR is not on your PATH yet."
        echo
        echo "Add it by appending this line to your ~/.bashrc:"
        echo
        echo '    export PATH="$HOME/.local/bin:$PATH"'
        echo
        echo "Then reload your shell (or open a new terminal):"
        echo
        echo "    source ~/.bashrc"
        echo
        echo "After that, run from anywhere (e.g. your home directory):"
        echo
        echo "    cd ~ && create-dev-container.sh"
        ;;
esac
