#!/usr/bin/env bash
# Attach-or-create: a second `introdus launch` from the same project directory
# must REATTACH to the running session, never spawn a second one. Proves the
# per-session `@introdus_project_dir` tag and the dir-keyed lookup that is the
# primary key (with the name hash only a fallback) — so it also holds when the
# persisted SESSION_NAME is changed out from under it.
set -euo pipefail
source /usr/local/bin/driver-common.sh

proj="$HOME/proj-reattach"
canon="$(mkdir -p "$proj" && cd "$proj" && pwd -P)"
harness_dummy_key
# No SESSION_NAME: let introdus mint and persist one, so the dir tag — not a
# harness-supplied name — is what the reattach relies on.
harness_write_env "$proj"
harness_ensure_base "$proj"
harness_clean
cd "$proj"

# The introdus sessions currently known to tmux (one line per name).
introdus_sessions() { tmux list-sessions -F '#{session_name}' 2>/dev/null | grep '^introdus-' || true; }
session_count() { introdus_sessions | grep -c . || true; }

launch() { ( cd "$proj" && introdus launch ) >"$1" 2>&1 || true; }

echo "==> first launch: create the session"
launch "$HOME/launch1.log"
for _ in $(seq 1 30); do [[ "$(session_count)" -ge 1 ]] && break; sleep 1; done
[[ "$(session_count)" -eq 1 ]] || { echo "FATAL: expected exactly 1 session after first launch, got:"; introdus_sessions; exit 1; }
s1="$(introdus_sessions)"
echo "    session: $s1"

echo "==> asserting the session is tagged with the canonical project dir"
tag="$(tmux show-options -t "$s1" -v @introdus_project_dir 2>/dev/null || true)"
[[ "$tag" == "$canon" ]] || { echo "FATAL: @introdus_project_dir='$tag' != '$canon'"; exit 1; }
echo "    ✓ @introdus_project_dir = $tag"

echo "==> second launch from the same dir: must reattach, not spawn a new session"
launch "$HOME/launch2.log"
grep -qF "attaching to existing session $s1" "$HOME/launch2.log" \
    || { echo "FATAL: second launch did not report reattaching to $s1:"; cat "$HOME/launch2.log"; exit 1; }
[[ "$(session_count)" -eq 1 ]] || { echo "FATAL: a second session was spawned:"; introdus_sessions; exit 1; }
echo "    ✓ reattached to $s1; still exactly one session"

echo "==> changing SESSION_NAME in config: the dir tag must still win"
cfg="$(harness_config_file "$proj")"
bogus="introdus-bogus-stale-name"
sed -i '/^SESSION_NAME=/d' "$cfg"
echo "SESSION_NAME=$bogus" >> "$cfg"
launch "$HOME/launch3.log"
grep -qF "attaching to existing session $s1" "$HOME/launch3.log" \
    || { echo "FATAL: dir tag did not override the stale SESSION_NAME:"; cat "$HOME/launch3.log"; exit 1; }
tmux has-session -t "$bogus" 2>/dev/null \
    && { echo "FATAL: a session named '$bogus' was spawned — the name fallback beat the dir tag"; exit 1; }
[[ "$(session_count)" -eq 1 ]] || { echo "FATAL: extra session spawned after SESSION_NAME change:"; introdus_sessions; exit 1; }
echo "    ✓ dir tag overrode the stale name; still exactly one session ($s1)"

echo
echo "=== REATTACH OK: repeat launches from the same directory reattach to the one"
echo "    session (dir tag is primary, name hash only a fallback). ==="
