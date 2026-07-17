#!/usr/bin/env bash
# "Detach tmux session (Keep container running)" detaches every client from the
# session — returning each to its shell — but LEAVES the session, its windows,
# and the container all running (so a later `introdus` reattaches). Unlike "Quit
# introdus", it neither stops the container nor tears down the session, and
# unlike a plain menu-quit the control TUI keeps running.
#
# Drives it end to end: attach a real probe client → pick the option → assert the
# client got detached while the session, the menu, and the container all survive.
# Covers TEST_PLAN: TA142
set -euo pipefail
source /usr/local/bin/driver-common.sh

session="harness-session"
proj="$HOME/proj-detach"

harness_dummy_key
harness_write_env "$proj" "$session" "claude"
harness_ensure_base "$proj"
harness_clean
cd "$proj"

harness_launch "$session" "$proj"
cname="$HARNESS_CNAME"
mc_ready

# Sanity: container running + session up before we detach.
harness_poll "container running" \
    bash -c "[ \"\$(podman inspect -f '{{.State.Running}}' '$cname' 2>/dev/null)\" = true ]"
echo "    ✓ container running + control panel up"

# Attach a real client so we can observe it get detached. `env -u TMUX` lets a
# nested `tmux attach` connect instead of refusing ("sessions should be nested").
# The session is window-size manual, so a second client does not resize it.
probe="detach-probe"
tmux kill-session -t "$probe" 2>/dev/null || true
tmux new-session -d -s "$probe" "exec env -u TMUX tmux attach -t '$session'"
n_clients() { tmux list-clients -t "$session" 2>/dev/null | grep -c . || true; }
harness_poll "probe client attached" bash -c "[ \"\$($(declare -f n_clients); n_clients)\" -ge 1 ]"
echo "    ✓ a client is attached to $session"

# ---- Detach tmux session (keep the container running) ----------------------
# No confirmation: detaching is non-destructive. The menu process stays alive.
echo "==> menu: Detach tmux session (Keep container running)"
mc_select "Detach tmux"

# The client is detached — no clients remain — but the session is NOT torn down.
harness_poll "client detached" bash -c "[ \"\$($(declare -f n_clients); n_clients)\" -eq 0 ]"
echo "    ✓ client detached (returned to its shell)"

tmux has-session -t "$session" 2>/dev/null \
    || { echo "FATAL: session was torn down by detach"; exit 1; }
echo "    ✓ tmux session still alive"

# The control panel keeps running in its now-unattached window (menu still drawn).
mc_wait_prompt "Terminals & agents" "control panel still running"
echo "    ✓ control panel still running"

running="$(podman inspect -f '{{.State.Running}}' "$cname" 2>/dev/null || true)"
[[ "$running" == "true" ]] \
    || { echo "FATAL: container is not running after detach (State.Running=$running)"; exit 1; }
echo "    ✓ container still running ($cname)"

echo
echo "=== DETACH OK: detaching returned the client to its shell but left the"
echo "    session, the control panel, and the container all running — all nested. ==="
