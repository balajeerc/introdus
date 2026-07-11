#!/usr/bin/env bash
# Milestone 3: the FULL tmux experience via the real `introdus launch`, then
# drive the live control menu with tmux send-keys / capture-pane.
#   - launch builds the introdus tmux session (main-control / notify /
#     dev-container windows) and the dev container comes up in it.
#   - we assert the session structure + the menu's live status/grouping, then
#     dispatch a real action (open a dev terminal) and prove it execs into the
#     running container as the dev user.
set -euo pipefail
source /usr/local/bin/driver-common.sh

session="harness-session"
proj="$HOME/proj-menu"
mc="$session:main-control"

harness_dummy_key
harness_write_env "$proj" "$session"
harness_ensure_base "$proj"
harness_clean
cd "$proj"

echo "==> introdus launch (builds the tmux session; attach fails without a tty,"
echo "    but the detached session persists)"
introdus launch >"$HOME/launch.log" 2>&1 || true

echo "==> waiting for the tmux session…"
for _ in $(seq 1 30); do tmux has-session -t "$session" 2>/dev/null && break; sleep 1; done
tmux has-session -t "$session" 2>/dev/null \
    || { echo "FATAL: session not created"; cat "$HOME/launch.log"; exit 1; }

echo "==> session windows:"
tmux list-windows -t "$session" -F '    #{window_name}'
for w in main-control notify dev-container; do
    tmux list-windows -t "$session" -F '#{window_name}' | grep -qx "$w" \
        || { echo "FATAL: missing window '$w'"; exit 1; }
done
echo "    ✓ main-control / notify / dev-container present"

echo "==> waiting for the container to come up + clone…"
cname=""
for _ in $(seq 1 60); do cname="$(harness_container)"; [[ -n "$cname" ]] && break; sleep 1; done
[[ -n "$cname" ]] || { echo "FATAL: no container"; exit 1; }
for _ in $(seq 1 180); do
    podman exec --user dev "$cname" test -d /home/dev/work/harness/.git 2>/dev/null && break
    sleep 1
done
echo "    container: $cname"

# ---- drive the control menu (main-control runs `introdus menu`) -------------
# Capture WITH scrollback (-S -): the inquire list redraws at the bottom and
# pushes the status block above the visible region.
cap() { tmux capture-pane -t "$mc" -p -S -; }
wait_for() { # $1=substring $2=label
    for _ in $(seq 1 60); do cap | grep -qF "$1" && return 0; sleep 0.5; done
    echo "FATAL: timed out waiting for [$2]:"; cap | tail -30 | sed 's/^/      /'; return 1
}

# Menu list is up (headers + items render).
wait_for "Quit this menu" "menu list"
wait_for "Container lifecycle" "grouped sections"
echo "    ✓ grouped menu rendered"

# Status renders once at startup (container was still coming up), so Refresh to
# re-read it, then assert it shows running + the container name.
echo "==> menu: Refresh status"
tmux send-keys -t "$mc" "Refresh" Enter
sleep 1
tmux send-keys -t "$mc" Enter   # step past the "press Enter to continue" pause
wait_for "running" "running status"
cap | grep -qF "$cname" || { echo "FATAL: container name not on status line"; exit 1; }
echo "    ✓ live status shows the container running"

echo "==> menu: Open a dev terminal (type-to-filter + enter)"
tmux send-keys -t "$mc" "dev terminal" Enter
ok=false
for _ in $(seq 1 30); do
    tmux list-windows -t "$session" -F '#{window_name}' | grep -qx dev-bash && { ok=true; break; }
    sleep 1
done
$ok || { echo "FATAL: dev-bash window not spawned"; tmux list-windows -t "$session"; exit 1; }
echo "    ✓ dev-bash window spawned"

# Prove the new window is an interactive shell as dev, inside the container.
tmux send-keys -t "$session:dev-bash" "id" Enter
idok=false
for _ in $(seq 1 20); do
    tmux capture-pane -t "$session:dev-bash" -p | grep -q "uid=1000(dev)" && { idok=true; break; }
    sleep 0.5
done
$idok || { echo "FATAL: dev terminal is not uid=1000(dev)"; tmux capture-pane -t "$session:dev-bash" -p; exit 1; }
echo "    ✓ dev terminal is uid=1000(dev) inside the container"

echo
echo "=== MILESTONE 3 OK: real 'introdus launch' built the full tmux session;"
echo "    the control menu renders live status + grouped sections and dispatches"
echo "    a real action (dev terminal exec'd into the running container). ==="
