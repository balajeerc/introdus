#!/usr/bin/env bash
# Paseo — the optional agent orchestrator — end to end through the live TUI.
#
# Opt-in is seeded in .env (INSTALL_PASEO=true), the way the wizard records it, so
# the first container is built the way a real opted-in launch is:
#   1. setup's install-agents installs @getpaseo/cli (INSTALL_PASEO passed into
#      the container), and launch resolves relay.paseo.sh into PASEO_RELAY_IPS.
#   2. The nft filter allows those relay IPs directly on :443 — because paseo's
#      daemon reaches the relay over a WebSocket that ignores HTTP_PROXY, so the
#      hostname proxy alone can't carry it. Without this the relay handshake times
#      out and phone pairing fails; with it the daemon connects.
#   3. "Launch an installed agent" offers a "via paseo" mode for claude (spawns a
#      paseo-backed window); "Show paseo pairing QR code" spawns the QR window;
#      both run paseo INSIDE the container (a single quoted podman exec).
#
# Not asserted: a physically paired phone (needs a device). Daemon<->relay
# connectivity IS asserted — that is the whole egress fix.
# Covers TEST_PLAN: TA128, TA130
set -euo pipefail
source /usr/local/bin/driver-common.sh

session="harness-session"
proj="$HOME/proj-paseo"

harness_dummy_key
harness_write_env "$proj" "$session" "claude"
echo "INSTALL_PASEO=true" >> "$proj/.env"   # opt in the way the wizard would
harness_ensure_base "$proj"
harness_clean
cd "$proj"

harness_launch "$session" "$proj"
cname="$HARNESS_CNAME"

# A live, healthy container: claude present, and paseo auto-installed by setup
# because INSTALL_PASEO=true was passed into the container.
harness_poll "claude installed in the container" \
    podman exec --user dev "$cname" bash -lc 'command -v claude'
echo "    ✓ claude present in the container"
harness_poll "paseo auto-installed by setup (INSTALL_PASEO=true)" \
    podman exec --user dev "$cname" sh -c 'command -v paseo'
echo "    ✓ paseo auto-installed on launch (opt-in wired into the container)"

# ---- 1. Assert the relay bypass is wired + the daemon reaches the relay -----
# relay.paseo.sh is resolved at launch and its IPs are allowed directly on :443 by
# nft, because paseo's ws client bypasses the proxy. Prove the env + nft rule
# exist, then that the daemon actually connects (no more "handshake timed out").
relay_ips="$(podman exec --user dev "$cname" printenv PASEO_RELAY_IPS 2>/dev/null || true)"
[[ -n "${relay_ips// /}" ]] \
    || { echo "FATAL: PASEO_RELAY_IPS empty in the container — relay.paseo.sh was not resolved into the bypass"; exit 1; }
echo "    ✓ PASEO_RELAY_IPS present in the container env: $relay_ips"

nft_rules="$(podman exec --user root "$cname" nft list table inet egress 2>/dev/null || true)"
for ip in $relay_ips; do
    echo "$nft_rules" | grep -F "$ip" | grep -q 'dport 443 accept' \
        || { echo "FATAL: relay IP $ip has no nft :443 accept rule:"; echo "$nft_rules" | sed 's/^/      /'; exit 1; }
done
echo "    ✓ nft allows each relay IP directly on :443 (the proxy-bypassing ws can dial out)"

# ---- 2. Launch claude via paseo (also starts the daemon) -------------------
# install_paseo is true, so the launch offers a "via paseo" mode for claude. The
# spawned window runs `paseo daemon start` (with a tty) then `paseo run`, so this
# is also how the daemon comes up.
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
# ...and it runs paseo INSIDE the container (one quoted `podman exec`), not on the
# host. The `exec paseo run --provider` tail only reaches podman's argv when the
# script is passed as a single quoted arg — proving the spawn command wasn't the
# unquoted debug string that would leak it to the host shell.
harness_assert_in_container_cmd "$cname" 'exec paseo run --provider' "paseo-claude in-container"
echo "    ✓ paseo-claude runs in the container (script not leaked to the host)"

# The daemon the window started must now reach the relay — the exact thing that
# timed out before the nft bypass. Poll its log for the relay control channel.
echo "==> waiting for the daemon to connect to relay.paseo.sh…"
connected=""
for _ in $(seq 1 90); do
    if podman exec --user dev "$cname" bash -lc 'grep -q relay_control_connected ~/.paseo/daemon.log 2>/dev/null'; then
        connected=1; break
    fi
    sleep 1
done
[[ -n "$connected" ]] || {
    echo "FATAL: daemon never reported relay_control_connected — pairing would time out."
    echo "       daemon status + last log lines:"
    podman exec --user dev "$cname" bash -lc 'paseo daemon status 2>&1 | head -3; echo ---; ls -la ~/.paseo 2>&1 | head; echo ---; tail -5 ~/.paseo/daemon.log 2>/dev/null | cut -c1-160' || true
    exit 1
}
echo "    ✓ daemon connected to the relay (relay_control_connected) — phone pairing works"

# ---- 3. Show the pairing QR -------------------------------------------------
echo "==> menu: Show paseo pairing QR code"
mc_select "Show paseo pairing QR"
# Short substring — the full line wraps in the narrow output pane.
mc_wait_prompt "opening the pairing QR" "QR window log"
harness_window_appears paseo-qr \
    || { echo "FATAL: no paseo-qr window spawned"; exit 1; }
echo "    ✓ paseo-qr window spawned"
# The QR window must render paseo's pairing code IN the container. The `; exec
# bash` tail only survives in podman's argv when the script is a single quoted
# arg; with the old unquoted-label bug it split on the host and ran `paseo` there
# ("paseo: command not found" on the host, a blank/host shell instead of the QR).
harness_assert_in_container_cmd "$cname" 'paseo daemon pair; exec bash' "paseo-qr in-container"
echo "    ✓ paseo-qr runs in the container (QR renders in-container, not on the host)"

echo
echo "=== PASEO OK: opted in via .env, paseo auto-installed on launch, relay bypass"
echo "    wired (PASEO_RELAY_IPS -> nft :443), daemon CONNECTED to the relay, claude"
echo "    launched via paseo, and the pairing-QR window opened — all nested. ==="
