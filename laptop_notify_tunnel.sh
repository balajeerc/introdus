#!/usr/bin/env bash
# Laptop side of the two-hop remote notification setup.
#
# Topology:
#
#   [dev container] --FIFO--> [KVM server] host_notify.sh
#       --(TCP to 127.0.0.1:PORT)--> (this ssh -R reverse tunnel)
#           --> [laptop] host_listener.py --> desktop popup + sound
#
# The container -> server hop is the harness's existing FIFO transport and is
# untouched. This script handles the server -> laptop hop, which has to cross
# the network while your laptop sits behind NAT. Because the laptop dials the
# tunnel OUT to the server (over the same SSH you already use to reach it), no
# inbound port is needed anywhere.
#
# It does two things, both on the laptop:
#   1. starts host_listener.py listening on 127.0.0.1:PORT (renders locally
#      via host_notify.sh -- the usual popup + notification_sound.wav)
#   2. opens an SSH reverse tunnel so the server's 127.0.0.1:PORT reaches that
#      listener. autossh (if installed) keeps it alive across drops/suspends.
#
# On the KVM server, set in the harness .env:
#       RC_FORWARD_ADDR=127.0.0.1:<PORT>
# and (re)launch the container so host_notify.sh forwards instead of trying to
# render on a desktop that isn't there.
#
# Usage:
#   ./laptop_notify_tunnel.sh user@kvm-server [PORT]
#   ./laptop_notify_tunnel.sh my-ssh-alias            # PORT defaults to 8765
#
# Leave it running in a terminal (or wrap it in a systemd --user service /
# launchd agent for always-on). The PORT must match RC_FORWARD_ADDR's port.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REMOTE="${1:?usage: laptop_notify_tunnel.sh user@host|ssh-alias [port]}"
PORT="${2:-8765}"

LOG="/tmp/rc-notify-laptop.log"

# --- 1. local listener -----------------------------------------------------
# Reuse an existing listener if one is already bound to PORT; otherwise start
# one. RC_LISTEN_TCP puts host_listener.py into TCP-render mode (and makes it
# set RC_NO_FORWARD so it never bounces the event back out).
if pgrep -af "RC_LISTEN_TCP|host_listener.py" 2>/dev/null | grep -q "host_listener.py"; then
    echo "==> notification listener already running"
else
    echo "==> starting notification listener on 127.0.0.1:$PORT (log: $LOG)"
    RC_LISTEN_TCP="127.0.0.1:$PORT" nohup python3 "$SCRIPT_DIR/host_listener.py" \
        >>"$LOG" 2>&1 &
    disown
fi

# --- 2. reverse tunnel -----------------------------------------------------
# ExitOnForwardFailure makes ssh fail fast if PORT is already bound on the
# server (e.g. a stale tunnel) instead of silently dropping the forward. The
# keepalives tear a dead session down so autossh can rebuild it.
SSH_OPTS=(
    -N
    -o ExitOnForwardFailure=yes
    -o ServerAliveInterval=30
    -o ServerAliveCountInterval=3
    -R "127.0.0.1:${PORT}:127.0.0.1:${PORT}"
)

echo "==> reverse tunnel: $REMOTE 127.0.0.1:$PORT -> laptop 127.0.0.1:$PORT"
if command -v autossh >/dev/null 2>&1; then
    exec autossh -M 0 "${SSH_OPTS[@]}" "$REMOTE"
else
    echo "    (install 'autossh' for automatic reconnects; using plain ssh)"
    exec ssh "${SSH_OPTS[@]}" "$REMOTE"
fi
