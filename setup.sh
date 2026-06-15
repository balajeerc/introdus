#!/usr/bin/env bash
# Runs inside the container as the non-root `dev` user, in two phases driven by
# firewall-entrypoint.sh:
#   setup.sh prepare  — clone the repo + apply --pull. The entrypoint then runs
#                       ON_LAUNCH_ROOT_SCRIPT (as root, with the repo present).
#   setup.sh serve    — start the cloudflared tunnel (if enabled), run
#                       ON_LAUNCH_SCRIPT (as dev), print the banner, and idle.
# With no argument it runs both (prepare then serve).
#
# Before this runs, the firewall entrypoint has already, as root: installed the
# nft egress filter, started the hostname-allowlist proxy, run the egress
# self-check, and staged the deploy key into ~/.ssh. ssh reaches git hosts
# through the egress proxy (see /home/dev/.ssh/config). Safe to re-run —
# /home/dev is a persistent podman volume.
set -euo pipefail

: "${PROJECT_NAME:?}"
: "${REPO_URL:?}"
: "${WEBAPP_PORT:?}"

# HOST_OS is set by launch.sh and only controls which host-side instructions are
# printed at the end (the container is always Linux).
HOST_OS="${HOST_OS:-linux}"
EXPOSE_WEBAPP="${EXPOSE_WEBAPP:-false}"

HOME="${HOME:-/home/dev}"
WORKDIR="${HOME}/work/${PROJECT_NAME}"

# Container name as created by launch.sh (carries the per-project suffix). Used
# only to print correct host-side exec/stop commands. Note: exec hints below use
# `--user dev` because the workload, its tmux sessions, and its files all belong
# to dev — a default (root) exec would miss dev's per-uid tmux socket.
CNAME="${CONTAINER_NAME:-remote-code-$PROJECT_NAME}"

log() { printf '\n==> %s\n' "$*"; }

# Non-interactive bash does not source .bashrc, so activate mise explicitly.
export PATH="${HOME}/.local/bin:$PATH"
eval "$(${HOME}/.local/bin/mise activate bash)"

# ---- prepare: clone + --pull (repo ends up owned by dev) --------------------
do_prepare() {
    # known_hosts: StrictHostKeyChecking=accept-new (in ~/.ssh/config) records the
    # host key on first clone over the proxy tunnel. ssh-keyscan is NOT usable
    # here — it dials :22 directly and the egress filter drops that.
    mkdir -p "${HOME}/.ssh"
    chmod 700 "${HOME}/.ssh" 2>/dev/null || true
    touch "${HOME}/.ssh/known_hosts"
    chmod 600 "${HOME}/.ssh/known_hosts"

    if [[ -d "$WORKDIR/.git" ]]; then
        log "repo already present at $WORKDIR — skipping clone"
    else
        log "cloning $REPO_URL (git-over-SSH tunneled through the egress proxy)"
        mkdir -p "$(dirname "$WORKDIR")"
        git clone "$REPO_URL" "$WORKDIR"
    fi
    cd "$WORKDIR"

    # One-shot --pull sentinel dropped by launch.sh. Always consume it (rm first)
    # so a failed pull doesn't keep retrying on every relaunch.
    if [[ -f "${HOME}/.pull-on-next-start" ]]; then
        rm -f "${HOME}/.pull-on-next-start"
        log "fast-forwarding repo (--pull)"
        if git fetch --prune; then
            if ! git pull --ff-only; then
                echo "  warning: not a fast-forward — repo left as-is. resolve manually inside the container."
            fi
        else
            echo "  warning: git fetch failed — repo left as-is"
        fi
    fi
}

print_banner() {
    cat <<EOF

============================================================
  Dev container '$CNAME' is up and running.
============================================================

Shell into the container (as the dev user):
  podman exec -it --user dev $CNAME bash

Start Claude Code (remote control is on by default — pair from
claude.ai/code or the mobile app to drive it from your phone):
  podman exec -it --user dev $CNAME run-claude
  (cds into the repo, opens the 'claude' tmux session, and runs
   claude --dangerously-skip-permissions; re-running re-attaches.
   Ctrl-a d detaches without killing it.)

Connect with VSCode (Dev Containers):

${VSCODE_SETUP_INSTRUCTIONS:-}

  Once attached, install the 'Claude Code' extension — it lands on the
  container's persistent volume and survives relaunches.

To stop the container:
  podman stop $CNAME
  or: press Ctrl+C / Ctrl+D in this terminal

============================================================
${TUNNEL_BANNER:-}
${NTFY_BANNER:-}

EOF
}

# ---- serve: tunnel + ON_LAUNCH_SCRIPT (dev) + banner + idle -----------------
do_serve() {
    cd "$WORKDIR" 2>/dev/null || cd "$HOME"
    mkdir -p "${HOME}/.logs"

    if [[ "$EXPOSE_WEBAPP" == "true" ]]; then
        log "starting cloudflared quick tunnel in tmux session 'tunnel' (-> port $WEBAPP_PORT)"
        : > "${HOME}/.logs/tunnel.log"
        rm -f "${HOME}/.logs/tunnel-url.txt"
        # Pin edge IPs and force HTTP/2 so cloudflared skips SRV-based edge
        # discovery and avoids QUIC/UDP. Edge IPs come from TUNNEL_EDGE_IPS, set
        # by launch.sh and allowed (by IP, on 7844) in the nft egress filter —
        # cloudflared's edge protocol can't go through the HTTP proxy.
        EDGE_ARGS=""
        for ip in ${TUNNEL_EDGE_IPS:-}; do
            EDGE_ARGS="$EDGE_ARGS --edge ${ip}:7844"
        done
        tmux new-session -d -s tunnel "cloudflared tunnel --protocol http2 $EDGE_ARGS --url http://localhost:$WEBAPP_PORT 2>&1; echo '[cloudflared exited]'; exec bash"
        tmux pipe-pane -t tunnel -o "cat >>${HOME}/.logs/tunnel.log"
        echo "  attach: podman exec -it --user dev $CNAME tmux attach -t tunnel"
        echo "  tail:   podman exec -it --user dev $CNAME tail -f ~/.logs/tunnel.log"
    fi

    # VSCode connection instructions are OS- and locality-aware.
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
            TUNNEL_URL=$(grep -oE 'https://[a-z0-9-]+\.trycloudflare\.com' "${HOME}/.logs/tunnel.log" 2>/dev/null | grep -v '^https://api\.trycloudflare\.com$' | head -1 || true)
            [[ -n "$TUNNEL_URL" ]] && break
            sleep 0.5
        done
        if [[ -n "$TUNNEL_URL" ]]; then
            echo "$TUNNEL_URL" > "${HOME}/.logs/tunnel-url.txt"
            TUNNEL_BANNER=$(cat <<TBEOF

============================================================
  PUBLIC TUNNEL ACTIVE — webapp exposed to the internet
============================================================

  $TUNNEL_URL

  Anyone with this URL can reach your webapp; the URL is the
  only access control. Stable until the next ./launch.sh.
  Cached at ~/.logs/tunnel-url.txt inside the container.

  attach: podman exec -it --user dev $CNAME tmux attach -t tunnel
  log:    podman exec -it --user dev $CNAME tail -f ~/.logs/tunnel.log

============================================================
TBEOF
)
        else
            TUNNEL_BANNER=$(cat <<TBEOF

============================================================
  PUBLIC TUNNEL — URL not detected after 30s
============================================================

  Check the tunnel session:
    podman exec -it --user dev $CNAME tmux attach -t tunnel
    podman exec -it --user dev $CNAME tail -f ~/.logs/tunnel.log

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

    print_banner

    trap 'echo; echo "shutting down..."; exit 0' INT TERM

    # Optional per-project launch hook (runs as dev). Multi-line is fine — it's
    # executed as a script. If it blocks (e.g. a foreground dev server), the
    # container stays alive on it; if it returns, we fall through to the cat
    # blocker below.
    if [[ -n "${ON_LAUNCH_SCRIPT:-}" ]]; then
        log "running ON_LAUNCH_SCRIPT (as dev)"
        bash -c "$ON_LAUNCH_SCRIPT" || echo "  warning: ON_LAUNCH_SCRIPT exited with status $?"
        print_banner
    fi

    cat >/dev/null || true
    echo
    echo "stdin closed, shutting down..."
}

case "${1:-all}" in
    prepare) do_prepare ;;
    serve)   do_serve ;;
    *)       do_prepare; do_serve ;;
esac
