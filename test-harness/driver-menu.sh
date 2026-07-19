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
for w in main-control dev-container; do
    tmux list-windows -t "$session" -F '#{window_name}' | grep -qx "$w" \
        || { echo "FATAL: missing window '$w'"; exit 1; }
done
echo "    ✓ main-control / dev-container present"

# ---- the notify service runs DETACHED (no window) + its log is viewable -----
# notify-host used to occupy a 'notify' tmux window; it now runs as a detached
# background process (logging to a per-session file), so the window must be gone
# but the process must be up.
echo "==> notify-host runs detached (no 'notify' tmux window)"
tmux list-windows -t "$session" -F '#{window_name}' | grep -qx notify \
    && { echo "FATAL: a 'notify' window exists — it should run detached"; exit 1; }
harness_poll "notify-host running detached" pgrep -f 'notify-host'
echo "    ✓ notify-host running detached, no window in the session"

# The workload (and rc-notify) runs as non-root `dev`; rootless podman maps the
# FIFO's host owner to container-root, so a 0600 FIFO would be unwritable by dev
# and every notification would silently no-op. Assert dev CAN write /run/notify
# (notify-host is reading, so the write returns instead of blocking).
echo "==> dev (non-root) can write the notify FIFO"
harness_poll "dev writes /run/notify" \
    podman exec -u dev "$cname" timeout 5 sh -lc 'printf "done\tharness\n" > /run/notify'
echo "    ✓ rc-notify's dev writer can deliver to the host FIFO"

# ---- menu renders, grouped, with live status ------------------------------
mc_ready
mc_vis | grep -qF "Container lifecycle" || { echo "FATAL: grouped sections missing"; exit 1; }
echo "    ✓ grouped menu rendered"

echo "==> menu: Refresh status (via the 'f' hotkey — direct-access path)"
# Status lives in the panel's left header (always on screen); Refresh just
# re-snapshots and redraws. Exercise the single-key hotkey path here (no filter,
# no Enter): pressing 'f' runs Refresh directly. Assert against the visible pane.
mc_hotkey "f"
mc_ready
mc_wait_prompt "running" "running status"
mc_vis | grep -qF "$cname" \
    || { echo "FATAL: container name not on the status panel"; mc_vis | sed 's/^/      /'; exit 1; }
echo "    ✓ live status panel shows the container running"

# ---- view the detached notify service's log from the menu ------------------
# notify-host prints "rc-notify: reading FIFO …" to its log at startup; the menu
# option surfaces that log in the output pane on demand.
echo "==> menu: Show the notification log"
mc_select "Show the notification log"
mc_wait_prompt "reading FIFO" "notify log contents"
echo "    ✓ notification log shown on demand in the output pane"

# ---- restart the detached notify service (apply config without a bounce) ----
# "Restart the notification service" SIGTERMs the running notify-host (found via
# the PID file it wrote) and respawns it, so a changed RC_FORWARD_ADDR/ntfy takes
# effect without recreating the container. Assert a notify-host is still up
# afterwards but with a NEW pid.
echo "==> menu: Restart the notification service"
OLD_NOTIFY_PID="$(pgrep -f 'notify-host' | head -1)"
notify_pid_changed() {
    local new
    new="$(pgrep -f 'notify-host' | head -1)"
    [[ -n "$new" && "$new" != "$OLD_NOTIFY_PID" ]]
}
mc_select "Restart the notification service"
harness_poll "notify-host respawned with a new pid" notify_pid_changed
echo "    ✓ notify-host restarted (was pid $OLD_NOTIFY_PID, now $(pgrep -f 'notify-host' | head -1))"

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
harness_poll "example.org persisted" grep -q "example.org" "$(harness_config_file "$proj")"
echo "    ✓ example.org persisted to WHITELIST_HOSTS in .introdus/config.env"

# ---- lifecycle: stop out-of-band, then Restart, asserting status transitions
running() { podman container inspect -f '{{.State.Running}}' "$cname" 2>/dev/null | grep -qx "$1"; }

# There's no menu 'Stop' any more — Quit stops+closes, Detach keeps it running —
# so stop the container directly and assert the *polling* status panel reflects
# it. (The slow-op spinner label is covered by Destroy/Reset teardown in
# driver-lifecycle.sh.)
echo "==> stop the container out-of-band; the status panel should show stopped"
podman stop "$cname" >/dev/null
harness_poll "container stopped" running false
mc_wait_prompt "stopped" "stopped status"
echo "    ✓ status panel shows stopped once the container goes down"

# ---- 'starting container…' status while a launch marker is present ---------
# `introdus launch` writes a per-container marker at bring-up and execs into
# podman, so the polling menu shows "starting container…" until the container is
# running. Reproduce that state deterministically on the stopped container by
# dropping a fresh marker, then assert the status flips (and reverts when gone).
echo "==> a launch marker makes the status show 'starting container…'"
marker="${XDG_STATE_HOME:-$HOME/.local/state}/introdus/launching-$cname"
touch "$marker"
mc_wait_prompt "starting container" "starting status"
echo "    ✓ status shows 'starting container…' while the launch marker is present"
rm -f "$marker"
mc_wait_prompt "stopped" "status reverts to stopped once the marker clears"
echo "    ✓ status reverts to stopped once the marker is cleared"

echo "==> menu: Restart the container"
mc_select "Restart the container"
# (Restart here is fast — the container is already stopped, so it's just a start,
# not a stop+start — so we don't race for its in-progress label; Destroy/Reset's
# teardown covers the spinner-label behaviour on reliably-slow ops.)
harness_poll "container running again" running true
mc_wait_prompt "running" "running status"
echo "    ✓ Restart worked; the status panel shows running again"

echo
echo "=== MILESTONE 3+ OK: full launch + a battery of live control-menu actions:"
echo "    grouped render, live status, dev & root terminals, copy-file, add-"
echo "    allowlist (persisted), and stop/Restart status transitions — nested. ==="
