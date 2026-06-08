#!/usr/bin/env bash
# Host-side notifier: plays notification_sound.wav and shows a desktop
# notification. Invoked by the host-side listener when the container signals
# a notification event via the UDS.
#
# Supports Linux (libnotify + PulseAudio/PipeWire/ALSA) and macOS
# (osascript + afplay). On macOS, osascript's display notification does not
# require any extra packages and respects Do Not Disturb settings.
#
# Arg 1 is an event-type keyword that maps to a preset body; anything
# else is rejected. Arg 2 optionally overrides the title.
#
# Standalone test:
#   ./host_notify.sh                         # "Task complete"
#   ./host_notify.sh done                    # "Task complete"
#   ./host_notify.sh waiting                 # "Awaiting your input"

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOUND_FILE="${SOUND_FILE:-$SCRIPT_DIR/notification_sound.wav}"

EVENT="${1:-done}"
# Arg 2 is an optional label identifying which container fired the event. It is
# the only attacker-influenceable text reaching the notification, so sanitize it
# to a safe charset and cap its length even though the listener already did —
# host_notify.sh can also be invoked directly. Never let it carry control chars
# or spoof additional text under the "Claude Code" brand.
LABEL="${2:-}"
LABEL="${LABEL//[^A-Za-z0-9._-]/}"
LABEL="${LABEL:0:40}"

TITLE="Claude Code"
[[ -n "$LABEL" ]] && TITLE="Claude Code — $LABEL"

# Whitelist of valid events. Refuse to display arbitrary body text because
# the notification appears under the trusted "Claude Code" brand —
# a compromised process in the container could otherwise spoof prompts.
# The upstream listener daemon also rejects unknown events; this is
# cheap defence-in-depth.
case "$EVENT" in
    done)       BODY="Task complete" ;;
    waiting)    BODY="Awaiting your input" ;;
    *)          echo "host_notify: unknown event '$EVENT'" >&2; exit 2 ;;
esac

# Pull ntfy.sh config from the harness .env on every invocation rather than
# relying on the long-running listener daemon's environment, so edits to
# .env take effect without a listener restart. We sandbox the source in a
# subshell so unrelated variables in .env do not leak into this script.
# An explicit RC_FORWARD_ADDR in the environment always wins over .env. We
# capture it before reading .env so a laptop-side listener (which shares this
# same checkout) can force-disable forwarding via RC_NO_FORWARD regardless of
# what .env says.
RC_FORWARD_ADDR_OVERRIDE="${RC_FORWARD_ADDR:-}"
ENABLE_NOTIFY_SH_ALERTS=""
NTFY_SH_TOPIC=""
RC_FORWARD_ADDR=""
if [[ -r "$SCRIPT_DIR/.env" ]]; then
    eval "$(
        set +eu
        # shellcheck disable=SC1091
        source "$SCRIPT_DIR/.env" >/dev/null 2>&1
        printf 'ENABLE_NOTIFY_SH_ALERTS=%q\nNTFY_SH_TOPIC=%q\nRC_FORWARD_ADDR=%q\n' \
            "${ENABLE_NOTIFY_SH_ALERTS:-}" "${NTFY_SH_TOPIC:-}" "${RC_FORWARD_ADDR:-}"
    )"
fi
[[ -n "$RC_FORWARD_ADDR_OVERRIDE" ]] && RC_FORWARD_ADDR="$RC_FORWARD_ADDR_OVERRIDE"

send_ntfy() {
    [[ "$ENABLE_NOTIFY_SH_ALERTS" == "true" ]] || return 0
    if [[ -z "$NTFY_SH_TOPIC" ]]; then
        echo "host_notify: ENABLE_NOTIFY_SH_ALERTS=true but NTFY_SH_TOPIC unset" >&2
        return 0
    fi
    if ! command -v curl >/dev/null 2>&1; then
        echo "host_notify: curl not found; cannot send ntfy.sh alert" >&2
        return 0
    fi
    # Fire-and-forget: don't block the local notification on network latency,
    # and don't fail the script if ntfy.sh is unreachable.
    (
        curl -fsS --max-time 5 \
            -H "Title: $TITLE" \
            -H "Tags: bell" \
            -d "$BODY" \
            "https://ntfy.sh/${NTFY_SH_TOPIC}" >/dev/null 2>&1 || true
    ) &
    disown 2>/dev/null || true
}

send_ntfy

# ---------------------------------------------------------------------------
# Headless forward (remote server with no desktop of its own)
# ---------------------------------------------------------------------------
# Two-hop remote setup:
#
#   [dev container] --FIFO--> [this remote host] host_notify.sh
#       --(TCP to RC_FORWARD_ADDR)--> (ssh -R reverse tunnel)
#           --> [laptop] host_listener.py --> desktop popup + sound
#
# When RC_FORWARD_ADDR=host:port is set, this machine is the remote server: it
# has no desktop, so instead of rendering we forward the already-validated
# event over TCP to host:port. That is normally 127.0.0.1:<PORT>, which an SSH
# reverse tunnel opened by the laptop forwards back to a host_listener.py
# running there; the laptop's listener re-validates against the same whitelist
# and renders the real notification.
#
# RC_NO_FORWARD=1 hard-disables this branch — the laptop-side listener sets it
# so that, even though it shares this checkout (and possibly an .env with
# RC_FORWARD_ADDR), it renders locally instead of bouncing the event back.
#
# bash's /dev/tcp avoids any netcat dependency; timeout caps the wait so a
# down tunnel can never wedge the Claude hook that triggered this.
if [[ -n "$RC_FORWARD_ADDR" && "${RC_NO_FORWARD:-}" != "1" ]]; then
    fwd_host="${RC_FORWARD_ADDR%:*}"
    fwd_port="${RC_FORWARD_ADDR##*:}"
    # Preserve the label across the hop using the same "event<TAB>label" wire
    # format the container used, so the laptop's listener renders it identically.
    if [[ -n "$LABEL" ]]; then
        fwd_msg="${EVENT}"$'\t'"${LABEL}"
    else
        fwd_msg="${EVENT}"
    fi
    if ! timeout 5 bash -c 'printf "%s\n" "$1" > "/dev/tcp/$2/$3"' _ \
            "$fwd_msg" "$fwd_host" "$fwd_port" 2>/dev/null; then
        echo "host_notify: forward to $RC_FORWARD_ADDR failed (tunnel down?)" >&2
    fi
    exit 0
fi

# ---------------------------------------------------------------------------
# macOS
# ---------------------------------------------------------------------------
if [[ "$(uname -s)" == "Darwin" ]]; then
    # Audio: afplay is built into macOS (no install required).
    if [[ -r "$SOUND_FILE" ]]; then
        afplay "$SOUND_FILE" &
        disown 2>/dev/null || true
    fi

    # Notification: osascript is always available on macOS. The 'display
    # notification' command routes through Notification Centre and respects
    # per-app and focus/DND settings. We do not force critical urgency here
    # because macOS manages that at the system level.
    osascript -e "display notification \"$BODY\" with title \"$TITLE\"" 2>/dev/null || {
        # Fallback if osascript is somehow unavailable.
        echo "$TITLE: $BODY" >&2
    }
    exit 0
fi

# ---------------------------------------------------------------------------
# Linux
# ---------------------------------------------------------------------------

# Play the sound in the background so the notification pops immediately.
# Try common players in order: paplay (PulseAudio/PipeWire), pw-play
# (native PipeWire), aplay (bare ALSA), ffplay (last-ditch fallback).
play_sound() {
    if [[ ! -r "$SOUND_FILE" ]]; then
        echo "notify: sound file not readable at $SOUND_FILE" >&2
        return 0
    fi
    if command -v paplay >/dev/null 2>&1; then
        paplay "$SOUND_FILE" &
    elif command -v pw-play >/dev/null 2>&1; then
        pw-play "$SOUND_FILE" &
    elif command -v aplay >/dev/null 2>&1; then
        aplay -q "$SOUND_FILE" &
    elif command -v ffplay >/dev/null 2>&1; then
        ffplay -nodisp -autoexit -loglevel quiet "$SOUND_FILE" &
    else
        echo "notify: no audio player found (install one of: paplay, pw-play, aplay, ffplay)" >&2
        return 0
    fi
    disown 2>/dev/null || true
}

# --urgency=critical + --expire-time=0 = persistent until dismissed on every
# freedesktop-compliant notification daemon (GNOME Shell, KDE Plasma, dunst,
# mako, xfce4-notifyd, ...). Critical notifications typically bypass DND.
show_notification() {
    if ! command -v notify-send >/dev/null 2>&1; then
        echo "notify: notify-send not installed (try 'apt install libnotify-bin')" >&2
        echo "$TITLE: $BODY" >&2
        return 1
    fi

    # Persist the last notification ID so a follow-up event collapses onto
    # the previous bubble instead of queuing a new one. Stale IDs (whose
    # notification has already been dismissed) are silently ignored by the
    # daemon, so the new bubble simply appears as normal.
    local id_file="${XDG_RUNTIME_DIR:-/tmp}/claude-code-notify.id"
    local prev_id=0
    [[ -r "$id_file" ]] && prev_id=$(cat "$id_file" 2>/dev/null || echo 0)

    local new_id
    new_id=$(notify-send \
        --print-id \
        --replace-id="$prev_id" \
        --urgency=critical \
        --expire-time=0 \
        --app-name="claude-code" \
        --icon=dialog-information \
        "$TITLE" \
        "$BODY")
    [[ -n "$new_id" ]] && echo "$new_id" > "$id_file"
}

play_sound
show_notification
