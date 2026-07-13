#!/usr/bin/env bash
# Manually send an rc-notify event to the host listener over the endpoint
# mounted in at /run/notify (a unix socket on macOS hosts, a FIFO on Linux
# hosts). Use this to verify the host-side listener + notifier pipeline
# without waiting for claude to fire a Stop/Notification hook.
#
#   ./test_notify.sh           # defaults to 'done'
#   ./test_notify.sh done
#   ./test_notify.sh waiting

set -euo pipefail

TARGET="/run/notify"
EVENT="${1:-done}"

if [[ ! -S "$TARGET" && ! -p "$TARGET" ]]; then
    echo "error: notify endpoint not present at $TARGET" >&2
    echo "       ensure the host introdus launch started the rc-notify listener and mounted it" >&2
    exit 1
fi

rc-notify "$EVENT"
echo "sent '$EVENT' to host"
