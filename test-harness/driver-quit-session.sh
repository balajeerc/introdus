#!/usr/bin/env bash
# "Quit this tmux session (leave the container running)" tears down the whole
# tmux session (closing every window, the control panel included) but LEAVES the
# container running — unlike "Quit introdus", which also stops it. Drives it end
# to end: pick the option → assert the session is gone but the container is still
# running (so a later `introdus` reattaches to it).
# Covers TEST_PLAN: TA142
set -euo pipefail
source /usr/local/bin/driver-common.sh

session="harness-session"
proj="$HOME/proj-quit-session"

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

# ---- Quit this tmux session (leave the container running) ------------------
# No confirmation: this is non-destructive to the container. Selecting it breaks
# the menu loop and — after the TUI is torn down — kills the session.
echo "==> menu: Quit this tmux session (leave the container running)"
mc_select "Quit this tmux"

harness_poll "session gone" \
    bash -c "! tmux has-session -t '$session' 2>/dev/null"
echo "    ✓ tmux session torn down (all windows closed)"

# The whole point: the container survives the session teardown.
running="$(podman inspect -f '{{.State.Running}}' "$cname" 2>/dev/null || true)"
[[ "$running" == "true" ]] \
    || { echo "FATAL: container is not running after quit (State.Running=$running)"; exit 1; }
echo "    ✓ container still running ($cname)"

echo
echo "=== QUIT-SESSION OK: quitting the tmux session closed every window but left"
echo "    the container running (reattachable on the next launch) — all nested. ==="
