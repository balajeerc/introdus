#!/usr/bin/env bash
# Runs inside the container as root. Base image already has apt packages,
# mise, node LTS, and claude. This script handles the per-project bits:
# ssh setup, cloning, and (when EXPOSE_WEBAPP=true) the cloudflared tunnel.
# Safe to re-run — /root is a persistent podman volume.
set -euo pipefail

: "${PROJECT_NAME:?}"
: "${REPO_URL:?}"
: "${WEBAPP_PORT:?}"
: "${CANARY_BLOCKED_IP:?}"

# HOST_OS is set by launch.sh on the host and passed in as an env var.
# It never affects container behaviour (the container is always Linux);
# it only controls which host-side instructions are printed at the end.
HOST_OS="${HOST_OS:-linux}"
EXPOSE_WEBAPP="${EXPOSE_WEBAPP:-false}"
DISABLE_NETWORK_BLOCK="${DISABLE_NETWORK_BLOCK:-false}"

WORKDIR="/root/work/${PROJECT_NAME}"

# Container name as created by launch.sh (carries the per-project suffix, so it
# is not simply remote-code-$PROJECT_NAME). Used only to print correct host-side
# exec/stop commands. Fall back to the pre-suffix name if an older launch.sh
# didn't pass CONTAINER_NAME in.
CNAME="${CONTAINER_NAME:-remote-code-$PROJECT_NAME}"

log() { printf '\n==> %s\n' "$*"; }

# Non-interactive bash (`/bin/bash /setup.sh`) does not source .bashrc,
# so we activate mise explicitly.
export PATH="/root/.local/bin:$PATH"
eval "$(/root/.local/bin/mise activate bash)"

# Egress filter self-check. Runs BEFORE any network-using step so that if
# the host's egress filter is not enforcing, we abort before the deploy key
# is used, the repo is cloned, or package managers phone home.
#
# Two probes using bash's built-in /dev/tcp (no external deps):
#   1. CANARY_BLOCKED_IP must be unreachable — proves the filter is dropping
#      non-allowlisted destinations.
#   2. GIT_HOST must be reachable — proves the allowlist is not globally
#      broken (avoids a harness that "passes" only because all egress is down).
#
# nft `drop` silently discards packets, so a working filter appears as a
# connect timeout rather than a TCP reset. 3s is enough.
DISABLE_NETWORK_BLOCK="${DISABLE_NETWORK_BLOCK:-false}"

GIT_HOST="$(echo "$REPO_URL" | sed -E 's#^(git@|ssh://git@|https://)##; s#[:/].*$##')"
# Extract an explicit SSH port from ssh:// URLs (defaults to 22). Useful when an
# ISP or firewall blocks port 22 — in that case set REPO_URL to something like
# ssh://git@ssh.github.com:443/owner/repo.git and the harness will probe and
# keyscan on the right port.
GIT_SSH_PORT="$(echo "$REPO_URL" | sed -nE 's#^ssh://git@[^:/]+:([0-9]+)/.*#\1#p')"
GIT_SSH_PORT="${GIT_SSH_PORT:-22}"

if [[ "$DISABLE_NETWORK_BLOCK" == "true" ]]; then
    log "egress self-check skipped (--disable-network-block)"
else
    log "egress self-check (block: $CANARY_BLOCKED_IP, allow: $GIT_HOST:$GIT_SSH_PORT)"
    if timeout 3 bash -c "echo > /dev/tcp/${CANARY_BLOCKED_IP}/80" 2>/dev/null; then
        cat >&2 <<EOF

FATAL: egress filter is NOT enforcing.
  ${CANARY_BLOCKED_IP} was reachable on TCP/80, but it is not in the
  allowlist. The nft table is either missing, not matching this container's
  traffic, or installed against a stale cgroup/network. Aborting before
  this container touches the network.

  On the host, check:
    Linux rootless:  sudo nft list table inet rcode_\${PROJECT_NAME//-/_}
    Linux rootful:   sudo iptables -nvL REMOTE-CODE-<project-slug>
    macOS:           podman machine ssh -- sudo nft list table inet rcode_\${PROJECT_NAME//-/_}
EOF
        exit 1
    fi
    if ! timeout 5 bash -c "echo > /dev/tcp/${GIT_HOST}/${GIT_SSH_PORT}" 2>/dev/null \
       && ! timeout 5 bash -c "echo > /dev/tcp/${GIT_HOST}/443" 2>/dev/null; then
        cat >&2 <<EOF

FATAL: allowlisted host ${GIT_HOST} is unreachable on ${GIT_SSH_PORT} or 443.
  Either the allowlist is misconfigured (is ${GIT_HOST} in WHITELIST_HOSTS
  or resolved via GIT_HOST auto-add?) or host networking is down. Aborting
  before the repo clone.
EOF
        exit 1
    fi
    echo "  ok: egress filter enforcing (${CANARY_BLOCKED_IP} dropped, ${GIT_HOST}:${GIT_SSH_PORT} reachable)"
fi

log "installing deploy key"
mkdir -p /root/.ssh
chmod 700 /root/.ssh
cp /tmp/deploy_key /root/.ssh/id_ed25519
chmod 600 /root/.ssh/id_ed25519
touch /root/.ssh/known_hosts
chmod 600 /root/.ssh/known_hosts
if ! ssh-keygen -F "$GIT_HOST" -f /root/.ssh/known_hosts >/dev/null 2>&1; then
    ssh-keyscan -p "$GIT_SSH_PORT" -H "$GIT_HOST" >> /root/.ssh/known_hosts 2>/dev/null
fi

if [[ -d "$WORKDIR/.git" ]]; then
    log "repo already present at $WORKDIR — skipping clone"
else
    log "cloning $REPO_URL"
    mkdir -p "$(dirname "$WORKDIR")"
    git clone "$REPO_URL" "$WORKDIR"
fi
cd "$WORKDIR"

# One-shot --pull sentinel dropped by launch.sh. Always consume it (rm
# first) so a failed pull doesn't keep retrying on every relaunch.
if [[ -f /root/.pull-on-next-start ]]; then
    rm -f /root/.pull-on-next-start
    log "fast-forwarding repo (--pull)"
    if git fetch --prune; then
        if ! git pull --ff-only; then
            echo "  warning: not a fast-forward — repo left as-is. resolve manually inside the container."
        fi
    else
        echo "  warning: git fetch failed — repo left as-is"
    fi
fi

mkdir -p /root/.logs

if [[ "$EXPOSE_WEBAPP" == "true" ]]; then
    log "starting cloudflared quick tunnel in tmux session 'tunnel' (-> port $WEBAPP_PORT)"
    : > /root/.logs/tunnel.log
    rm -f /root/.logs/tunnel-url.txt
    # Pin edge IPs and force HTTP/2 so cloudflared skips its SRV-based edge
    # discovery (DNS for SRV records is unreliable through pasta) and avoids
    # QUIC/UDP. Edge IPs come from TUNNEL_EDGE_IPS, set by launch.sh and
    # already added to the egress allowlist there.
    EDGE_ARGS=""
    for ip in $TUNNEL_EDGE_IPS; do
        EDGE_ARGS="$EDGE_ARGS --edge ${ip}:7844"
    done
    tmux new-session -d -s tunnel "cloudflared tunnel --protocol http2 $EDGE_ARGS --url http://localhost:$WEBAPP_PORT 2>&1; echo '[cloudflared exited]'; exec bash"
    tmux pipe-pane -t tunnel -o 'cat >>/root/.logs/tunnel.log'
    echo "  attach: podman exec -it $CNAME tmux attach -t tunnel"
    echo "  tail:   podman exec -it $CNAME tail -f /root/.logs/tunnel.log"
fi

# VSCode connection instructions are OS- and locality-aware. macOS launches
# always need the podman-machine socket path (containers live inside the VM);
# Linux local launches need the user systemd socket; remote launches go via
# Remote-SSH and skip socket setup entirely.
if [[ "$HOST_OS" == "macos" ]]; then
    VSCODE_SETUP_INSTRUCTIONS="  Local-only setup (containers live in the podman machine VM):

    1. Find your podman socket path:
         podman machine inspect | python3 -c \"
import sys, json
d = json.load(sys.stdin)
print(d[0]['ConnectionInfo']['PodmanSocket']['Path'])
\"
       (typically ~/.local/share/containers/podman/machine/.../podman.sock)

    2. In VSCode settings.json:
         \"dev.containers.dockerPath\": \"podman\"

    3. Command Palette -> 'Dev Containers: Attach to Running Container...'
       -> pick '$CNAME'."
else
    VSCODE_SETUP_INSTRUCTIONS="  If THIS host is your local machine:

    1. In VSCode settings.json:
         \"dev.containers.dockerPath\": \"podman\"

    2. Command Palette -> 'Dev Containers: Attach to Running Container...'
       -> pick '$CNAME'.

  If THIS host is a remote (Hetzner/Oracle/etc.) that you SSH into:

    1. Install the Remote-SSH extension on your laptop and connect to
       this host (F1 -> 'Remote-SSH: Connect to Host...').
    2. In the resulting remote VSCode window, install Dev Containers
       (it installs into the remote, not your laptop).
    3. F1 -> 'Dev Containers: Attach to Running Container...' ->
       pick '$CNAME'.
    See docs/'Running on a remote host.md' for the full walkthrough."
fi

TUNNEL_BANNER=""
if [[ "$EXPOSE_WEBAPP" == "true" ]]; then
    log "waiting for cloudflared tunnel URL (up to 30s)"
    TUNNEL_URL=""
    for _ in $(seq 1 60); do
        TUNNEL_URL=$(grep -oE 'https://[a-z0-9-]+\.trycloudflare\.com' /root/.logs/tunnel.log 2>/dev/null | grep -v '^https://api\.trycloudflare\.com$' | head -1 || true)
        [[ -n "$TUNNEL_URL" ]] && break
        sleep 0.5
    done
    if [[ -n "$TUNNEL_URL" ]]; then
        echo "$TUNNEL_URL" > /root/.logs/tunnel-url.txt
        TUNNEL_BANNER=$(cat <<TBEOF

============================================================
  PUBLIC TUNNEL ACTIVE — webapp exposed to the internet
============================================================

  $TUNNEL_URL

  Anyone with this URL can reach your webapp; the URL is the
  only access control. Stable until the next ./launch.sh.
  Cached at /root/.logs/tunnel-url.txt inside the container.

  attach: podman exec -it $CNAME tmux attach -t tunnel
  log:    podman exec -it $CNAME tail -f /root/.logs/tunnel.log

============================================================
TBEOF
)
    else
        TUNNEL_BANNER=$(cat <<TBEOF

============================================================
  PUBLIC TUNNEL — URL not detected after 30s
============================================================

  Check the tunnel session:
    podman exec -it $CNAME tmux attach -t tunnel
    podman exec -it $CNAME tail -f /root/.logs/tunnel.log

============================================================
TBEOF
)
    fi
fi

NTFY_BANNER=""
if [[ "${ENABLE_NOTIFY_SH_ALERTS:-false}" == "true" && -n "${NTFY_SH_TOPIC:-}" ]]; then
    NTFY_BANNER=$(cat <<NBEOF

============================================================
  MOBILE NOTIFICATIONS via ntfy.sh
============================================================

  You are also subscribed to mobile notifications via ntfy.sh.
  Subscribe to the following topic name to access it via the
  ntfy.sh app: $NTFY_SH_TOPIC

============================================================
NBEOF
)
fi

print_banner() {
    cat <<EOF

============================================================
  Dev container '$CNAME' is up and running.
============================================================

Shell into the container:
  podman exec -it $CNAME bash

Start Claude Code (remote control is on by default — pair from
claude.ai/code or the mobile app to drive it from your phone):
  podman exec -it $CNAME run-claude
  (cds into the repo, opens the 'claude' tmux session, and runs
   claude --dangerously-skip-permissions; re-running re-attaches.
   Ctrl-a d detaches without killing it.)

Connect with VSCode (Dev Containers):

${VSCODE_SETUP_INSTRUCTIONS}

  Once attached, install the 'Claude Code' extension — it lands on the
  container's persistent volume and survives relaunches.

To stop the container:
  podman stop $CNAME
  or: press Ctrl+C / Ctrl+D in this terminal

============================================================
$TUNNEL_BANNER
$NTFY_BANNER

EOF
}

print_banner

trap 'echo; echo "shutting down..."; exit 0' INT TERM

# Optional per-project launch hook. Runs from the repo root on every container
# start. If it blocks (e.g. a foreground dev server), the container stays alive
# on it and the cat blocker below is never reached. If it returns, we fall
# through to cat. Failures are logged but don't kill the container, so a broken
# hook doesn't prevent attaching for debugging.
if [[ -n "${ON_LAUNCH_SCRIPT:-}" ]]; then
    log "running ON_LAUNCH_SCRIPT: $ON_LAUNCH_SCRIPT"
    bash -c "$ON_LAUNCH_SCRIPT" || echo "  warning: ON_LAUNCH_SCRIPT exited with status $?"
    # Reprint the banner so the attach/VSCode instructions aren't buried
    # under the hook's output. Only reached if the hook returned (a blocking
    # foreground server never falls through here).
    print_banner
fi

cat >/dev/null || true
echo
echo "stdin closed, shutting down..."
