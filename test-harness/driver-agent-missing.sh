#!/usr/bin/env bash
# Launching an agent that is SELECTED in .env but whose binary never actually
# installed (install-agents is deliberately never-fatal — a blocked-egress or
# pnpm failure leaves the agent absent) must NOT flash a dead tmux window that
# exits 127. Instead the menu detects the missing binary, says so, and offers to
# install it. This drives BOTH branches of that guard:
#   1. decline  -> the guard fires and nothing is launched (no dead window)
#   2. accept   -> install-agents runs, the agent installs, and it launches
# Phase 2 also regression-guards the pnpm PATH fix: pnpm's global bin dir is
# $PNPM_HOME itself, so the agent binary must resolve and launch after install.
# Covers TEST_PLAN: TA123
set -euo pipefail
source /usr/local/bin/driver-common.sh

session="harness-session"
proj="$HOME/proj-agent-missing"

harness_dummy_key
harness_write_env "$proj" "$session" "claude"   # only claude installs at setup
harness_ensure_base "$proj"
harness_clean
cd "$proj"

harness_launch "$session" "$proj"
cname="$HARNESS_CNAME"

# Wait for claude so we have a live, healthy container to drive.
harness_poll "claude installed in the container" \
    podman exec --user dev "$cname" bash -lc 'command -v claude'
echo "    ✓ claude present in the container"

# pi must be genuinely ABSENT (never installed). Confirm that up front so the
# test asserts the real "selected but missing" condition, not a flake.
if podman exec --user dev "$cname" sh -c 'command -v pi' >/dev/null 2>&1; then
    echo "FATAL: pi unexpectedly present — cannot test the missing-binary guard"; exit 1
fi
echo "    ✓ pi is absent (as expected)"

# Now SELECT pi in .env without installing it. The menu reloads .env every
# STATUS_POLL (2s) tick, so after the rewrite the picker will list pi even though
# its binary was never installed — exactly the state that used to open a window
# that exits 127.
harness_write_env "$proj" "$session" "claude pi"
echo "==> added pi to INSTALL_AGENTS (selected, but not installed)"
sleep 3   # let a status-poll tick reload the config

# ---- Phase 1: launch → pick pi → decline → nothing launched ----------------
echo "==> menu: Launch an installed agent → pi (decline the install)"
mc_select "Launch an installed agent"
mc_wait_prompt "Launch which agent" "agent picker"
# The picker is arrow-navigated (no type-to-filter): items are the .env order
# ["claude", "pi"], so Down moves from claude to pi.
mc_send Down
sleep 1
mc_send Enter

# The guard reports the missing binary and offers to install it, rather than
# spawning a window that exits 127.
mc_wait_prompt "isn't installed" "missing-binary guard"
echo "    ✓ guard reported pi isn't installed"
mc_wait_prompt "Install Pi agent" "offer-to-install confirm"
echo "    ✓ offered to install pi"

# Decline the install: nothing should launch.
mc_send "n"
mc_wait_prompt "nothing launched" "declined — nothing launched"
echo "    ✓ declined → nothing launched"

# No agent-pi window was spawned on the decline path (the old bug opened one that
# immediately died).
if tmux list-windows -t "$session" -F '#{window_name}' | grep -qx agent-pi; then
    echo "FATAL: an agent-pi window was spawned despite the missing binary"; exit 1
fi
echo "    ✓ no dead agent-pi window spawned on decline"

# ---- Phase 2: launch → pick pi → ACCEPT → install → it launches ------------
# This is the regression guard for the pnpm PATH fix: install-agents must place
# pi where the container's PATH actually resolves it ($PNPM_HOME, not
# $PNPM_HOME/bin), so the install succeeds AND the launch finds the binary.
echo "==> menu: Launch an installed agent → pi (accept the install)"
mc_select "Launch an installed agent"
mc_wait_prompt "Launch which agent" "agent picker (2nd time)"
mc_send Down
sleep 1
mc_send Enter
mc_wait_prompt "Install Pi agent" "offer-to-install confirm (2nd time)"
mc_send "y"
echo "    ✓ accepted the install — install-agents streaming"

# The install runs pnpm over the (harness-open) registry; wait for pi to actually
# land in the container. If the pnpm global-bin-dir PATH were still wrong this
# would time out (pnpm errored "global bin directory is not in PATH").
harness_poll "pi installed after accept" \
    podman exec --user dev "$cname" sh -c 'command -v pi'
echo "    ✓ pi installed — binary resolves in the container"

# After a successful install the guard falls through to launch: an agent-pi
# window must appear (pi is Yolo::Always, so no extra flag prompt).
harness_window_appears agent-pi \
    || { echo "FATAL: pi installed but no agent-pi window launched"; exit 1; }
echo "    ✓ agent-pi window launched after install"

echo
echo "=== AGENT-MISSING OK: a selected-but-uninstalled agent is caught before"
echo "    launch — declining launches nothing, accepting installs it (correct"
echo "    pnpm PATH) and launches it — all nested. ==="
