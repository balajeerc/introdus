#!/usr/bin/env bash
# Regenerate the root AGENTS.md from the canonical rule sources in agent_rules/.
#
# agent_rules/ is the single source of truth for every agent's project rules:
#   - Claude Code reads .claude/rules/*.md  (symlinks -> ../../agent_rules/*.md)
#   - Codex, Pi, and opencode each auto-discover a single root AGENTS.md, which
#     they cannot glob from a directory — so we concatenate agent_rules/*.md into
#     one AGENTS.md here.
#
# AGENTS.md is a GENERATED FILE. Never hand-edit it — edit agent_rules/*.md and
# rerun this script. `scripts/lint.sh --quick` runs the --check mode below to
# catch drift.
#
#   scripts/gen-agent-rules.sh          # (re)write AGENTS.md
#   scripts/gen-agent-rules.sh --check  # exit non-zero if AGENTS.md is stale

set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_DIR"

SRC_DIR="agent_rules"
OUT="AGENTS.md"

generate() {
    cat <<'EOF'
<!--
  GENERATED FILE — DO NOT EDIT.

  Concatenated from agent_rules/*.md by scripts/gen-agent-rules.sh, for the
  coding agents (Codex, Pi, opencode) that auto-load a single root AGENTS.md
  and cannot glob a rules directory. Edit the sources in agent_rules/ and rerun
  scripts/gen-agent-rules.sh.
-->
EOF
    local f
    for f in "$SRC_DIR"/*.md; do
        printf '\n'
        cat "$f"
        # Guarantee each section ends with a newline even if the source doesn't,
        # so the next section's heading stays on its own line.
        if [ -n "$(tail -c1 "$f")" ]; then printf '\n'; fi
    done
}

if [[ "${1:-}" == "--check" ]]; then
    if ! diff -q <(generate) "$OUT" >/dev/null 2>&1; then
        echo "AGENTS.md is out of date — run scripts/gen-agent-rules.sh" >&2
        exit 1
    fi
    echo "AGENTS.md is up to date"
    exit 0
fi

generate >"$OUT"
echo "wrote $OUT from $SRC_DIR/*.md"
