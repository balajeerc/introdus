#!/usr/bin/env bash
# Destructive lifecycle via the control menu: Recreate (keeps the /home/dev
# volume — persistence) and Destroy (double confirm + dirty-git scan + deploy-key
# deletion + full teardown). Kept separate from driver-menu.sh because it tears
# the container/volume down.
# Covers TEST_PLAN: TA54, TA55, TA56, TA58, TA59, TA62, TA63, TA93, TA95, TA112
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
oldid="$(harness_container_id "$cname")"
mc_select "Recreate the container"
mc_wait_prompt "Recreate the container now" "recreate confirm"
mc_send Enter   # default Yes
# Recreate removes the container and respawns the dev-container window (introdus
# up), which recreates the container (SAME name, NEW id) against the SAME volume.
# Wait for the NEW id specifically — grabbing the still-present old one would
# then race its teardown.
harness_poll "container recreated (new id, running)" bash -c "
    id=\$(podman container inspect -f '{{.Id}}' '$cname' 2>/dev/null) || exit 1
    [ -n \"\$id\" ] && [ \"\$id\" != '$oldid' ] \
      && podman container inspect -f '{{.State.Running}}' '$cname' | grep -qx true
"
harness_poll "marker survived recreate" bash -c \
    "podman exec --user dev '$cname' cat /home/dev/marker.txt 2>/dev/null | grep -q persist-me"
echo "    ✓ /home/dev survived recreate (marker present on the recreated container)"

# ---- Destroy: double confirm + dirty scan + deploy-key deletion ------------
key="$HOME/.ssh/harness-key"
[[ -f "$key" ]] || { echo "FATAL: deploy key fixture missing"; exit 1; }

# Dirty the cloned repo so the destroy safety scan (the data-loss guard) has
# real uncommitted + unpushed state to report.
echo "==> dirtying the repo to exercise the destroy safety scan"
podman exec --user dev "$cname" bash -c '
    set -e
    cd /home/dev/work/harness
    git config user.email harness@test.local
    git config user.name harness
    echo committed-locally > harness-local.txt
    git add harness-local.txt
    git commit -qm "unpushed local commit"           # -> unpushed commit
    echo modified >> "$(git ls-files | head -1)"      # -> unstaged working-tree change
    echo scratch > harness-untracked.txt              # -> untracked file
'

echo "==> menu: Destroy the container"
mc_select "Destroy the container"
# 1) plain yes/no
mc_wait_prompt "Destroy this container and permanently delete its volume" "destroy confirm"
mc_send "y"   # a single y submits the confirm (Enter here would leak to the next prompt)
# 2) dirty-git scan runs (throwaway container), then a typed confirmation. The
#    scan report streams into the output pane just before the typed prompt band
#    appears — assert it caught the uncommitted + unpushed state we planted.
mc_wait_prompt "Type 'yes'" "destroy typed confirm"
scan="$(mc_vis)"
echo "$scan" | grep -qF "working tree" \
    || { echo "FATAL: safety scan missed working-tree changes"; echo "$scan" | tail -25 | sed 's/^/      /'; exit 1; }
echo "$scan" | grep -qF "unpushed commits" \
    || { echo "FATAL: safety scan missed unpushed commits"; echo "$scan" | tail -25 | sed 's/^/      /'; exit 1; }
echo "    ✓ safety scan reported uncommitted working-tree + unpushed commits"
mc_send "yes" Enter
# 3) offer to delete the local deploy key (single y submits)
mc_wait_prompt "Also delete the local deploy key" "deploy-key prompt"
mc_send "y"

harness_poll "container gone" bash -c "! podman container exists '$cname'"
harness_poll "volume gone" bash -c "! podman volume exists introdus-vol-harness"
harness_poll "deploy key deleted" bash -c "[ ! -f '$key' ]"
echo "    ✓ destroy wiped the container, the volume, and the local deploy key"

# It must STAY destroyed — nothing (e.g. the dev-container window) should
# re-create it behind our back.
sleep 4
podman container exists "$cname" \
    && { echo "FATAL: container reappeared after destroy — something re-created it"; exit 1; }
echo "    ✓ container stayed destroyed (no re-create)"

echo
echo "=== LIFECYCLE OK: Recreate preserved /home/dev; Destroy double-confirmed,"
echo "    ran the dirty-git scan, deleted the deploy key, and tore everything"
echo "    down — all nested. ==="
