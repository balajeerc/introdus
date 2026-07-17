#!/usr/bin/env bash
# claude is a normal, opt-out-able agent — NOT baked into the base image. This
# guards the regression where claude was installed regardless of the wizard
# selection: launch with NOTHING selected (INSTALL_AGENTS="") and assert claude
# is genuinely absent, then install it through the control menu and assert it
# appears — proving the opt-out AND the opt-in (pnpm --allow-build) paths.
# Covers TEST_PLAN: TA117, TA118
set -euo pipefail
source /usr/local/bin/driver-common.sh

session="harness-session"
proj="$HOME/proj-agents"

harness_dummy_key
harness_write_env "$proj" "$session" ""    # INSTALL_AGENTS="" — no agents at all
harness_ensure_base "$proj"
harness_clean
cd "$proj"

harness_launch "$session" "$proj"
cname="$HARNESS_CNAME"

# ---- claude must be ABSENT when unselected (the regression guard) ----------
# Nothing installs claude, so this is safe to assert as soon as the container is
# up: no launch-phase step could make it appear. A login shell resolves the same
# PATH (mise/pnpm bins) a user would get, so this is the real "can I run it" test.
echo "==> asserting claude is not installed (it was unselected in .env)"
if podman exec --user dev "$cname" bash -lc 'command -v claude' >/dev/null 2>&1; then
    echo "FATAL: claude present despite INSTALL_AGENTS=\"\" — it was forced/prebaked in"
    exit 1
fi
echo "    ✓ claude absent when not selected"

# ---- install claude through the menu, assert it appears (opt-in works) ------
# With nothing selected, every agent is a candidate and claude is first in the
# picker, so a single Space toggles it.
echo "==> menu: Install a coding agent → claude"
mc_select "Install a coding agent"
mc_wait_prompt "Install which agents" "install picker"
mc_send Space   # claude is the first candidate when nothing is selected
mc_send Enter   # confirm the selection
# Saving the (unchanged) allowlist still offers a restart; decline (default No).
mc_wait_prompt "Restart the container to apply" "restart offer"
mc_send Enter
# The install streams with a live spinner while pnpm --allow-build runs.
mc_wait_prompt "working: install-agents" "install progress spinner"
echo "    ✓ install started with a live progress spinner"

harness_poll "claude installed via menu" \
    podman exec --user dev "$cname" bash -lc 'command -v claude'
echo "    ✓ claude installed on demand via pnpm --allow-build (native binary placed)"

echo
echo "=== AGENTS OK: claude is opt-out (absent when unselected) and opt-in"
echo "    (installable on demand through the menu) — all nested. ==="
