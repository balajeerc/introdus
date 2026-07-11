#!/usr/bin/env bash
# Milestone 2: bring the FULL dev container up nested — clone a small public repo
# through the (still-enforced) egress proxy and reach the "up and running" state.
#
# Clone mocking: the production clone is SSH deploy-key -> ProxyCommand, which
# needs a key registered on a real host (not hermetic). Instead we point REPO_URL
# at a tiny PUBLIC repo over HTTPS and, TEST-ONLY, overlay `https_proxy` onto the
# base image so git routes through the in-container proxy. The proxy still
# enforces the hostname allowlist — this only selects the transport, so we're
# still exercising real egress enforcement + a real clone + container bring-up.
set -euo pipefail
source /usr/local/bin/driver-common.sh

REPO_URL="${HARNESS_REPO_URL:-https://github.com/octocat/Hello-World.git}"
proj="$HOME/proj-launch"
mkdir -p "$proj"

mkdir -p "$HOME/.ssh"
[[ -f "$HOME/.ssh/harness-key" ]] || ssh-keygen -t ed25519 -N "" -C harness \
    -f "$HOME/.ssh/harness-key" >/dev/null

cat > "$proj/.env" <<EOF
PROJECT_NAME=harness
REPO_URL=$REPO_URL
DEPLOY_KEY_PATH=$HOME/.ssh/harness-key
WEBAPP_PORT=3000
INSTALL_AGENTS="claude"
SESSION_NAME=harness-session
EOF

cd "$proj"

# Build the base image ONCE (cached across runs via the storage volume). Use
# `verify` rather than `rebuild-base` — the latter forces --no-cache, and verify
# doubles as an egress smoke test on a clean base.
if podman image exists introdus-base:latest; then
    echo "==> base image already present (cached)"
else
    echo "==> building base image via 'introdus verify' (first run, cached after)"
    introdus verify
fi

echo "==> TEST-ONLY: overlay https_proxy onto introdus-base so the public HTTPS"
echo "    clone routes through the in-container allowlist proxy"
build_dir="$(mktemp -d)"
cat > "$build_dir/Containerfile" <<'EOF'
FROM introdus-base:latest
# The in-container egress proxy still enforces the hostname allowlist; this only
# picks HTTP-proxy transport so a keyless public clone works in the harness.
ENV https_proxy=http://127.0.0.1:8888 http_proxy=http://127.0.0.1:8888 \
    HTTPS_PROXY=http://127.0.0.1:8888 HTTP_PROXY=http://127.0.0.1:8888
EOF
podman build -t introdus-base:latest -f "$build_dir/Containerfile" "$build_dir"

echo "==> clearing any leftover container/volume from a prior harness run"
for c in $(podman ps -aq --filter "name=introdus-harness-" 2>/dev/null); do
    podman rm -f "$c" >/dev/null 2>&1 || true
done
podman volume rm -f introdus-vol-harness >/dev/null 2>&1 || true

echo "==> starting the dev container via 'introdus up' in a detached tmux window"
: > "$HOME/up.log"
tmux new-session -d -s dev "cd '$proj' && introdus up >'$HOME/up.log' 2>&1"

cname=""
echo "==> waiting for the container to be created…"
for _ in $(seq 1 60); do
    cname="$(podman ps --format '{{.Names}}' | grep '^introdus-harness-' || true)"
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
