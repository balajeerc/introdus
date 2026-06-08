#!/usr/bin/env bash
# install_dev_machine_listener.sh ALIAS [PORT]
#
# Run this ONCE on your laptop to receive task-completion notifications from dev
# containers running on a remote host, surviving reboot and sleep. It installs
# two systemd --user services:
#
#   rc-notify-listener.service  -- host_listener.py in TCP mode; renders the
#                                  native desktop popup + notification_sound.wav
#                                  for events arriving on 127.0.0.1:PORT.
#   rc-notify-tunnel.service    -- an SSH reverse tunnel (ssh -R) to ALIAS that
#                                  maps the remote host's 127.0.0.1:PORT to this
#                                  laptop's listener. Requires autossh, which
#                                  with systemd self-heals across drops/suspend.
#
# ALIAS is an entry in your ~/.ssh/config for the remote host (the same alias
# you already `ssh` into). PORT must match RC_FORWARD_ADDR=127.0.0.1:PORT in the
# harness .env on that remote host (default 8765).
#
# Topology (two hops):
#   [dev container] --FIFO--> [remote host] host_notify.sh
#       --(TCP 127.0.0.1:PORT)--> (this reverse tunnel)
#           --> [laptop] host_listener.py --> popup + sound
#
# Usage:
#   ./install_dev_machine_listener.sh my-kvm-box
#   ./install_dev_machine_listener.sh my-kvm-box 8765
#
# Manage afterwards:
#   systemctl --user status  rc-notify-listener.service rc-notify-tunnel.service
#   systemctl --user restart rc-notify-tunnel.service
#   journalctl --user -u rc-notify-tunnel.service -f
#   ./install_dev_machine_listener.sh --uninstall
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LISTENER="$SCRIPT_DIR/host_listener.py"
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
LISTENER_UNIT="rc-notify-listener.service"
TUNNEL_UNIT="rc-notify-tunnel.service"

err() { echo "error: $*" >&2; exit 1; }

command -v systemctl >/dev/null 2>&1 \
    || err "systemctl not found — this installer targets Linux laptops with systemd (use ./laptop_notify_tunnel.sh for a foreground tunnel instead)"

# ---- uninstall -------------------------------------------------------------
if [[ "${1:-}" == "--uninstall" ]]; then
    systemctl --user disable --now "$TUNNEL_UNIT" 2>/dev/null || true
    systemctl --user disable --now "$LISTENER_UNIT" 2>/dev/null || true
    rm -f "$UNIT_DIR/$TUNNEL_UNIT" "$UNIT_DIR/$LISTENER_UNIT"
    systemctl --user daemon-reload
    echo "==> removed $LISTENER_UNIT and $TUNNEL_UNIT"
    exit 0
fi

ALIAS="${1:-}"
PORT="${2:-8765}"
[[ -n "$ALIAS" ]] || err "usage: install_dev_machine_listener.sh <ssh-alias> [port]  (or --uninstall)"
[[ "$PORT" =~ ^[0-9]+$ ]] || err "port must be numeric, got '$PORT'"
[[ -f "$LISTENER" ]] || err "host_listener.py not found at $LISTENER"

PYTHON="$(command -v python3)" || err "python3 not found"

# Require autossh: combined with systemd's Restart it self-heals across dropped
# connections, suspend/resume, and network changes far more reliably than plain
# ssh. Error out with an install hint rather than silently degrading.
if ! command -v autossh >/dev/null 2>&1; then
    cat >&2 <<'MSG'
error: autossh is not installed.

The reverse tunnel uses autossh so it reconnects cleanly across network
changes, suspend/resume, and dropped connections. Install it, then re-run:

  Debian/Ubuntu:  sudo apt install autossh
  Fedora:         sudo dnf install autossh
  Arch:           sudo pacman -S autossh
  macOS:          brew install autossh
MSG
    exit 1
fi
SSH_CMD="$(command -v autossh) -M 0"

# BatchMode=yes so a missing/passphrase-locked key fails fast under systemd
# instead of hanging on a prompt. ExitOnForwardFailure surfaces a stale remote
# port binding as a restart instead of a silently-dead forward.
SSH_OPTS="-N -o BatchMode=yes -o ExitOnForwardFailure=yes -o ServerAliveInterval=30 -o ServerAliveCountMax=3"
FORWARD="-R 127.0.0.1:${PORT}:127.0.0.1:${PORT}"

mkdir -p "$UNIT_DIR"

cat > "$UNIT_DIR/$LISTENER_UNIT" <<EOF
[Unit]
Description=remote-control-harness notification listener (renders remote dev-container alerts)
After=graphical-session.target

[Service]
Type=simple
Environment=RC_LISTEN_TCP=127.0.0.1:${PORT}
ExecStart=${PYTHON} ${LISTENER}
Restart=always
RestartSec=2

[Install]
WantedBy=default.target
EOF

cat > "$UNIT_DIR/$TUNNEL_UNIT" <<EOF
[Unit]
Description=remote-control-harness reverse tunnel to ${ALIAS} (notifications back to this laptop)
Requires=${LISTENER_UNIT}
After=${LISTENER_UNIT} network-online.target
Wants=network-online.target

[Service]
Type=simple
# AUTOSSH_GATETIME=0: keep retrying even if the very first connection dies
# quickly (otherwise autossh gives up when ssh exits within its gate time).
Environment=AUTOSSH_GATETIME=0
ExecStart=${SSH_CMD} ${SSH_OPTS} ${FORWARD} ${ALIAS}
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
EOF

systemctl --user daemon-reload
systemctl --user enable --now "$LISTENER_UNIT"
systemctl --user enable --now "$TUNNEL_UNIT"

# Linger lets the user manager (and these services) run at boot without an
# active login session, so notifications work even before you log in.
if ! loginctl show-user "$USER" 2>/dev/null | grep -q '^Linger=yes'; then
    if loginctl enable-linger "$USER" 2>/dev/null; then
        echo "==> enabled linger for $USER (services start at boot)"
    else
        echo "note: could not enable linger automatically. For start-at-boot, run:"
        echo "        sudo loginctl enable-linger $USER"
    fi
fi

cat <<EOF

============================================================
  Installed laptop-side notification services.
============================================================
  listener:  $UNIT_DIR/$LISTENER_UNIT   (127.0.0.1:${PORT})
  tunnel:    $UNIT_DIR/$TUNNEL_UNIT      (ssh -R to '${ALIAS}')

  On the REMOTE host, set in the harness .env:
      RC_FORWARD_ADDR=127.0.0.1:${PORT}

  Verify:
      systemctl --user status $LISTENER_UNIT $TUNNEL_UNIT
      journalctl --user -u $TUNNEL_UNIT -f

  Note: the tunnel uses BatchMode (no password prompts). The '${ALIAS}'
  host must accept key-based SSH with a passphrase-less key (or an agent
  reachable from your systemd --user session).

  Uninstall:  ./install_dev_machine_listener.sh --uninstall
============================================================
EOF
