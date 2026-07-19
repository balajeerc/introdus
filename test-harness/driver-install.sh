#!/usr/bin/env bash
# Install a coding agent through the control panel — the flow that must show a
# live progress indicator (a long, chatty install) AND disable the menu while it
# runs, so a keystroke mashed during the install can't cascade into another
# action. Regression guard for both.
# Covers TEST_PLAN: TA115, TA116
set -euo pipefail
source /usr/local/bin/driver-common.sh

session="harness-session"
proj="$HOME/proj-install"

harness_dummy_key
harness_write_env "$proj" "$session"
harness_ensure_base "$proj"
harness_clean
cd "$proj"

harness_launch "$session" "$proj"
cname="$HARNESS_CNAME"
running() { podman container inspect -f '{{.State.Running}}' "$cname" 2>/dev/null | grep -qx true; }

# This project selected only claude (INSTALL_AGENTS="claude"), so codex is not
# installed and starts absent.
if podman exec --user dev "$cname" test -e /home/dev/.local/share/pnpm/bin/codex; then
    echo "FATAL: codex already present before the test"; exit 1
fi

# ---- pick codex in the install checklist -----------------------------------
echo "==> menu: Install a coding agent → codex"
mc_select "Install a coding agent"
mc_wait_prompt "Install which agents" "install picker"
mc_send Space   # toggle the first candidate (codex is first: claude already selected)
mc_send Enter   # confirm the selection
# Saving the new allowlist offers a restart to apply it; decline (default No).
mc_wait_prompt "Restart the container to apply" "restart offer"
mc_send Enter

# ---- the install shows live progress ---------------------------------------
echo "==> a live progress spinner appears while install-agents runs"
mc_wait_prompt "working: install-agents" "install progress spinner"
echo "    ✓ progress spinner shown (menu marked paused)"

# ---- a stray action during the install must be ignored ---------------------
# Mash the Destroy/Reset hotkey (d) while the install task owns the UI — a single
# key that would pop a confirm the moment it fired. Keystrokes are drained, so
# nothing happens.
echo "==> mashing the stray Destroy/Reset hotkey (d) during the install"
mc_hotkey "d"
mc_hotkey "d"

# ---- wait for the install to finish, then prove nothing cascaded -----------
echo "==> waiting for install-agents to finish"
mc_wait_gone "working: install-agents" "install task end"

echo "==> no stray Destroy/Reset confirm leaked, and the container still runs"
mc_vis | grep -qF "Destroy/Reset this container" \
    && { echo "FATAL: a stray hotkey leaked a Destroy/Reset confirm — keys weren't drained"; exit 1; }
running || { echo "FATAL: container stopped — a stray action cascaded through!"; exit 1; }
echo "    ✓ menu was disabled during the task; no cascade"

echo "==> codex was actually installed (the task ran to completion)"
harness_poll "codex installed" \
    bash -c "podman exec --user dev '$cname' bash -lc 'pnpm ls -g 2>/dev/null | grep -Fq @openai/codex'"
echo "    ✓ codex present in the container"

# ---- a script-method agent: egress hosts must cover the vendor installer ----
# antigravity is installed by a vendor script that downloads its CLI tarball
# from storage.googleapis.com. If that host is missing from the agent's egress
# allowlist the download 403s and the install silently fails — this guards it.
# Restart to apply (Yes) so the newly-added hosts are live before the install.
echo "==> menu: Install antigravity (script-method — needs its download host)"
mc_select "Install a coding agent"
mc_wait_prompt "Install which agents" "install picker"
mc_send Space   # antigravity is the first candidate now (codex already installed)
mc_send Enter
mc_wait_prompt "Restart the container to apply" "restart offer"
mc_send "y"   # YES (single key) — apply the new allowlist before installing
mc_wait_prompt "working: install-agents" "antigravity install progress"
mc_wait_gone "working: install-agents" "antigravity install end"

echo "==> agy (antigravity) was actually installed"
harness_poll "agy installed" \
    bash -c "podman exec --user dev '$cname' bash -lc 'command -v agy >/dev/null'"
echo "    ✓ antigravity present — its egress hosts covered the vendor download"

echo
echo "=== INSTALL OK: the install streamed live progress, the menu was disabled"
echo "    for the duration (a mashed Stop was ignored — no cascade), codex (npm)"
echo "    and antigravity (vendor script) both installed — its download host is"
echo "    allowlisted — all nested. ==="
