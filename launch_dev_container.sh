#!/usr/bin/env bash
set -euo pipefail

# Linux rootless podman only. Egress filtering happens INSIDE the container:
# firewall-entrypoint.sh (the container's PID 1) installs an nft default-deny
# filter and a hostname-allowlist forward proxy, then drops to the non-root
# `dev` user. The host does no firewall work, needs no sudo, and is otherwise
# uninvolved in egress — so there is a single, simple code path here.

if [[ "$(uname -s)" != "Linux" ]]; then
    echo "error: this harness supports Linux only (rootless podman)." >&2
    exit 1
fi

# ---- arg parsing -----------------------------------------------------------

RESET=false
RECREATE=false
REBUILD_BASE=false
VERIFY=false
DISABLE_NETWORK_BLOCK=false
UPDATE=false
PULL=false
ENV_FILE=".env"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --reset)                  RESET=true; shift ;;
        --recreate)               RECREATE=true; shift ;;
        --rebuild-base)           REBUILD_BASE=true; shift ;;
        --verify)                 VERIFY=true; shift ;;
        --disable-network-block)  DISABLE_NETWORK_BLOCK=true; shift ;;
        --update)                 UPDATE=true; shift ;;
        --pull)                   PULL=true; shift ;;
        -h|--help)
            cat <<'EOF'
usage: ./launch.sh [--reset] [--recreate] [--rebuild-base] [--verify]
                   [--disable-network-block] [--update] [--pull] [env-file]

Linux rootless podman. Egress is filtered inside the container: an nft
default-deny filter plus a hostname-allowlist forward proxy (tinyproxy). The
workload runs as the non-root `dev` user and can only reach the internet
through the proxy, for hostnames listed in WHITELIST_HOSTS.

prereqs: podman (rootless), pasta (apt install passt).

  --reset                   remove the container and wipe the persistent volume
                            (fresh create on next launch, picks up config edits).
  --recreate                remove the container but KEEP the persistent volume,
                            so the next launch is a fresh `podman run` (picks up
                            new --publish / --env / etc.) while your checkout and
                            Claude state under /home/dev survive. Use after .env edits.
  --rebuild-base            force rebuild of the base image (--no-cache).
  --verify                  run the egress filter + proxy self-check in a
                            throwaway container and exit.
  --disable-network-block   run with NO egress filtering (all outbound permitted).
                            mutually exclusive with --verify.
  --update                  in-container refresh (apt upgrade, mise, claude code,
                            lazyvim) against an already-running container.
  --pull                    on next start, fast-forward the project repo at
                            /home/dev/work/<project> (git fetch + pull --ff-only).
  env-file                  path to env file (default: .env)
EOF
            exit 0
            ;;
        -*) echo "error: unknown flag $1" >&2; exit 1 ;;
        *)  ENV_FILE="$1"; shift ;;
    esac
done

if $VERIFY && $DISABLE_NETWORK_BLOCK; then
    echo "error: --verify and --disable-network-block are mutually exclusive." >&2
    exit 1
fi
if $UPDATE && ( $RESET || $RECREATE || $REBUILD_BASE || $VERIFY ); then
    echo "error: --update is mutually exclusive with --reset, --recreate, --rebuild-base, --verify." >&2
    exit 1
fi
if $RECREATE && $RESET; then
    echo "error: --recreate and --reset are mutually exclusive (use --reset to also wipe the volume)." >&2
    exit 1
fi
if $RECREATE && $VERIFY; then
    echo "error: --recreate and --verify are mutually exclusive." >&2
    exit 1
fi
if $PULL && ( $VERIFY || $UPDATE ); then
    echo "error: --pull is mutually exclusive with --verify and --update." >&2
    exit 1
fi

# ---- env -------------------------------------------------------------------

if [[ ! -f "$ENV_FILE" ]]; then
    echo "error: $ENV_FILE not found. copy sample.env to .env and edit it." >&2
    exit 1
fi

set -a
# shellcheck disable=SC1090
source "$ENV_FILE"
set +a

: "${PROJECT_NAME:?PROJECT_NAME must be set}"
: "${REPO_URL:?REPO_URL must be set}"
: "${DEPLOY_KEY_PATH:?DEPLOY_KEY_PATH must be set}"
: "${WEBAPP_PORT:?WEBAPP_PORT must be set}"

ON_LAUNCH_SCRIPT="${ON_LAUNCH_SCRIPT:-}"
# Runs as ROOT, from the repo dir, after the clone and before ON_LAUNCH_SCRIPT.
# Multi-line is fine (executed as a script). Use for root-only launch setup
# (apt install, starting a system service like clickhouse-server, etc.).
ON_LAUNCH_ROOT_SCRIPT="${ON_LAUNCH_ROOT_SCRIPT:-}"
# Safety cap (seconds) so a hung ON_LAUNCH_ROOT_SCRIPT can't block startup.
ON_LAUNCH_ROOT_TIMEOUT="${ON_LAUNCH_ROOT_TIMEOUT:-600}"
ENABLE_NOTIFY_SH_ALERTS="${ENABLE_NOTIFY_SH_ALERTS:-false}"
NTFY_SH_TOPIC="${NTFY_SH_TOPIC:-}"
if [[ "$ENABLE_NOTIFY_SH_ALERTS" == "true" && -z "$NTFY_SH_TOPIC" ]]; then
    echo "error: ENABLE_NOTIFY_SH_ALERTS=true but NTFY_SH_TOPIC is unset" >&2
    exit 1
fi

MEM_LIMIT="${MEM_LIMIT:-8g}"
CPU_LIMIT="${CPU_LIMIT:-8}"
PIDS_LIMIT="${PIDS_LIMIT:-16384}"
if ! $DISABLE_NETWORK_BLOCK; then
    : "${WHITELIST_HOSTS:?WHITELIST_HOSTS must be set}"
fi

# Optional direct-IP allowlist for fixed internal targets (DBs, internal
# registries, etc.) that the workload may reach WITHOUT the proxy. Whitespace-
# separated IPs/CIDRs. Default empty (workload reaches the internet only via the
# hostname proxy).
INTERNAL_ALLOW_CIDRS="${INTERNAL_ALLOW_CIDRS:-}"

# Canary IP the container's self-check dials to prove the filter drops
# non-allowlisted destinations. Must be a real, routable IP not in the allowlist.
CANARY_BLOCKED_IP="${CANARY_BLOCKED_IP:-93.184.216.34}"

if [[ ! -f "$DEPLOY_KEY_PATH" ]]; then
    echo "error: DEPLOY_KEY_PATH does not exist: $DEPLOY_KEY_PATH" >&2
    exit 1
fi

SHARED_DATA_PATH="${SHARED_DATA_PATH:-}"
declare -a SHARED_DATA_VOLUME_ARGS=()
if [[ -n "$SHARED_DATA_PATH" ]]; then
    if [[ ! -d "$SHARED_DATA_PATH" ]]; then
        echo "error: SHARED_DATA_PATH is not a directory: $SHARED_DATA_PATH" >&2
        exit 1
    fi
    SHARED_DATA_PATH="$(cd "$SHARED_DATA_PATH" && pwd)"
    SHARED_DATA_VOLUME_ARGS=(--volume "${SHARED_DATA_PATH}:/home/dev/shared_data:ro")
fi

EXTRA_PORTS="${EXTRA_PORTS:-}"
declare -a EXTRA_PUBLISH_ARGS=()
declare -a EXTRA_PORTS_DESC=()
for entry in $EXTRA_PORTS; do
    if [[ "$entry" =~ ^([0-9]+):([0-9]+)$ ]]; then
        host_port="${BASH_REMATCH[1]}"; container_port="${BASH_REMATCH[2]}"
    elif [[ "$entry" =~ ^([0-9]+)$ ]]; then
        host_port="${BASH_REMATCH[1]}"; container_port="$host_port"
    else
        echo "error: EXTRA_PORTS entry is not a port or host:container mapping: '$entry'" >&2
        exit 1
    fi
    for p in "$host_port" "$container_port"; do
        if (( p < 1 || p > 65535 )); then
            echo "error: EXTRA_PORTS entry has out-of-range port: '$entry'" >&2
            exit 1
        fi
    done
    if [[ "$host_port" == "$WEBAPP_PORT" ]]; then
        echo "error: EXTRA_PORTS host port $host_port collides with WEBAPP_PORT" >&2
        exit 1
    fi
    EXTRA_PUBLISH_ARGS+=(--publish "127.0.0.1:${host_port}:${container_port}")
    if [[ "$host_port" == "$container_port" ]]; then
        EXTRA_PORTS_DESC+=("$host_port")
    else
        EXTRA_PORTS_DESC+=("${host_port}->${container_port}")
    fi
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SETUP_SCRIPT="$SCRIPT_DIR/setup.sh"
DOCKERFILE="$SCRIPT_DIR/Dockerfile"
NOTIFY_LISTENER="$SCRIPT_DIR/host_listener.py"
ENTRYPOINT_SCRIPT="$SCRIPT_DIR/container/egress/firewall-entrypoint.sh"
TINYPROXY_CONF="$SCRIPT_DIR/container/egress/tinyproxy.conf"
[[ -f "$SETUP_SCRIPT"      ]] || { echo "error: setup.sh not found at $SETUP_SCRIPT" >&2; exit 1; }
[[ -f "$DOCKERFILE"        ]] || { echo "error: Dockerfile not found at $DOCKERFILE" >&2; exit 1; }
[[ -f "$ENTRYPOINT_SCRIPT" ]] || { echo "error: firewall-entrypoint.sh not found at $ENTRYPOINT_SCRIPT" >&2; exit 1; }
[[ -f "$TINYPROXY_CONF"    ]] || { echo "error: tinyproxy.conf not found at $TINYPROXY_CONF" >&2; exit 1; }

# ---- notify endpoint (Linux FIFO) ------------------------------------------
# A FIFO under XDG_RUNTIME_DIR. Some kernels refuse to let a process in the
# container's mount/user namespace connect() to a bind-mounted unix socket even
# when ownership/permissions/LSM allow it; writing to a bind-mounted FIFO has no
# such restriction.
NOTIFY_SOCK="${XDG_RUNTIME_DIR:-/run/user/$UID}/rc-notify.fifo"
notify_exists() { [[ -p "$NOTIFY_SOCK" ]]; }

# ---- image / container identifiers -----------------------------------------

BASE_IMAGE_NAME="remote-code-base:latest"

md5_hex() { printf '%s' "$1" | md5sum | awk '{print $1}'; }

# Per-project suffix on BOTH the image tag and the container name so VS Code Dev
# Containers (which caches attach config by image name AND container name) keeps
# each project — and the same project across hosts — distinct. Normally set by
# create-dev-container.sh in .env; otherwise a deterministic project+host hash.
IMAGE_SUFFIX="${IMAGE_SUFFIX:-$(md5_hex "${PROJECT_NAME}@$(hostname 2>/dev/null)" | cut -c1-4)}"

CONTAINER_NAME="remote-code-${PROJECT_NAME}-${IMAGE_SUFFIX}"
LEGACY_CONTAINER_NAME="remote-code-${PROJECT_NAME}"
VOLUME_NAME="remote-code-vol-${PROJECT_NAME}"

# Image names must be lowercase [a-z0-9._-]; re-slugify PROJECT_NAME for the tag.
IMAGE_PROJECT_SLUG=$(printf '%s' "$PROJECT_NAME" \
    | tr 'A-Z' 'a-z' | tr -c 'a-z0-9_.-' '-' | sed -E 's/-+/-/g; s/^-//; s/-$//')
IMAGE_NAME="remote-code-${IMAGE_PROJECT_SLUG}-${IMAGE_SUFFIX}:latest"

# ---- cloudflared edge (only when EXPOSE_WEBAPP) ----------------------------
# cloudflared dials Cloudflare's tunnel edge on TCP/7844 with a bespoke protocol
# (not HTTP), so it can't go through the proxy — its edge IPs are allowed
# directly by the in-container nft filter (passed as TUNNEL_EDGE_IPS). The
# trycloudflare API host is allowlisted in the proxy via WHITELIST_HOSTS below.
EXPOSE_WEBAPP="${EXPOSE_WEBAPP:-false}"
TUNNEL_HOSTS=""
TUNNEL_EDGE_IPS=""
TUNNEL_API_IPS=""
if [[ "$EXPOSE_WEBAPP" == "true" ]]; then
    TUNNEL_HOSTS="api.trycloudflare.com"
    TUNNEL_EDGE_IPS="198.41.192.167 198.41.192.227 198.41.200.13 198.41.200.193"
    # cloudflared registers the quick tunnel with a direct POST to
    # api.trycloudflare.com:443 and does NOT honor HTTP_PROXY for it, so the
    # proxy allowlist alone isn't enough — resolve it and allow those IPs
    # directly on :443 (like the edge IPs on :7844). Cloudflare anycast, stable.
    TUNNEL_API_IPS="$(getent ahostsv4 api.trycloudflare.com 2>/dev/null | awk '{print $1}' | sort -u | tr '\n' ' ')"
    [[ -n "$TUNNEL_API_IPS" ]] || echo "    warn: could not resolve api.trycloudflare.com — tunnel registration may fail" >&2
fi

# The proxy allowlist the container enforces = WHITELIST_HOSTS plus the git host
# (so the deploy-key clone works) plus the tunnel API host. Passed in as
# WHITELIST_HOSTS; firewall-entrypoint.sh turns it into the tinyproxy filter.
GIT_HOST="$(echo "$REPO_URL" | sed -E 's#^(git@|ssh://git@|https://)##; s#[:/].*$##')"
CONTAINER_WHITELIST_HOSTS="$GIT_HOST ${WHITELIST_HOSTS:-} $TUNNEL_HOSTS"

# Generate the proxy hostname allowlist on the host and bind-mount it into the
# container (below). Because launch.sh runs on every invocation and the
# entrypoint reads this file at start, editing WHITELIST_HOSTS in .env and then
# a plain `./launch.sh` (which `podman start`s the existing container, re-running
# the entrypoint) updates the allowlist WITHOUT --recreate — so you don't have to
# destroy the container (and its apt-installed packages) just to allow a new host.
STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/remote-code-harness"
mkdir -p "$STATE_DIR"
ALLOWLIST_FILE="$STATE_DIR/allowlist-${CONTAINER_NAME}.txt"
: > "$ALLOWLIST_FILE"
for _h in $CONTAINER_WHITELIST_HOSTS; do
    _esc=$(printf '%s' "$_h" | sed 's/[.[\*^$()+?{|]/\\&/g')
    printf '(^|\\.)%s$\n' "$_esc" >> "$ALLOWLIST_FILE"
done
chmod 644 "$ALLOWLIST_FILE"

# ---- --update: in-container refresh ----------------------------------------
if $UPDATE; then
    if ! podman container inspect -f '{{.State.Running}}' "$CONTAINER_NAME" 2>/dev/null | grep -qi true; then
        echo "error: container $CONTAINER_NAME is not running. launch it in another terminal first." >&2
        exit 1
    fi
    echo "==> --update: apt upgrade (as root, via the proxy)"
    podman exec "$CONTAINER_NAME" bash -c '
set -e
export DEBIAN_FRONTEND=noninteractive
apt-get update && apt-get -y upgrade'
    echo "==> --update: mise / claude code / lazyvim (as dev)"
    podman exec --user dev "$CONTAINER_NAME" bash -c '
set -e
export HOME=/home/dev
export PATH="/home/dev/.local/bin:/home/dev/.local/share/mise/shims:/home/dev/.local/share/pnpm/bin:$PATH"
eval "$(/home/dev/.local/bin/mise activate bash)"
mise self-update -y || true
mise upgrade
pnpm update -g @anthropic-ai/claude-code
node "$(pnpm root -g)/@anthropic-ai/claude-code/install.cjs"
nvim --headless "+Lazy! sync" +qa'
    echo "==> --update: done"
    exit 0
fi

# ---- pre-flight ------------------------------------------------------------

echo "==> pre-flight (linux rootless)"
if [[ $EUID -eq 0 ]]; then
    echo "error: run as a regular user — rootless podman only (no sudo)." >&2
    exit 1
fi
command -v podman >/dev/null || { echo "error: podman not installed." >&2; exit 1; }
command -v pasta  >/dev/null || { echo "error: pasta not installed. try 'apt install passt'." >&2; exit 1; }
if ! podman info --format '{{.Host.Security.Rootless}}' 2>/dev/null | grep -qi true; then
    echo "error: podman is not configured for rootless operation." >&2
    exit 1
fi

# No cleanup trap is needed: the egress filter and proxy live inside the
# container (nothing host-side to undo), and `podman run -it` / `start -ai` stop
# the container themselves when the foreground process exits.

# ---- --reset: confirm destroy up-front -------------------------------------

if ! $VERIFY && $RESET; then
    if podman volume inspect "$VOLUME_NAME" >/dev/null 2>&1 \
       && podman image inspect "$BASE_IMAGE_NAME" >/dev/null 2>&1; then
        echo "==> --reset: scanning /home/dev/work for uncommitted/unpushed git state"
        DIRTY_REPORT=$(
            podman run --rm --network=none \
                --volume "$VOLUME_NAME:/home/dev:ro" \
                "$BASE_IMAGE_NAME" \
                bash -c '
set +e
git config --global --add safe.directory "*" 2>/dev/null
while IFS= read -r gitpath; do
    repo=${gitpath%/.git}
    cd "$repo" 2>/dev/null || continue
    status=$(git status --porcelain 2>/dev/null)
    stashes=$(git stash list 2>/dev/null)
    if [ -f "$gitpath" ]; then
        unpushed=0
    else
        unpushed=$(git rev-list --count --all --not --remotes 2>/dev/null || echo 0)
    fi
    if [ -n "$status" ] || [ "${unpushed:-0}" -gt 0 ] || [ -n "$stashes" ]; then
        echo "--- $repo ---"
        [ -n "$status" ] && { echo "  working tree:"; echo "$status" | sed "s/^/    /"; }
        [ "${unpushed:-0}" -gt 0 ] && echo "  unpushed commits: $unpushed (not reachable from any remote)"
        [ -n "$stashes" ] && { echo "  stashes:"; echo "$stashes" | sed "s/^/    /"; }
        echo
    fi
done < <(find /home/dev/work -maxdepth 5 -name .git \( -type d -o -type f \) 2>/dev/null)
' 2>/dev/null
        )
        if [[ -n "$DIRTY_REPORT" ]]; then
            echo
            echo "$DIRTY_REPORT"
            echo "The above state in /home/dev/work would be LOST on --reset (volume wipe)."
            read -r -p "Type 'yes' to proceed: " confirm
            [[ "$confirm" == "yes" ]] || { echo "aborted."; exit 1; }
        fi
    fi
fi

# ---- base image ------------------------------------------------------------

# True when the Dockerfile or any baked-in container/ source file is newer than
# the current base image — i.e. a pull/edit changed something the image embeds.
# mtime is only the trigger; the rebuild below is cache-enabled, so podman's own
# content hashing does the real work (an unchanged COPY is a cache hit, ~instant)
# and a false positive costs a second, never a full rebuild. Edits/pulls always
# move mtimes forward past the image, so it won't miss a real change.
base_image_is_stale() {
    local created epoch newest
    created=$(podman image inspect --format '{{.Created}}' "$BASE_IMAGE_NAME" 2>/dev/null) || return 1
    epoch=$(date -d "$created" +%s 2>/dev/null) || return 0
    # Only files BAKED into the image count. container/egress/* and setup.sh are
    # bind-mounted at runtime, so editing them applies on the next launch with no
    # rebuild — don't trigger one for them.
    newest=$(find "$DOCKERFILE" "$SCRIPT_DIR/container/bin" "$SCRIPT_DIR/container/claude" \
        -type f -printf '%T@\n' 2>/dev/null | sort -rn | head -1 | cut -d. -f1)
    [[ -n "$newest" && "$newest" -gt "$epoch" ]]
}

if $REBUILD_BASE; then
    echo "==> rebuilding base image $BASE_IMAGE_NAME (--no-cache)"
    podman build --no-cache -t "$BASE_IMAGE_NAME" -f "$DOCKERFILE" "$SCRIPT_DIR"
elif ! podman image inspect "$BASE_IMAGE_NAME" >/dev/null 2>&1; then
    echo "==> building base image $BASE_IMAGE_NAME"
    podman build -t "$BASE_IMAGE_NAME" -f "$DOCKERFILE" "$SCRIPT_DIR"
elif base_image_is_stale; then
    echo "==> base image is older than the Dockerfile/container files — cached rebuild"
    echo "    (use --rebuild-base to force a full --no-cache rebuild)"
    podman build -t "$BASE_IMAGE_NAME" -f "$DOCKERFILE" "$SCRIPT_DIR"
else
    echo "==> using cached base image $BASE_IMAGE_NAME"
fi

# Per-project image tag (cheap alias of the base; gives VS Code a distinct image
# name per project). Then prune stale per-project tags from earlier launches.
podman image tag "$BASE_IMAGE_NAME" "$IMAGE_NAME"
echo "==> tagged base image as $IMAGE_NAME"
while IFS= read -r ref; do
    ref=${ref#localhost/}
    [[ "$ref" == "$IMAGE_NAME" ]] && continue
    case "$ref" in
        "remote-code-${IMAGE_PROJECT_SLUG}-"????":latest")
            echo "==> removing stale project image tag $ref"
            podman untag "$ref" >/dev/null 2>&1 || true
            ;;
    esac
done < <(podman image ls --format '{{.Repository}}:{{.Tag}}' 2>/dev/null)

# ---- --verify: throwaway container runs the firewall self-check ------------
if $VERIFY; then
    echo "==> --verify: running egress filter + proxy self-check in a throwaway container"
    podman run --rm \
        --cap-drop=ALL --cap-add=CHOWN --cap-add=DAC_OVERRIDE --cap-add=FOWNER \
        --cap-add=SETUID --cap-add=SETGID --cap-add=NET_ADMIN \
        --security-opt=no-new-privileges \
        --network=pasta \
        --env "VERIFY_ONLY=true" \
        --env "WHITELIST_HOSTS=$CONTAINER_WHITELIST_HOSTS" \
        --env "INTERNAL_ALLOW_CIDRS=$INTERNAL_ALLOW_CIDRS" \
        --env "TUNNEL_EDGE_IPS=$TUNNEL_EDGE_IPS" \
        --env "TUNNEL_API_IPS=$TUNNEL_API_IPS" \
        --env "CANARY_BLOCKED_IP=$CANARY_BLOCKED_IP" \
        --env "REPO_URL=$REPO_URL" \
        --volume "$ENTRYPOINT_SCRIPT:/usr/local/bin/firewall-entrypoint.sh:ro" \
        --volume "$TINYPROXY_CONF:/etc/tinyproxy/tinyproxy.conf:ro" \
        --volume "$ALLOWLIST_FILE:/etc/tinyproxy/egress-allowlist.txt:ro" \
        "$IMAGE_NAME" /usr/local/bin/firewall-entrypoint.sh
    echo "==> verify passed"
    exit 0
fi

# ---- container lifecycle ---------------------------------------------------

# Migration: remove a pre-suffix container left by the rename (volume is reused).
if [[ "$LEGACY_CONTAINER_NAME" != "$CONTAINER_NAME" ]] \
   && podman container inspect "$LEGACY_CONTAINER_NAME" >/dev/null 2>&1; then
    echo "==> removing legacy pre-suffix container $LEGACY_CONTAINER_NAME (volume preserved)"
    podman rm -f "$LEGACY_CONTAINER_NAME" >/dev/null 2>&1 || true
fi

if $RESET; then
    echo "==> --reset: removing container $CONTAINER_NAME and wiping volume $VOLUME_NAME"
    podman rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
    podman volume rm -f "$VOLUME_NAME" >/dev/null 2>&1 || true
elif $RECREATE; then
    if podman container inspect "$CONTAINER_NAME" >/dev/null 2>&1; then
        echo "==> --recreate: removing container $CONTAINER_NAME (volume $VOLUME_NAME preserved)"
        podman rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
    fi
fi

if ! podman volume inspect "$VOLUME_NAME" >/dev/null 2>&1; then
    podman volume create "$VOLUME_NAME" >/dev/null
    echo "==> created volume $VOLUME_NAME (first launch)"
else
    echo "==> reusing volume $VOLUME_NAME"
fi

# --pull: drop a one-shot sentinel into the volume for setup.sh to consume.
if $PULL; then
    echo "==> --pull: scheduling git pull --ff-only on next container start"
    podman run --rm --network=none --volume "$VOLUME_NAME:/home/dev" \
        "$IMAGE_NAME" touch /home/dev/.pull-on-next-start >/dev/null
fi

CONTAINER_EXISTS=false
podman container inspect "$CONTAINER_NAME" >/dev/null 2>&1 && CONTAINER_EXISTS=true

# ---- host-side notification listener (systemd --user) ----------------------
if systemctl --user is-active --quiet rc-notify.service 2>/dev/null && notify_exists; then
    echo "==> rc-notify listener already running"
elif [[ -f "${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/rc-notify.service" ]]; then
    echo "==> starting rc-notify listener (installed unit) on $NOTIFY_SOCK"
    systemctl --user start rc-notify.service 2>/dev/null || true
    for _ in 1 2 3 4 5 6 7 8 9 10; do notify_exists && break; sleep 0.1; done
elif [[ -f "$NOTIFY_LISTENER" ]] && command -v systemd-run >/dev/null 2>&1; then
    echo "==> starting rc-notify listener on $NOTIFY_SOCK"
    systemctl --user reset-failed rc-notify.service 2>/dev/null || true
    systemctl --user stop rc-notify.service 2>/dev/null || true
    systemd-run --user --quiet --unit=rc-notify.service \
        --description="remote-code-harness notification listener" \
        python3 "$NOTIFY_LISTENER"
    for _ in 1 2 3 4 5 6 7 8 9 10; do notify_exists && break; sleep 0.1; done
fi

declare -a NOTIFY_VOLUME_ARGS=()
if notify_exists; then
    NOTIFY_VOLUME_ARGS=(--volume "$NOTIFY_SOCK:/run/notify")
else
    echo "    warn: rc-notify endpoint not present at $NOTIFY_SOCK — container hooks will fail silently"
fi

# ---- launch ----------------------------------------------------------------

echo
echo "==> launching container $CONTAINER_NAME (linux rootless)"
echo "    repo:    $REPO_URL"
echo "    webapp:  port $WEBAPP_PORT"
[[ -n "$ON_LAUNCH_ROOT_SCRIPT" ]] && echo "    on-launch (root): set"
[[ -n "$ON_LAUNCH_SCRIPT" ]] && echo "    on-launch (dev): set"
$DISABLE_NETWORK_BLOCK && echo "    WARNING: --disable-network-block — egress filtering OFF"

if $CONTAINER_EXISTS; then
    echo "==> reusing existing container $CONTAINER_NAME (use --recreate/--reset to rebuild it)"
    exec podman start -ai "$CONTAINER_NAME"
fi

echo "==> creating new container $CONTAINER_NAME"
declare -a CAP_ADD=(
    --cap-add=CHOWN --cap-add=DAC_OVERRIDE --cap-add=FOWNER --cap-add=FSETID
    --cap-add=SETFCAP --cap-add=MKNOD --cap-add=SETUID --cap-add=SETGID
)
# NET_ADMIN lets the entrypoint install nft in the container's own netns; it is
# dropped before the workload starts (setpriv to non-root dev).
$DISABLE_NETWORK_BLOCK || CAP_ADD+=(--cap-add=NET_ADMIN)

declare -a PODMAN_ARGS=(
    run -it
    --name "$CONTAINER_NAME"
    --hostname remote-code
    --network=pasta
    --memory="$MEM_LIMIT"
    --cpus="$CPU_LIMIT"
    --pids-limit="$PIDS_LIMIT"
    --cap-drop=ALL
    "${CAP_ADD[@]}"
    --security-opt=no-new-privileges
    --volume "$VOLUME_NAME:/home/dev"
    --volume "$DEPLOY_KEY_PATH:/tmp/deploy_key:ro"
    --volume "$SETUP_SCRIPT:/setup.sh:ro"
    --volume "$ENTRYPOINT_SCRIPT:/usr/local/bin/firewall-entrypoint.sh:ro"
    --volume "$TINYPROXY_CONF:/etc/tinyproxy/tinyproxy.conf:ro"
    --volume "$ALLOWLIST_FILE:/etc/tinyproxy/egress-allowlist.txt:ro"
    ${SHARED_DATA_VOLUME_ARGS[@]:+"${SHARED_DATA_VOLUME_ARGS[@]}"}
    ${NOTIFY_VOLUME_ARGS[@]:+"${NOTIFY_VOLUME_ARGS[@]}"}
    --env "PROJECT_NAME=$PROJECT_NAME"
    --env "CONTAINER_NAME=$CONTAINER_NAME"
    --env "REPO_URL=$REPO_URL"
    --env "WEBAPP_PORT=$WEBAPP_PORT"
    --env "ON_LAUNCH_SCRIPT=$ON_LAUNCH_SCRIPT"
    --env "ON_LAUNCH_ROOT_SCRIPT=$ON_LAUNCH_ROOT_SCRIPT"
    --env "ON_LAUNCH_ROOT_TIMEOUT=$ON_LAUNCH_ROOT_TIMEOUT"
    --env "CANARY_BLOCKED_IP=$CANARY_BLOCKED_IP"
    --env "HOST_OS=linux"
    --env "DISABLE_NETWORK_BLOCK=$DISABLE_NETWORK_BLOCK"
    --env "EXPOSE_WEBAPP=$EXPOSE_WEBAPP"
    --env "TUNNEL_EDGE_IPS=$TUNNEL_EDGE_IPS"
    --env "TUNNEL_API_IPS=$TUNNEL_API_IPS"
    --env "WHITELIST_HOSTS=$CONTAINER_WHITELIST_HOSTS"
    --env "INTERNAL_ALLOW_CIDRS=$INTERNAL_ALLOW_CIDRS"
    --env "ENABLE_NOTIFY_SH_ALERTS=$ENABLE_NOTIFY_SH_ALERTS"
    --env "NTFY_SH_TOPIC=$NTFY_SH_TOPIC"
    --publish "127.0.0.1:${WEBAPP_PORT}:${WEBAPP_PORT}"
    ${EXTRA_PUBLISH_ARGS[@]:+"${EXTRA_PUBLISH_ARGS[@]}"}
)

exec podman "${PODMAN_ARGS[@]}" "$IMAGE_NAME" /usr/local/bin/firewall-entrypoint.sh
