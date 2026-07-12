#!/usr/bin/env bash
# Launching an agent that is SELECTED in .env but whose binary never actually
# installed (install-agents is deliberately never-fatal — a blocked-egress or
# pnpm failure leaves the agent absent) must NOT flash a dead tmux window that
# exits 127. Instead the menu detects the missing binary, says so, and offers to
# install it. This drives the decline path: the guard fires and nothing is
# launched (no dead window).
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

# ---- Launch → pick pi → the guard must fire --------------------------------
echo "==> menu: Launch an installed agent → pi"
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

# The definitive assertion: NO agent-pi window was ever spawned (the old bug
# opened one that immediately died).
if tmux list-windows -t "$session" -F '#{window_name}' | grep -qx agent-pi; then
    echo "FATAL: an agent-pi window was spawned despite the missing binary"; exit 1
fi
echo "    ✓ no dead agent-pi window spawned"

echo
echo "=== AGENT-MISSING OK: a selected-but-uninstalled agent is caught before"
echo "    launch — the menu offers to install it instead of flashing a dead"
echo "    window — all nested. ==="
