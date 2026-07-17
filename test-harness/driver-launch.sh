#!/usr/bin/env bash
# Milestone 2: bring the FULL dev container up nested — clone a small public repo
# through the (still-enforced) egress proxy and reach the "up and running" state.
set -euo pipefail
source /usr/local/bin/driver-common.sh

proj="$HOME/proj-launch"
harness_dummy_key
harness_write_env "$proj"
harness_ensure_base "$proj"
harness_clean
cd "$proj"

echo "==> starting the dev container via 'introdus up' in a detached tmux window"
: > "$HOME/up.log"
tmux new-session -d -s dev "cd '$proj' && introdus up >'$HOME/up.log' 2>&1"

echo "==> waiting for the container to be created…"
cname=""
for _ in $(seq 1 60); do
    cname="$(harness_container)"
    [[ -n "$cname" ]] && break
    sleep 1
done
[[ -n "$cname" ]] || { echo "FATAL: container never appeared"; tail -40 "$HOME/up.log"; exit 1; }
echo "    container: $cname"

echo "==> waiting for the repo clone + 'up and running' banner (up to 180s)…"
ok=false
for _ in $(seq 1 180); do
    if grep -q "up and running" "$HOME/up.log" 2>/dev/null; then ok=true; break; fi
    if ! podman container exists "$cname"; then break; fi
    sleep 1
done

echo
echo "==================== up.log (tail) ===================="
tail -30 "$HOME/up.log"
echo "======================================================="

echo
echo "==> asserting the repo was cloned inside the container"
if podman exec --user dev "$cname" test -d "/home/dev/work/harness/.git"; then
    echo "    ✓ /home/dev/work/harness/.git present"
else
    echo "FATAL: repo not cloned"; exit 1
fi

$ok || { echo "FATAL: never reached 'up and running'"; exit 1; }
echo
echo "=== MILESTONE 2 OK: public repo cloned through the egress proxy and the"
echo "    dev container reached 'up and running' nested. ==="
