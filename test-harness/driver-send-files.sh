#!/usr/bin/env bash
# The full `introdus send-files` experience against a REAL running container,
# driven over tmux: pick "this machine (local)" -> pick the container -> in the
# dual-pane browser pick a host file (left) -> send it to the right pane's
# directory -> assert it landed in the container, dev-owned. Covers the local
# transfer path end to end (the remote/ssh path is covered by unit tests —
# there's no second ssh host in the nested harness).
# Covers TEST_PLAN: TA149 (harness send-files)
set -euo pipefail
source /usr/local/bin/driver-common.sh

session="harness-session"
proj="$HOME/proj-sendfiles"

harness_dummy_key
harness_write_env "$proj" "$session"
harness_ensure_base "$proj"
harness_clean
cd "$proj"

harness_launch "$session" "$proj"
cname="$HARNESS_CNAME"

# A dedicated source dir so left-pane navigation is deterministic: the only
# entries are `..` then `payload.txt`, so one Down lands on the file.
outbox="$HOME/outbox"
rm -rf "$outbox"; mkdir -p "$outbox"
echo "introdus-send-files-payload" > "$outbox/payload.txt"

# send-files is its own full-screen app (not a menu action): run it in a new
# tmux window, started IN the source dir so the left pane opens there.
sf_pane="$session:send-files"
tmux new-window -t "$session" -n send-files -c "$outbox" "introdus send-files"
tmux set-option -t "$session" window-size manual 2>/dev/null || true
tmux resize-window -t "$session" -x 80 -y 50 2>/dev/null || true

sf_vis() { tmux capture-pane -t "$sf_pane" -p; }
sf_send() { tmux send-keys -t "$sf_pane" "$@"; }
sf_wait() {
    local pat="$1" lbl="${2:-$1}" _
    for _ in $(seq 1 80); do sf_vis | grep -qF "$pat" && return 0; sleep 0.5; done
    echo "FATAL: timed out waiting for [$lbl]:"; sf_vis | sed 's/^/      /'; return 1
}

# ---- stage 1: host picker (only "this machine (local)" — no ~/.ssh/config) ---
sf_wait "Send files" "host picker"
echo "    ✓ host picker rendered"
sf_send Enter

# ---- stage 2: container picker (the one running harness container) ----------
sf_wait "$cname" "container picker"
echo "    ✓ container picker lists the running container: $cname"
sf_send Enter

# ---- stage 3: dual-pane browser --------------------------------------------
sf_wait "CONTAINER" "dual-pane browser"
sf_wait "payload.txt" "left pane shows the source file"
echo "    ✓ dual-pane browser up; left pane shows payload.txt"

# Left pane is active; cursor starts on `..`. Down -> payload.txt, Space picks
# it as the source, then `s` sends it into the right pane's dir (/home/dev).
sf_send Down
sf_send Space
sf_wait "picked payload.txt" "source picked"
echo "    ✓ picked payload.txt as the source"
sf_send s

harness_poll "payload delivered into the container" bash -c \
    "podman exec --user dev '$cname' cat /home/dev/payload.txt 2>/dev/null | grep -q introdus-send-files-payload"
echo "    ✓ file sent into the container at /home/dev/payload.txt"

# The delivered file must be dev-owned (the transfer chowns it after copy).
harness_poll "delivered file is dev-owned" bash -c \
    "podman exec '$cname' stat -c '%U' /home/dev/payload.txt 2>/dev/null | grep -qx dev"
echo "    ✓ delivered file is owned by dev"

# And the browser surfaces the success in-pane.
sf_wait "sent payload.txt" "in-pane success status"
echo "    ✓ browser shows the send succeeded"

echo
echo "=== SEND-FILES OK: real launch + send-files local flow end to end:"
echo "    host picker -> container picker -> dual-pane browser -> pick + send,"
echo "    delivered dev-owned into the container. ==="
