#!/usr/bin/env bash
# Install a git pre-commit hook that runs the fast test suite (cargo test) and
# then the lint suite.
#
# Usage:
#   ./scripts/install-pre-commit.sh             # default: --security (superset of --full)
#   ./scripts/install-pre-commit.sh --full      # everything except semgrep
#   ./scripts/install-pre-commit.sh --quick     # only fmt + clippy
#
# Idempotent — overwrites any prior hook this script wrote, but refuses to
# clobber a hook it didn't write so a user's bespoke hook isn't lost silently.

set -euo pipefail

MODE="--security"
case "${1:-}" in
    "" | --security) MODE="--security" ;;
    --full)          MODE="--full" ;;
    --quick)         MODE="--quick" ;;
    -h | --help)
        sed -n '2,9p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    *) echo "unknown mode: $1" >&2; exit 2 ;;
esac

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOOK_PATH="$REPO_DIR/.git/hooks/pre-commit"
MARKER="# managed-by: introdus scripts/install-pre-commit.sh"

if [[ -e "$HOOK_PATH" ]] && ! grep -q "$MARKER" "$HOOK_PATH" 2>/dev/null; then
    echo "error: $HOOK_PATH already exists and wasn't written by this script." >&2
    echo "       Move it aside if you really want this hook installed." >&2
    exit 1
fi

mkdir -p "$(dirname "$HOOK_PATH")"
cat > "$HOOK_PATH" <<EOF
#!/usr/bin/env bash
# managed-by: introdus scripts/install-pre-commit.sh
set -euo pipefail
REPO_DIR="\$(git rev-parse --show-toplevel)"
cd "\$REPO_DIR"
echo "==> pre-commit: fast test suite (cargo test --workspace)"
cargo test --workspace --quiet
echo "==> pre-commit: lint suite (scripts/lint.sh $MODE)"
exec "\$REPO_DIR/scripts/lint.sh" $MODE
EOF
chmod +x "$HOOK_PATH"
echo "Installed pre-commit hook at $HOOK_PATH"
echo "It runs: cargo test --workspace, then scripts/lint.sh $MODE"
echo
echo "Bypass for a single commit with: git commit --no-verify"
echo "Switch modes with: ./scripts/install-pre-commit.sh --quick|--full|--security"
echo "Uninstall with:    rm $HOOK_PATH"
