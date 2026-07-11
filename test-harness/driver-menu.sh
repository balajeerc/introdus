#!/usr/bin/env bash
# Milestone 3+: the FULL tmux experience via the real `introdus launch`, driving
# a sequence of control-menu actions against ONE running container with tmux
# send-keys / capture-pane. Ordered non-destructive first; lifecycle teardown
# (recreate/reset/destroy) lives in driver-lifecycle.sh.
# Covers TEST_PLAN: TA48, TA67, TA85, TA89, TA90, TA91, TA94, TA109, TA111
set -euo pipefail
source /usr/local/bin/driver-common.sh

session="harness-session"
proj="$HOME/proj-menu"

harness_dummy_key
harness_write_env "$proj" "$session"
harness_ensure_base "$proj"
harness_clean
cd "$proj"

harness_launch "$session" "$proj"
cname="$HARNESS_CNAME"

echo "==> session windows:"
tmux list-windows -t "$session" -F '    #{window_name}'
for w in main-control notify dev-container; do
    tmux list-windows -t "$session" -F '#{window_name}' | grep -qx "$w" \
        || { echo "FATAL: missing window '$w'"; exit 1; }
done
echo "    ✓ main-control / notify / dev-container present"

# ---- menu renders, grouped, with live status ------------------------------
mc_ready
mc_vis | grep -qF "Container lifecycle" || { echo "FATAL: grouped sections missing"; exit 1; }
echo "    ✓ grouped menu rendered"

echo "==> menu: Refresh status"
# Status lives in the panel's left header (always on screen); Refresh just
# re-snapshots and redraws. Assert against the visible pane.
mc_select "Refresh"
mc_ready
mc_wait_prompt "running" "running status"
mc_vis | grep -qF "$cname" \
    || { echo "FATAL: container name not on the status panel"; mc_vis | sed 's/^/      /'; exit 1; }
echo "    ✓ live status panel shows the container running"

# ---- open a dev terminal (dispatch into the container as dev) --------------
echo "==> menu: Open a dev terminal"
mc_select "dev terminal"
harness_window_appears dev-bash || { echo "FATAL: dev-bash window not spawned"; exit 1; }
tmux send-keys -t "$session:dev-bash" "id" Enter
harness_poll "dev-bash is dev" bash -c \
    "tmux capture-pane -t '$session:dev-bash' -p | grep -q 'uid=1000(dev)'"
echo "    ✓ dev-bash spawned; shell is uid=1000(dev) in the container"

# ---- open a root terminal (uid 0) ------------------------------------------
echo "==> menu: Open a root terminal"
mc_select "root terminal"
harness_window_appears root-bash || { echo "FATAL: root-bash window not spawned"; exit 1; }
tmux send-keys -t "$session:root-bash" "id" Enter
harness_poll "root-bash is root" bash -c \
    "tmux capture-pane -t '$session:root-bash' -p | grep -q 'uid=0(root)'"
echo "    ✓ root-bash spawned; shell is uid=0(root) in the container"

# ---- copy a host file into the container -----------------------------------
echo "==> menu: Copy a host file into the container"
payload="$HOME/payload-$$.txt"
echo "introdus-harness-payload" > "$payload"
base="$(basename "$payload")"
mc_select "Copy a host"
mc_wait_prompt "Host path to copy" "copy prompt"
mc_send "$payload" Enter
harness_poll "file present in uploads" bash -c \
    "podman exec --user dev '$cname' cat '/home/dev/uploads/$base' 2>/dev/null | grep -q introdus-harness-payload"
echo "    ✓ file copied into /home/dev/uploads"

# ---- add a host to the egress allowlist (persist) --------------------------
echo "==> menu: Add a hostname to the egress allowlist"
mc_select "Add hostnames"
mc_wait_prompt "Hostnames to allow" "allowlist prompt"
mc_send "example.org" Enter
# It offers to restart to apply; decline (default No -> Enter).
mc_wait_prompt "Restart the container to apply" "restart offer"
mc_send Enter
harness_poll "example.org persisted" grep -q "example.org" "$proj/.env"
echo "    ✓ example.org persisted to WHITELIST_HOSTS in .env"

# ---- lifecycle: Stop then Restart, asserting status transitions ------------
running() { podman container inspect -f '{{.State.Running}}' "$cname" 2>/dev/null | grep -qx "$1"; }

echo "==> menu: Stop the container"
mc_select "Stop the container"
harness_poll "container stopped" running false
mc_wait_prompt "stopped" "stopped status"
echo "    ✓ Stop worked; the status panel shows stopped"

echo "==> menu: Restart the container"
mc_select "Restart the container"
harness_poll "container running again" running true
mc_wait_prompt "running" "running status"
echo "    ✓ Restart worked; the status panel shows running again"

echo
echo "=== MILESTONE 3+ OK: full launch + a battery of live control-menu actions:"
echo "    grouped render, live status, dev & root terminals, copy-file, add-"
echo "    allowlist (persisted), and Stop/Restart lifecycle — all nested. ==="
