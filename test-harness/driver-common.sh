#!/usr/bin/env bash
# Shared setup + helpers sourced by every milestone driver.

# logind isn't present to provision /run/user/<uid>. Give introdus (and the
# nested podman) an explicit XDG_RUNTIME_DIR under /tmp — /tmp supports the netns
# bind-mounts pasta needs, whereas the home overlay fs does not. Used for the
# notify FIFO, runtime state, and the nested podman's RunRoot/netns.
export XDG_RUNTIME_DIR="/tmp/xdg-$(id -u)"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"

# A small PUBLIC repo, cloned over HTTPS through the in-container proxy (see
# harness_ensure_base). Override with HARNESS_REPO_URL.
HARNESS_REPO_URL="${HARNESS_REPO_URL:-https://github.com/octocat/Hello-World.git}"

# A throwaway deploy key so input validation passes (the public clone never
# uses it).
harness_dummy_key() {
    mkdir -p "$HOME/.ssh"
    [[ -f "$HOME/.ssh/harness-key" ]] \
        || ssh-keygen -t ed25519 -N "" -C harness -f "$HOME/.ssh/harness-key" >/dev/null
}

# Write a project .env. $1=project dir, $2=optional SESSION_NAME.
harness_write_env() {
    local proj="$1" session="${2:-}"
    mkdir -p "$proj"
    {
        echo "PROJECT_NAME=harness"
        echo "REPO_URL=$HARNESS_REPO_URL"
        echo "DEPLOY_KEY_PATH=$HOME/.ssh/harness-key"
        echo "WEBAPP_PORT=3000"
        echo 'INSTALL_AGENTS="claude"'
        [[ -n "$session" ]] && echo "SESSION_NAME=$session"
    } > "$proj/.env"
}

# Ensure introdus-base:latest exists (cached across runs via the storage volume)
# and carries the TEST-ONLY https_proxy overlay so a keyless public HTTPS clone
# routes through the in-container allowlist proxy. $1 = a project dir with .env.
harness_ensure_base() {
    if podman image exists introdus-base:latest; then
        echo "==> base image already present (cached)"
    else
        echo "==> building base image via 'introdus verify' (first run, cached after)"
        ( cd "$1" && introdus verify )
    fi
    echo "==> TEST-ONLY: overlay https_proxy onto introdus-base (clone via proxy)"
    local d
    d="$(mktemp -d)"
    cat > "$d/Containerfile" <<'CF'
FROM introdus-base:latest
# The in-container egress proxy still enforces the hostname allowlist; this only
# selects HTTP-proxy transport so a keyless public clone works in the harness.
ENV https_proxy=http://127.0.0.1:8888 http_proxy=http://127.0.0.1:8888 \
    HTTPS_PROXY=http://127.0.0.1:8888 HTTP_PROXY=http://127.0.0.1:8888
CF
    podman build -t introdus-base:latest -f "$d/Containerfile" "$d" >/dev/null
}

# Remove any leftover harness container + volume from a prior run (they persist
# in the storage volume).
harness_clean() {
    local c
    for c in $(podman ps -aq --filter "name=introdus-harness-" 2>/dev/null); do
        podman rm -f "$c" >/dev/null 2>&1 || true
    done
    podman volume rm -f introdus-vol-harness >/dev/null 2>&1 || true
}

# Print the running harness container name (empty if none).
harness_container() {
    podman ps --format '{{.Names}}' | grep '^introdus-harness-' || true
}
