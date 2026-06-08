#!/usr/bin/env bash
# Host-level setup for the remote-control-harness. Run this ONCE per host after
# cloning the repo, before creating any dev containers. It:
#
#   1. symlinks create-dev-container.sh onto your PATH (~/.local/bin), so you
#      can run `create-dev-container.sh` from any project directory;
#   2. configures task-completion notifications — if this host forwards them to
#      another machine (a remote/headless box reporting back to your laptop),
#      it records the loopback port in the harness .env as RC_FORWARD_ADDR;
#   3. installs the rc-notify listener as a persistent systemd --user service so
#      it starts at boot and survives reboots (Linux only).
#
# A symlink (not a copy) is used so the installed command always tracks this
# harness checkout — `git pull` here updates everything with no reinstall, and
# create-dev-container.sh resolves the symlink back to find the harness scripts.
#
# Re-running is safe and idempotent.
#
# Usage:
#   ./host_install.sh
set -euo pipefail

HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="$HARNESS_DIR/create-dev-container.sh"
LISTENER="$HARNESS_DIR/host_listener.py"
HARNESS_ENV="$HARNESS_DIR/.env"
[[ -f "$SRC" ]] || { echo "error: create-dev-container.sh not found at $SRC" >&2; exit 1; }
chmod +x "$SRC"

# ---- 1. symlink create-dev-container.sh onto PATH --------------------------

BIN_DIR="$HOME/.local/bin"
DEST="$BIN_DIR/create-dev-container.sh"

mkdir -p "$BIN_DIR"
# Replace any existing install (a stale symlink or an older copy).
[[ -e "$DEST" || -L "$DEST" ]] && rm -f "$DEST"
ln -s "$SRC" "$DEST"

echo "==> Installed: $DEST -> $SRC"

# ---- 2. notification forwarding (host-level) -------------------------------
# RC_FORWARD_ADDR lives in the harness .env (read by host_notify.sh) and applies
# to every container on this host, since a single listener relays them all. On a
# laptop running containers locally, leave it unset and notifications render here.

echo
echo "  Will this host FORWARD task-completion notifications to another machine?"
echo "  (Yes for a remote/headless host reporting back to your laptop; No if you"
echo "   run containers locally and want desktop notifications on this machine.)"
read -r -p "  Forward to another machine? [y/N]: " FORWARD_REPLY
case "${FORWARD_REPLY,,}" in
    y|yes|true|1)
        read -r -p "  Loopback port for the tunnel [8765]: " RC_FORWARD_PORT
        RC_FORWARD_PORT="${RC_FORWARD_PORT:-8765}"
        if [[ ! "$RC_FORWARD_PORT" =~ ^[0-9]+$ ]]; then
            echo "  error: port must be numeric, got '$RC_FORWARD_PORT'" >&2
            exit 1
        fi
        RC_FORWARD_ADDR="127.0.0.1:${RC_FORWARD_PORT}"
        if [[ -f "$HARNESS_ENV" ]] && grep -q '^[[:space:]]*RC_FORWARD_ADDR=' "$HARNESS_ENV"; then
            # Replace the existing value in place rather than appending a dup.
            tmp="$(mktemp)"
            grep -v '^[[:space:]]*RC_FORWARD_ADDR=' "$HARNESS_ENV" > "$tmp"
            mv "$tmp" "$HARNESS_ENV"
        fi
        {
            [[ -s "$HARNESS_ENV" ]] && echo ""
            echo "# Forward task-completion notifications to a laptop listener over an"
            echo "# SSH reverse tunnel. Host-level (applies to every container here)."
            echo "# Added by host_install.sh."
            echo "RC_FORWARD_ADDR=$RC_FORWARD_ADDR"
        } >> "$HARNESS_ENV"
        chmod 600 "$HARNESS_ENV" 2>/dev/null || true
        echo "==> Set RC_FORWARD_ADDR=$RC_FORWARD_ADDR in $HARNESS_ENV"
        echo
        echo "    On your LAPTOP, run (once) from the harness checkout:"
        echo "        ./install_dev_machine_listener.sh <ssh-alias-for-this-host> ${RC_FORWARD_PORT}"
        ;;
    *)
        # Clear any forwarding left over from a previous run, so switching a host
        # back to local actually stops forwarding.
        if [[ -f "$HARNESS_ENV" ]] && grep -q '^[[:space:]]*RC_FORWARD_ADDR=' "$HARNESS_ENV"; then
            tmp="$(mktemp)"
            grep -v '^[[:space:]]*RC_FORWARD_ADDR=' "$HARNESS_ENV" > "$tmp"
            mv "$tmp" "$HARNESS_ENV"
            chmod 600 "$HARNESS_ENV" 2>/dev/null || true
            echo "==> Removed prior RC_FORWARD_ADDR from $HARNESS_ENV"
        fi
        echo "==> Notifications will render locally on this host (no forwarding)."
        ;;
esac

# ---- 3. persistent rc-notify listener service ------------------------------
# host_notify.sh (which the listener spawns per event) is what reads .env and
# decides whether to render locally or forward, so this single service covers
# both cases. We install it persistently so it survives reboot; launch.sh keeps
# its own "ensure running" check as a safety net and a transient fallback for
# hosts where this installer was never run.

if [[ "$(uname -s)" == "Darwin" ]]; then
    echo
    echo "==> macOS detected: no systemd --user. The listener is managed by"
    echo "    launch.sh (background process) on each launch — nothing to install."
elif ! command -v systemctl >/dev/null 2>&1; then
    echo
    echo "==> systemctl not found: skipping the persistent listener service."
    echo "    launch.sh will start a transient listener on each launch instead."
elif [[ ! -f "$LISTENER" ]]; then
    echo
    echo "==> warn: host_listener.py not found at $LISTENER; skipping the service."
else
    PYTHON="$(command -v python3)" || { echo "error: python3 not found" >&2; exit 1; }
    UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
    UNIT="$UNIT_DIR/rc-notify.service"
    mkdir -p "$UNIT_DIR"
    cat > "$UNIT" <<EOF
[Unit]
Description=remote-control-harness notification listener
After=default.target

[Service]
Type=simple
ExecStart=${PYTHON} ${LISTENER}
Restart=always
RestartSec=2

[Install]
WantedBy=default.target
EOF
    systemctl --user daemon-reload
    # If launch.sh already started a transient unit of this name, stop it so the
    # installed (persistent) unit takes over cleanly.
    systemctl --user reset-failed rc-notify.service 2>/dev/null || true
    systemctl --user stop rc-notify.service 2>/dev/null || true
    systemctl --user enable --now rc-notify.service
    echo
    echo "==> Installed and started rc-notify.service ($UNIT)"

    if ! loginctl show-user "$USER" 2>/dev/null | grep -q '^Linger=yes'; then
        if loginctl enable-linger "$USER" 2>/dev/null; then
            echo "==> Enabled linger for $USER (listener starts at boot)"
        else
            echo "note: could not enable linger automatically. For start-at-boot, run:"
            echo "        sudo loginctl enable-linger $USER"
        fi
    fi
fi

# ---- PATH guidance ---------------------------------------------------------

echo
case ":$PATH:" in
    *":$BIN_DIR:"*)
        echo "$BIN_DIR is on your PATH — you're all set. Create a project:"
        echo
        echo "    mkdir ~/myproject && cd ~/myproject && create-dev-container.sh"
        ;;
    *)
        echo "NOTE: $BIN_DIR is not on your PATH yet. Append to your ~/.bashrc:"
        echo
        echo '    export PATH="$HOME/.local/bin:$PATH"'
        echo
        echo "Reload your shell (source ~/.bashrc), then:"
        echo
        echo "    mkdir ~/myproject && cd ~/myproject && create-dev-container.sh"
        ;;
esac
