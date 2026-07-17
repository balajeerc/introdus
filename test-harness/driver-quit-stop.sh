#!/usr/bin/env bash
# "Quit introdus (stop the container)" stops the running container AND tears down
# the whole tmux session (closing every window, the control panel included).
# Drives it end to end: pick the option → confirm → assert the container is no
# longer running and the session is gone.
# Covers TEST_PLAN: TA124
set -euo pipefail
source /usr/local/bin/driver-common.sh

session="harness-session"
proj="$HOME/proj-quit-stop"

harness_dummy_key
harness_write_env "$proj" "$session" "claude"
harness_ensure_base "$proj"
harness_clean
cd "$proj"

harness_launch "$session" "$proj"
cname="$HARNESS_CNAME"

# Sanity: the container is running and the session is up before we quit.
harness_poll "container running" \
    bash -c "[ \"\$(podman inspect -f '{{.State.Running}}' '$cname' 2>/dev/null)\" = true ]"
tmux has-session -t "$session" 2>/dev/null \
    || { echo "FATAL: session missing before quit"; exit 1; }
echo "    ✓ container running + session up"

# ---- Quit introdus (stop the container) ------------------------------------
echo "==> menu: Quit introdus (stop the container)"
mc_select "Quit introdus"
mc_wait_prompt "stop the container and close all its windows" "quit-stop confirm"
echo "    ✓ quit-stop confirm shown"
mc_send "y"

# The action stops the container (synchronous podman stop), then — after the TUI
# is torn down — kills the session, closing every window.
harness_poll "container stopped" \
    bash -c "[ \"\$(podman inspect -f '{{.State.Running}}' '$cname' 2>/dev/null)\" != true ]"
echo "    ✓ container stopped"

harness_poll "session gone" \
    bash -c "! tmux has-session -t '$session' 2>/dev/null"
echo "    ✓ tmux session torn down (all windows closed)"

echo
echo "=== QUIT-STOP OK: quitting introdus stopped the container and closed the"
echo "    whole tmux session — all nested. ==="
