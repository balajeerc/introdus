#!/usr/bin/env bash
# The headless CLI surface end to end: boot ONE real dev container, then drive
# the control-panel utilities through their `introdus <subcommand>` forms and
# assert their real effects (config + container + notify service). The panel's
# equivalents are covered by driver-menu.sh; this proves the same actions work
# from the shell, non-interactively, with prompts replaced by flags.
#
# Deliberately NOT re-tested here (identical in-container mechanism already
# harness-covered): `install-agent`/`install-paseo` (the pnpm install path ==
# driver-install.sh / driver-agents.sh), `paseo-url`'s daemon start (==
# driver-paseo.sh), and `expose-webapp`'s tunnel (== the webapp tunnel path).
# The config-only edits (allow/ntfy/expose flag flips + arg validation) are
# proven hermetically by the `cli_actions_it.rs` integration test (TA156).
# Covers TEST_PLAN: TA157
set -euo pipefail
source /usr/local/bin/driver-common.sh

session="harness-cli-session"
proj="$HOME/proj-cli"

harness_dummy_key
harness_write_env "$proj" "$session"
harness_ensure_base "$proj"
harness_clean
cd "$proj"

harness_launch "$session" "$proj"
cname="$HARNESS_CNAME"
running() { podman container inspect -f '{{.State.Running}}' "$cname" 2>/dev/null | grep -qx "$1"; }

# `introdus <sub>` in the project dir. Container-touching subcommands resolve the
# same container `introdus launch` just brought up.
cli() { ( cd "$proj" && introdus "$@" ); }

# ---- blocked-egress: runs egress-log inside the container ------------------
echo "==> cli: blocked-egress"
cli blocked-egress >"$HOME/cli-blocked.log" 2>&1 \
    || { echo "FATAL: 'introdus blocked-egress' exited non-zero"; cat "$HOME/cli-blocked.log"; exit 1; }
echo "    ✓ blocked-egress ran against the container"

# ---- notify-log: surfaces the detached notify-host log --------------------
echo "==> cli: notify-log shows the notify-host log"
harness_poll "notify-host running detached" pgrep -f 'notify-host'
harness_poll "notify-log shows startup line" bash -c \
    "cd '$proj' && introdus notify-log 2>&1 | grep -qF 'reading FIFO'"
echo "    ✓ notify-log printed the notify-host startup line"

# ---- restart-notify: SIGTERM + respawn the detached notify-host -----------
echo "==> cli: restart-notify respawns notify-host with a new pid"
OLD_NOTIFY_PID="$(pgrep -f 'notify-host' | head -1)"
notify_pid_changed() {
    local new; new="$(pgrep -f 'notify-host' | head -1)"
    [[ -n "$new" && "$new" != "$OLD_NOTIFY_PID" ]]
}
cli restart-notify >/dev/null 2>&1
harness_poll "notify-host respawned with a new pid" notify_pid_changed
echo "    ✓ restart-notify gave notify-host a new pid (was $OLD_NOTIFY_PID)"

# ---- test-notify: fire a 'done' event from inside the container -----------
echo "==> cli: test-notify"
cli test-notify >/dev/null 2>&1 \
    || { echo "FATAL: 'introdus test-notify' exited non-zero"; exit 1; }
echo "    ✓ test-notify fired without error"

# ---- allow --restart: persist a host + regen allowlist + restart proxy ----
echo "==> cli: allow example.org --restart"
cli allow example.org --restart >"$HOME/cli-allow.log" 2>&1 \
    || { echo "FATAL: 'introdus allow --restart' exited non-zero"; cat "$HOME/cli-allow.log"; exit 1; }
harness_poll "example.org persisted to config" \
    grep -q "example.org" "$(harness_config_file "$proj")"
harness_poll "container running after --restart" running true
# The regenerated allowlist is bind-mounted read-only at the tinyproxy filter
# path; the restarted proxy re-reads it. Each host renders as the anchored
# regex `(^|\.)<escaped>$`, so match the escaped form `example\.org`.
harness_poll "example.org in the in-container tinyproxy allowlist" \
    podman exec "$cname" grep -qF 'example\.org' /etc/tinyproxy/egress-allowlist.txt
echo "    ✓ allow persisted the host + restarted so the proxy re-read the allowlist"

# ---- dev-shell / root-shell: foreground exec into the container -----------
# The CLI shells `exec` `podman exec -it … bash`, which needs a tty, so drive
# each in its own tmux window (a pty) and assert the resulting uid.
echo "==> cli: dev-shell execs as dev (uid 1000)"
tmux new-window -t "$session" -n cli-dev "cd '$proj' && introdus dev-shell"
harness_window_appears cli-dev || { echo "FATAL: cli-dev window not spawned"; exit 1; }
tmux send-keys -t "$session:cli-dev" "id" Enter
harness_poll "dev-shell is dev" bash -c \
    "tmux capture-pane -t '$session:cli-dev' -p | grep -q 'uid=1000(dev)'"
echo "    ✓ dev-shell dropped into uid=1000(dev) in the container"

echo "==> cli: root-shell execs as root (uid 0)"
tmux new-window -t "$session" -n cli-root "cd '$proj' && introdus root-shell"
harness_window_appears cli-root || { echo "FATAL: cli-root window not spawned"; exit 1; }
tmux send-keys -t "$session:cli-root" "id" Enter
harness_poll "root-shell is root" bash -c \
    "tmux capture-pane -t '$session:cli-root' -p | grep -q 'uid=0(root)'"
echo "    ✓ root-shell dropped into uid=0(root) in the container"

# ---- agent: rejects an id not in INSTALL_AGENTS (fast, deterministic) ------
echo "==> cli: agent <not-selected> is refused"
if cli agent definitely-not-selected >"$HOME/cli-agent.log" 2>&1; then
    echo "FATAL: 'introdus agent <bad>' should have failed"; cat "$HOME/cli-agent.log"; exit 1
fi
grep -qF "INSTALL_AGENTS" "$HOME/cli-agent.log" \
    || { echo "FATAL: unexpected agent error:"; cat "$HOME/cli-agent.log"; exit 1; }
echo "    ✓ launching an unselected agent is refused with a clear error"

# ---- restart: in-place bounce, container stays up -------------------------
echo "==> cli: restart"
cli restart >/dev/null 2>&1 || { echo "FATAL: 'introdus restart' exited non-zero"; exit 1; }
harness_poll "container running after restart" running true
echo "    ✓ restart kept the container running"

# ---- stop: refuses without --yes, stops with it ---------------------------
echo "==> cli: stop requires --yes"
if cli stop >"$HOME/cli-stop.log" 2>&1; then
    echo "FATAL: 'introdus stop' without --yes should have failed"; exit 1
fi
grep -qF -- "--yes" "$HOME/cli-stop.log" \
    || { echo "FATAL: stop error didn't mention --yes:"; cat "$HOME/cli-stop.log"; exit 1; }
echo "    ✓ stop without --yes refused"

echo "==> cli: stop --yes stops the container"
cli stop --yes >/dev/null 2>&1 || { echo "FATAL: 'introdus stop --yes' exited non-zero"; exit 1; }
harness_poll "container stopped" running false
echo "    ✓ stop --yes brought the container down"

echo
echo "=== CLI HARNESS OK: headless subcommands drive a real container — blocked-"
echo "    egress, notify-log/restart-notify/test-notify, allow --restart (persist"
echo "    + proxy re-read), dev/root shells (uid), agent refusal, restart, and"
echo "    stop (--yes gated) — nested. ==="
