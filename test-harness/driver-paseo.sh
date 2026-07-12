#!/usr/bin/env bash
# Paseo — the optional agent orchestrator — end to end through the live TUI:
#   1. "Install paseo" installs @getpaseo/cli into the container, records the
#      opt-in (INSTALL_PASEO=true) and allowlists the relay host (paseo.sh).
#   2. "Launch an installed agent" then offers a "via paseo" mode for a native
#      provider (claude), which spawns a paseo-backed window.
#   3. "Show paseo pairing QR code" spawns a window that renders the pairing QR.
#
# What is NOT asserted here (untestable without a real relay + a paired phone):
# actual daemon<->relay connectivity and phone pairing. We assert the harness
# wiring — install, config/allowlist, the launch offer, and the spawned windows.
# Covers TEST_PLAN: TA128
set -euo pipefail
source /usr/local/bin/driver-common.sh

session="harness-session"
proj="$HOME/proj-paseo"

harness_dummy_key
harness_write_env "$proj" "$session" "claude"   # paseo is opted in via the menu
harness_ensure_base "$proj"
harness_clean
cd "$proj"

harness_launch "$session" "$proj"
cname="$HARNESS_CNAME"

# A live, healthy container: claude present, paseo not yet installed.
harness_poll "claude installed in the container" \
    podman exec --user dev "$cname" bash -lc 'command -v claude'
echo "    ✓ claude present in the container"
if podman exec --user dev "$cname" sh -c 'command -v paseo' >/dev/null 2>&1; then
    echo "FATAL: paseo unexpectedly present before install"; exit 1
fi
echo "    ✓ paseo is absent (as expected)"

# ---- 1. Install paseo from the menu ----------------------------------------
echo "==> menu: Install paseo (drive agents from your phone)"
mc_select "Install paseo"
mc_wait_prompt "It runs your agents" "paseo install confirm"
mc_send "y"
# save_and_regen_allowlist offers a restart to apply the new allowlist — decline
# it (the install itself only needs the already-allowed npm registry).
mc_wait_prompt "Restart the container to apply" "allowlist restart offer"
mc_send "n"
echo "    ✓ accepted install, declined restart — install-agents (paseo) streaming"

# The install runs pnpm over the (harness-open) registry; wait for the binary.
harness_poll "paseo installed after accept" \
    podman exec --user dev "$cname" sh -c 'command -v paseo'
echo "    ✓ paseo installed — binary resolves in the container"
mc_wait_prompt "paseo installed" "paseo install success log"

# The opt-in + relay host were persisted to .env.
grep -Eq 'INSTALL_PASEO=.*true' "$proj/.env" \
    || { echo "FATAL: INSTALL_PASEO not set true in .env"; grep INSTALL_PASEO "$proj/.env" || true; exit 1; }
grep -qxF 'paseo.sh' "$proj/.env" \
    || { echo "FATAL: paseo.sh not added to WHITELIST_HOSTS in .env"; exit 1; }
echo "    ✓ .env records INSTALL_PASEO=true and allowlists paseo.sh"

# ---- 2. Launch claude via paseo --------------------------------------------
# The menu reloads .env every STATUS_POLL tick, so install_paseo is now true and
# the launch offers a "via paseo" mode for the native provider claude.
sleep 3
echo "==> menu: Launch an installed agent → claude → via paseo"
mc_select "Launch an installed agent"
mc_wait_prompt "Launch which agent" "agent picker"
mc_send Enter                     # only claude is installed; it's the sole row
mc_wait_prompt "via paseo" "via-paseo launch offer"
echo "    ✓ offered 'launch via paseo' for claude"
mc_send "y"
mc_wait_prompt "Initial task" "initial-task prompt"
mc_send Enter                     # blank task -> interactive session
# The deterministic proof the via-paseo branch ran (logged just before spawn).
# Match a short substring — the full line wraps in the narrow output pane.
mc_wait_prompt "launching Claude" "via-paseo launch log"
echo "    ✓ launched claude via paseo (daemon-supervised)"
harness_window_appears paseo-claude \
    || { echo "FATAL: no paseo-claude window spawned"; exit 1; }
echo "    ✓ paseo-claude window spawned"

# ---- 3. Show the pairing QR -------------------------------------------------
echo "==> menu: Show paseo pairing QR code"
mc_select "Show paseo pairing QR"
# Short substring — the full line wraps in the narrow output pane.
mc_wait_prompt "opening the pairing QR" "QR window log"
harness_window_appears paseo-qr \
    || { echo "FATAL: no paseo-qr window spawned"; exit 1; }
echo "    ✓ paseo-qr window spawned"

echo
echo "=== PASEO OK: installed from the menu (INSTALL_PASEO + paseo.sh persisted),"
echo "    claude launched via paseo, and the pairing-QR window opened — all"
echo "    nested. ==="
