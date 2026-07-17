#!/usr/bin/env bash
# Egress enforcement from the WORKLOAD's point of view (the dev user in the
# running container) — the core security guarantee — plus the menu's
# blocked-egress listing utility.
# Covers TEST_PLAN: TA41, TA88
set -euo pipefail
source /usr/local/bin/driver-common.sh

session="harness-session"
proj="$HOME/proj-egress"

harness_dummy_key
harness_write_env "$proj" "$session"
harness_ensure_base "$proj"
harness_clean
cd "$proj"

harness_launch "$session" "$proj"
cname="$HARNESS_CNAME"

dev() { podman exec --user dev "$cname" "$@"; }

echo "==> workload: a DIRECT dial to a non-allowlisted host is dropped"
if dev curl -s --max-time 8 --noproxy '*' -o /dev/null https://example.com; then
    echo "FATAL: direct egress to example.com succeeded — filter is open"; exit 1
fi
echo "    ✓ direct dial to example.com blocked (nft default-deny)"

echo "==> workload: an allowlisted host IS reachable through the proxy"
dev curl -fsS --max-time 20 -x http://127.0.0.1:8888 -o /dev/null https://github.com \
    || { echo "FATAL: proxy could not reach allowlisted github.com"; exit 1; }
echo "    ✓ github.com reachable via the proxy"

echo "==> workload: a non-allowlisted host is REFUSED by the proxy"
if dev curl -fsS --max-time 20 -x http://127.0.0.1:8888 -o /dev/null https://example.com; then
    echo "FATAL: proxy allowed non-allowlisted example.com"; exit 1
fi
echo "    ✓ example.com refused by the proxy allowlist"

echo "==> the denied host appears in egress-log"
harness_poll "example.com in egress-log" \
    bash -c "podman exec --user dev '$cname' egress-log 2>/dev/null | grep -q example.com"
echo "    ✓ egress-log lists the denied host"

echo "==> menu: List recently blocked egress URLs"
mc_select "blocked egress"
mc_wait_prompt "example.com" "blocked-egress listing"
echo "    ✓ the menu blocked-egress utility shows example.com"

echo
echo "=== EGRESS OK: workload default-deny holds (direct blocked, allowlisted via"
echo "    proxy allowed, non-allowlisted refused), and the block is logged +"
echo "    surfaced in the menu — all nested. ==="
