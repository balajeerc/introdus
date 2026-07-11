#!/usr/bin/env bash
# Destructive lifecycle via the control menu: Recreate (keeps the /home/dev
# volume — persistence) and Destroy (double confirm + dirty-git scan + deploy-key
# deletion + full teardown). Kept separate from driver-menu.sh because it tears
# the container/volume down.
set -euo pipefail
source /usr/local/bin/driver-common.sh

session="harness-session"
proj="$HOME/proj-lifecycle"

harness_dummy_key
harness_write_env "$proj" "$session"
harness_ensure_base "$proj"
harness_clean
cd "$proj"

harness_launch "$session" "$proj"
cname="$HARNESS_CNAME"

# ---- Recreate keeps the /home/dev volume (persistence) ---------------------
echo "==> dropping a marker into the persistent /home/dev volume"
podman exec --user dev "$cname" bash -c 'echo persist-me > /home/dev/marker.txt'

echo "==> menu: Recreate the container (keeps the volume)"
mc_select "Recreate the container"
mc_wait_prompt "Recreate the container now" "recreate confirm"
mc_send Enter   # default Yes
# Recreate removes the container and respawns the dev-container window (introdus
# up), which recreates the container against the SAME volume.
harness_poll "container back up" bash -c \
    "podman ps --format '{{.Names}}' | grep -q '^introdus-harness-'"
newc="$(harness_container)"
harness_poll "marker survived recreate" bash -c \
    "podman exec --user dev '$newc' cat /home/dev/marker.txt 2>/dev/null | grep -q persist-me"
echo "    ✓ /home/dev survived recreate (marker present on the new container)"
mc_continue

# ---- Destroy: double confirm + dirty scan + deploy-key deletion ------------
key="$HOME/.ssh/harness-key"
[[ -f "$key" ]] || { echo "FATAL: deploy key fixture missing"; exit 1; }

echo "==> menu: Destroy the container"
mc_select "Destroy the container"
# 1) plain yes/no
mc_wait_prompt "Destroy this container and permanently delete its volume" "destroy confirm"
mc_send "y" Enter
# 2) dirty-git scan runs (throwaway container), then a typed confirmation
mc_wait_prompt "Type 'yes'" "destroy typed confirm"
mc_send "yes" Enter
# 3) offer to delete the local deploy key
mc_wait_prompt "Also delete the local deploy key" "deploy-key prompt"
mc_send "y" Enter

harness_poll "container gone" bash -c "! podman container exists '$newc'"
harness_poll "volume gone" bash -c "! podman volume exists introdus-vol-harness"
harness_poll "deploy key deleted" bash -c "[ ! -f '$key' ]"
echo "    ✓ destroy wiped the container, the volume, and the local deploy key"

echo
echo "=== LIFECYCLE OK: Recreate preserved /home/dev; Destroy double-confirmed,"
echo "    ran the dirty-git scan, deleted the deploy key, and tore everything"
echo "    down — all nested. ==="
