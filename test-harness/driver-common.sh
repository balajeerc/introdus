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

# Run `introdus launch` (attach fails without a tty, but the detached session
# persists), then wait for the session, the running container, and the clone.
# Sets HARNESS_SESSION and HARNESS_CNAME. $1=session name, $2=project dir.
harness_launch() {
    HARNESS_SESSION="$1"
    echo "==> introdus launch (builds the tmux session; attach fails without a tty)"
    ( cd "$2" && introdus launch ) >"$HOME/launch.log" 2>&1 || true

    local _
    for _ in $(seq 1 30); do tmux has-session -t "$HARNESS_SESSION" 2>/dev/null && break; sleep 1; done
    tmux has-session -t "$HARNESS_SESSION" 2>/dev/null \
        || { echo "FATAL: session not created"; cat "$HOME/launch.log"; return 1; }

    HARNESS_CNAME=""
    for _ in $(seq 1 60); do HARNESS_CNAME="$(harness_container)"; [[ -n "$HARNESS_CNAME" ]] && break; sleep 1; done
    [[ -n "$HARNESS_CNAME" ]] || { echo "FATAL: no container appeared"; return 1; }

    echo "==> waiting for the container to come up + clone…"
    for _ in $(seq 1 180); do
        podman exec --user dev "$HARNESS_CNAME" test -d /home/dev/work/harness/.git 2>/dev/null && break
        sleep 1
    done
    echo "    container: $HARNESS_CNAME"
}

# ---- helpers for driving the control menu (main-control window) -------------
# The menu is a full-screen ratatui chooser (alternate screen); its actions run
# on the normal screen with their sub-prompts drawn as inline modals. Two
# captures: mc_vis is the VISIBLE pane — the chooser frame while the menu is up,
# or the normal screen (inline modal + action output) during an action;
# mc_scroll includes scrollback, where an action's plain output accumulates.
mc_pane() { echo "${HARNESS_SESSION}:main-control"; }
mc_vis() { tmux capture-pane -t "$(mc_pane)" -p; }
mc_scroll() { tmux capture-pane -t "$(mc_pane)" -p -S -; }
mc_send() { tmux send-keys -t "$(mc_pane)" "$@"; }
mc_reset() { tmux clear-history -t "$(mc_pane)" 2>/dev/null || true; }

# The first menu section ("Terminals & agents") is drawn ONLY while the
# full-screen chooser is up (not during an inline sub-prompt modal or the action
# pause, which render on the normal screen), so it's a reliable "menu is ready
# for input" marker.
mc_ready() {
    local _
    for _ in $(seq 1 80); do mc_vis | grep -qF "Terminals & agents" && return 0; sleep 0.5; done
    echo "FATAL: menu never returned to the chooser:"; mc_vis | sed 's/^/      /'; return 1
}

# Wait for a substring in the VISIBLE pane — for inline sub-prompts (text/confirm)
# and the full-screen status panel.
mc_wait_prompt() {
    local pat="$1" lbl="${2:-$1}" _
    for _ in $(seq 1 60); do mc_vis | grep -qF "$pat" && return 0; sleep 0.5; done
    echo "FATAL: timed out waiting for prompt [$lbl]:"; mc_vis | sed 's/^/      /'; return 1
}

# Wait for a substring in the scrollback — for the status block / action logs.
mc_wait_scroll() {
    local pat="$1" lbl="${2:-$1}" _
    for _ in $(seq 1 60); do mc_scroll | grep -qF "$pat" && return 0; sleep 0.5; done
    echo "FATAL: timed out waiting for [$lbl]:"; mc_scroll | tail -40 | sed 's/^/      /'; return 1
}

# Select a menu item: wait until the menu is ready, clear scrollback so the
# action's fresh output is isolated, then type the filter + Enter.
mc_select() { mc_ready; mc_reset; mc_send "$1" Enter; }

# Step past the "press Enter to continue" pause and wait until the chooser is
# back up (so the next action starts from a known state).
mc_continue() { mc_send Enter; mc_ready; }

# True if window $1 exists in the session (waits up to ~30s).
harness_window_appears() {
    local w="$1" _
    for _ in $(seq 1 30); do
        tmux list-windows -t "$HARNESS_SESSION" -F '#{window_name}' | grep -qx "$w" && return 0
        sleep 1
    done
    return 1
}

# Poll a command until it succeeds (up to ~60s). $1=label, rest=command.
harness_poll() {
    local lbl="$1"; shift
    local _
    for _ in $(seq 1 120); do "$@" >/dev/null 2>&1 && return 0; sleep 0.5; done
    echo "FATAL: condition never held: $lbl"; return 1
}

# The container's current ID (empty if absent) — used to tell a recreated
# container (same name, new ID) from the one being torn down.
harness_container_id() { podman container inspect -f '{{.Id}}' "$1" 2>/dev/null || true; }
