#!/usr/bin/env bash
# Run the project's linting suite.
#
# Modes (each is a strict superset of the previous one):
#   --quick     fmt + clippy. ~10 s warm.
#   --full      adds tokei file-size budget, cargo-machete, cargo-deny,
#               cargo-audit, jscpd duplication detection.
#   --security  adds semgrep (Rust rule pack).
#
# Default is --quick. `./scripts/install-pre-commit.sh` defaults to --security
# so every commit runs the whole suite.
#
# Philosophy: every check the mode would run is REQUIRED. If a tool isn't
# installed, the script accumulates failures across the full run (so you see
# every install you owe in one pass) and exits non-zero. No silent skips —
# that hides regressions behind missing tooling.

set -uo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_DIR"

MODE="quick"
case "${1:-}" in
    --quick|"")  MODE="quick" ;;
    --full)      MODE="full" ;;
    --security)  MODE="security" ;;
    -h|--help)
        sed -n '2,18p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    *) echo "unknown mode: $1" >&2; exit 2 ;;
esac

PASS=0
FAIL=0
FAIL_NAMES=()
MISSING=()

bold()  { printf "\033[1m%s\033[0m" "$*"; }
green() { printf "\033[32m%s\033[0m" "$*"; }
red()   { printf "\033[31m%s\033[0m" "$*"; }

run() {
    local name="$1"; shift
    echo
    echo "$(bold "==> $name")"
    if "$@"; then
        PASS=$((PASS+1))
        echo "$(green "✓ $name")"
    else
        FAIL=$((FAIL+1))
        FAIL_NAMES+=("$name")
        echo "$(red "✗ $name")"
    fi
}

# Record a missing tool as a failure so the user has to install it before the
# lint passes. Returns 0 if present (caller should run the check), non-zero if
# missing (caller should skip — we've already recorded the failure).
require_cmd() {
    local cmd="$1" install_hint="$2"
    if command -v "$cmd" >/dev/null 2>&1; then
        return 0
    fi
    FAIL=$((FAIL+1))
    FAIL_NAMES+=("$cmd (not installed)")
    MISSING+=("$cmd: $install_hint")
    echo
    echo "$(red "✗ $cmd is not installed")"
    echo "  install with: $install_hint"
    return 1
}

# Locally-installed jscpd lives under tools/node_modules so we don't pollute the
# user's global bin. Prints the executable path on stdout, or empty if absent.
local_jscpd() {
    local bin="$REPO_DIR/tools/node_modules/.bin/jscpd"
    if [[ -x "$bin" ]]; then
        printf '%s\n' "$bin"
    fi
}

# ---- ensure cargo is on PATH (works even from a fresh shell) ----------------
if ! command -v cargo >/dev/null 2>&1 && [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
fi

# -------------------- quick (always) --------------------

if require_cmd cargo "https://rustup.rs (rustup-init)"; then
    run "rustfmt" cargo fmt --all -- --check
    run "clippy"  cargo clippy --workspace --all-targets --no-deps -- -D warnings
fi

# AGENTS.md is generated from agent_rules/*.md — fail if it has drifted from the
# sources (see scripts/gen-agent-rules.sh).
run "AGENTS.md up-to-date" "$REPO_DIR/scripts/gen-agent-rules.sh" --check

# -------------------- full (heavier Rust checks) --------------------

if [[ "$MODE" == "full" || "$MODE" == "security" ]]; then
    if require_cmd tokei "cargo install tokei"; then
        # Cap any single Rust source file at 600 lines of code.
        run "tokei (Rust file-size budget ≤ 600 lines)" bash -c '
            tokei -t Rust --output json crates/ |
            python3 -c "
import json, sys
data = json.load(sys.stdin)
limit = 600
violators = []
for report in data.get(\"Rust\", {}).get(\"reports\", []):
    if report.get(\"stats\", {}).get(\"code\", 0) > limit:
        violators.append((report[\"name\"], report[\"stats\"][\"code\"]))
if violators:
    for name, n in violators:
        print(f\"  {name}: {n} lines\")
    sys.exit(1)
print(f\"  all files under {limit} lines\")
"
        '
    fi

    if require_cmd cargo-machete "cargo install cargo-machete"; then
        run "cargo-machete (unused deps)" cargo machete
    fi

    if require_cmd cargo-deny "cargo install cargo-deny --locked"; then
        run "cargo-deny" cargo deny --no-default-features check
    fi

    if require_cmd cargo-audit "cargo install cargo-audit --locked"; then
        run "cargo-audit" cargo audit
    fi

    # jscpd lives under tools/ via pnpm — keep the Node ecosystem out of the
    # user's global bin. Any clone found is a failure. `--exitCode 1` makes
    # jscpd return non-zero when clones.length > 0, regardless of the
    # total-lines percentage threshold. Drive duplication out via refactoring,
    # never by softening this gate.
    jscpd_bin="$(local_jscpd)"
    if [[ -z "$jscpd_bin" ]]; then
        FAIL=$((FAIL+1))
        FAIL_NAMES+=("jscpd (not installed)")
        MISSING+=("jscpd: cd tools && pnpm install")
        echo
        echo "$(red "✗ jscpd not installed (looked for tools/node_modules/.bin/jscpd)")"
        echo "  install with: cd tools && pnpm install"
    else
        run "jscpd (copy-paste duplication)" "$jscpd_bin" \
            --pattern "crates/**/*.rs" \
            --ignore "**/target/**,**/node_modules/**" \
            --min-lines 8 \
            --min-tokens 50 \
            --exitCode 1 \
            --reporters consoleFull \
            "$REPO_DIR"
    fi
fi

# -------------------- security (opt-in, heavy) --------------------

if [[ "$MODE" == "security" ]]; then
    if require_cmd semgrep "pipx install semgrep"; then
        run "semgrep" semgrep --error --config p/rust \
            --exclude target --exclude node_modules \
            "$REPO_DIR"
    fi
fi

# -------------------- summary --------------------

echo
echo "$(bold "================ lint summary ================")"
echo "mode:   $MODE"
echo "passed: $PASS"
echo "failed: $FAIL"
if (( FAIL > 0 )); then
    echo
    echo "$(red "failed checks:")"
    for n in "${FAIL_NAMES[@]}"; do
        echo "  - $n"
    done
    if (( ${#MISSING[@]} > 0 )); then
        echo
        echo "$(bold "to install missing tools:")"
        for m in "${MISSING[@]}"; do
            echo "  $m"
        done
    fi
    exit 1
fi
exit 0
