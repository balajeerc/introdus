#!/usr/bin/env bash
# Launching an agent from the control menu offers its skip-permissions / auto-
# approve mode and passes the right flag. Drives the claude path end to end:
# Launch → pick claude → accept the skip-permissions confirm → assert the agent
# window spawned AND the process was invoked with --dangerously-skip-permissions.
# Also asserts the confirm renders the highlighted Yes/No options.
# Covers TEST_PLAN: TA119, TA120
set -euo pipefail
source /usr/local/bin/driver-common.sh

session="harness-session"
proj="$HOME/proj-agent-launch"

harness_dummy_key
harness_write_env "$proj" "$session" "claude"   # claude installed -> launchable
harness_ensure_base "$proj"
harness_clean
cd "$proj"

harness_launch "$session" "$proj"
cname="$HARNESS_CNAME"

# claude installs at container setup (pnpm --allow-build); wait for it before we
# try to launch it.
harness_poll "claude installed in the container" \
    podman exec --user dev "$cname" bash -lc 'command -v claude'
echo "    ✓ claude present in the container"

# ---- Launch claude, accepting the skip-permissions offer -------------------
echo "==> menu: Launch an installed agent → claude"
mc_select "Launch an installed agent"
mc_wait_prompt "Launch which agent" "agent picker"
mc_send Enter    # single-select: claude is the only/first item

# The skip-permissions confirm appears, rendering the highlighted Yes/No options
# (the y/N highlight feature — TA120). Match an early, single-line-stable
# fragment of the question (the tail wraps across lines at 80 cols).
mc_wait_prompt "Launch Claude" "skip-permissions confirm"
vis="$(mc_vis)"
{ echo "$vis" | grep -qF "Yes" && echo "$vis" | grep -qF "No"; } \
    || { echo "FATAL: confirm did not render Yes/No options"; echo "$vis" | tail -20 | sed 's/^/      /'; exit 1; }
echo "    ✓ confirm rendered highlighted Yes/No options"

mc_send "y"      # single-key yes -> launch WITH the flag

# ---- assert the agent window + the exact flag on the launched process ------
harness_window_appears agent-claude \
    || { echo "FATAL: agent-claude window not spawned"; exit 1; }
echo "    ✓ agent-claude window spawned"

# The window runs `podman exec -it --user dev <cname> run-claude
# --dangerously-skip-permissions`; that exec persists while claude's session is
# attached. pgrep matches its cmdline (and excludes its own pid).
harness_poll "claude launched with --dangerously-skip-permissions" \
    pgrep -f 'run-claude --dangerously-skip-permissions'
echo "    ✓ launched via run-claude with --dangerously-skip-permissions"

echo
echo "=== AGENT-LAUNCH OK: the menu offered skip-permissions, rendered a"
echo "    highlighted Yes/No confirm, and accepting it launched claude with"
echo "    --dangerously-skip-permissions — all nested. ==="
