#!/usr/bin/env bash
# PID 1 of the dev container. Runs as ROOT with CAP_NET_ADMIN. It:
#   1. stages the deploy key into the dev user's ~/.ssh (while still root),
#   2. installs an in-container nft egress filter (default-deny, segregated by
#      uid: only the proxy user may reach the internet),
#   3. starts the hostname-allowlist forward proxy (tinyproxy) as that uid,
#   4. runs an egress self-check, then
#   5. drops ALL privilege and exec()s /setup.sh as the non-root `dev` user.
#
# Why this shape: once we exec as `dev` (non-root, no caps, no-new-privileges),
# the workload can never regain CAP_NET_ADMIN, so it cannot touch nft; and the
# nft policy drops every direct egress from `dev` except loopback/DNS/internal.
# The ONLY way out for the workload is the loopback proxy, which gates by
# hostname. Knowing a CDN's IP and dialing it directly does not help — direct
# egress is dropped regardless of destination.
set -euo pipefail

PROXY_PORT="${PROXY_PORT:-8888}"
PROXY_USER="rcproxy"
WORK_USER="dev"
WORK_HOME="/home/${WORK_USER}"
DISABLE_NETWORK_BLOCK="${DISABLE_NETWORK_BLOCK:-false}"
CANARY_BLOCKED_IP="${CANARY_BLOCKED_IP:-93.184.216.34}"

log()  { printf '\n==> [egress] %s\n' "$*"; }
warn() { printf '    [egress] warning: %s\n' "$*" >&2; }

# Run a command as the unprivileged dev user. reuid to a non-root user clears
# the permitted/effective cap sets, and the container's no-new-privileges flag
# stops dev from ever regaining them, so dev is fully unprivileged without
# touching the bounding set (editing which would need CAP_SETPCAP that we
# deliberately don't grant). setpriv does not reset the environment, so set
# HOME/USER explicitly or dev's tools (mise, git, claude) look in /root.
run_as_dev() {
    setpriv --reuid "$WORK_USER" --regid "$WORK_USER" --init-groups -- \
        env HOME="$WORK_HOME" USER="$WORK_USER" LOGNAME="$WORK_USER" "$@"
}

# Orchestrate the launch in phases so optional root-owned setup can run AFTER the
# repo is cloned (so it can use repo files) but BEFORE the dev workload starts:
#   1. setup.sh prepare  — clone + --pull, as dev (repo ends up dev-owned)
#   2. ON_LAUNCH_ROOT_SCRIPT — as root, from the repo dir (e.g. start a DB server)
#   3. setup.sh serve    — tunnel + ON_LAUNCH_SCRIPT (dev) + banner + idle
run_workload() {
    log "phase 1/3: clone + repo prep (as '$WORK_USER')"
    run_as_dev /bin/bash /setup.sh prepare

    if [[ -n "${ON_LAUNCH_ROOT_SCRIPT:-}" ]]; then
        local wd="${WORK_HOME}/work/${PROJECT_NAME:-}"
        cd "$wd" 2>/dev/null || cd "$WORK_HOME"
        local t="${ON_LAUNCH_ROOT_TIMEOUT:-600}"
        log "phase 2/3: running ON_LAUNCH_ROOT_SCRIPT as root (cwd=$(pwd), timeout ${t}s)"
        # Never let a failing OR hanging root script block startup: a non-zero
        # exit is logged and we continue; a script that doesn't return is killed
        # by `timeout` (124) and we continue. Either way the dev workload starts.
        # (Long-running services should be backgrounded/daemonized so the script
        # returns promptly; the timeout is only a safety net.)
        local rc=0
        timeout "$t" bash -c "$ON_LAUNCH_ROOT_SCRIPT" || rc=$?
        if [[ $rc -eq 124 ]]; then
            warn "ON_LAUNCH_ROOT_SCRIPT timed out after ${t}s — continuing launch without it"
        elif [[ $rc -ne 0 ]]; then
            warn "ON_LAUNCH_ROOT_SCRIPT exited non-zero ($rc) — continuing launch"
        fi
    fi

    log "phase 3/3: starting workload (as '$WORK_USER')"
    exec run_as_dev /bin/bash /setup.sh serve
}

# ---- 0a. ensure the persistent volume is owned by dev ----------------------
# A volume freshly seeded from the image is already dev-owned. But a volume
# carried over from the old /root-based layout (same project-only volume name)
# is root-owned, leaving dev unable to write. The cheap ownership check means the
# recursive chown runs only once, on the first launch after migrating.
if [[ "$(stat -c %U /home/dev 2>/dev/null)" != "$WORK_USER" ]]; then
    log "fixing ownership of /home/dev for '$WORK_USER' (migrated or root-owned volume)"
    chown -R "$WORK_USER":"$WORK_USER" /home/dev
fi

# ---- 0. stage the deploy key for the dev user ------------------------------
# The host-mounted key at /tmp/deploy_key is root/host-owned and unreadable by
# the unprivileged dev user. Copy it into dev's ~/.ssh now, while we are root.
if [[ -f /tmp/deploy_key ]]; then
    install -d -m 700 -o "$WORK_USER" -g "$WORK_USER" "${WORK_HOME}/.ssh"
    install -m 600 -o "$WORK_USER" -g "$WORK_USER" /tmp/deploy_key "${WORK_HOME}/.ssh/id_ed25519"
fi

# ---- escape hatch ----------------------------------------------------------
if [[ "$DISABLE_NETWORK_BLOCK" == "true" ]]; then
    log "DISABLE_NETWORK_BLOCK=true — NO firewall, NO proxy (unfiltered egress)"
    run_workload
fi

command -v nft        >/dev/null || { echo "FATAL: nft not found in image" >&2; exit 1; }
command -v tinyproxy  >/dev/null || { echo "FATAL: tinyproxy not found in image" >&2; exit 1; }

# ---- 1. (re)generate the proxy hostname allowlist from WHITELIST_HOSTS ------
# WHITELIST_HOSTS is the same whitespace-separated hostname list the previous
# IP-allowlist model used; here it is the source of truth for the proxy filter.
# Anchor each host as an extended regex so "github.com" matches "github.com" and
# "api.github.com" but not "notgithub.com" or "github.com.evil.test".
ALLOWLIST=/etc/tinyproxy/egress-allowlist.txt
if [[ -s "$ALLOWLIST" ]]; then
    # Pre-populated by launch.sh (bind-mounted, read-only). Use as-is: this is
    # what lets editing WHITELIST_HOSTS + a plain relaunch update the allowlist
    # without --recreate (launch.sh rewrites this host file each run).
    log "proxy allowlist: $(grep -c . "$ALLOWLIST" 2>/dev/null || echo 0) pattern(s) (from launch.sh)"
else
    # Fallback (e.g. running the image directly, no mount): build from the env.
    for h in ${WHITELIST_HOSTS:-}; do
        esc=$(printf '%s' "$h" | sed 's/[.[\*^$()+?{|]/\\&/g')
        printf '(^|\\.)%s$\n' "$esc" >> "$ALLOWLIST"
    done
    chown "$PROXY_USER":"$PROXY_USER" "$ALLOWLIST" 2>/dev/null || true
    log "proxy allowlist: $(grep -c . "$ALLOWLIST" 2>/dev/null || echo 0) pattern(s) from WHITELIST_HOSTS"
fi

# ---- 2. nft default-deny egress, segregated by uid -------------------------
# resolver IPs come from the container's own /etc/resolv.conf (pasta sets it).
RESOLVER_IPS=$(awk '/^nameserver/ {print $2}' /etc/resolv.conf 2>/dev/null \
    | grep -E '^[0-9]+(\.[0-9]+){3}$' | paste -sd, - || true)
PROXY_UID=$(id -u "$PROXY_USER")

mk_set() { printf '%s' "$1" | tr ' ' '\n' | grep -E '.' | paste -sd, - || true; }
INTERNAL_ELEMS=$(mk_set "${INTERNAL_ALLOW_CIDRS:-}")
EDGE_ELEMS=$(mk_set "${TUNNEL_EDGE_IPS:-}")
API_ELEMS=$(mk_set "${TUNNEL_API_IPS:-}")

{
    echo "table inet egress {"
    echo "    chain output {"
    echo "        type filter hook output priority 0; policy drop;"
    echo "        ct state established,related accept"
    echo "        oifname \"lo\" accept"
    echo "        meta skuid ${PROXY_UID} accept"
    [[ -n "$RESOLVER_IPS" ]]  && echo "        ip daddr { ${RESOLVER_IPS} } meta l4proto { tcp, udp } th dport 53 accept"
    [[ -n "$INTERNAL_ELEMS" ]] && echo "        ip daddr { ${INTERNAL_ELEMS} } accept"
    [[ -n "$EDGE_ELEMS" ]]     && echo "        ip daddr { ${EDGE_ELEMS} } tcp dport 7844 accept"
    [[ -n "$API_ELEMS" ]]      && echo "        ip daddr { ${API_ELEMS} } tcp dport 443 accept"
    echo "        counter drop"
    echo "    }"
    echo "}"
} | nft -f -
log "nft egress filter installed (proxy uid ${PROXY_UID} = egress; workload = default-deny)"

# ---- 3. start the forward proxy (drops to rcproxy via its config) ----------
PROXY_LOG=/var/log/tinyproxy/tinyproxy.log
install -d -m 755 -o "$PROXY_USER" -g "$PROXY_USER" /var/log/tinyproxy /run/tinyproxy
# -d keeps tinyproxy in the foreground (deterministic across versions, some of
# which daemonize by default) and makes it log to the console; we redirect that
# to a world-readable file so `egress-log` (run as dev) can show which hostnames
# the proxy blocked, then background it so it survives the exec into the workload
# as a child of PID 1. It still setuids to rcproxy via the config's User/Group.
: > "$PROXY_LOG"; chmod 644 "$PROXY_LOG"
tinyproxy -d -c /etc/tinyproxy/tinyproxy.conf >> "$PROXY_LOG" 2>&1 &
for _ in $(seq 1 25); do
    if (exec 3<>"/dev/tcp/127.0.0.1/${PROXY_PORT}") 2>/dev/null; then break; fi
    sleep 0.2
done
log "tinyproxy listening on 127.0.0.1:${PROXY_PORT}"

# ---- 4. egress self-check --------------------------------------------------
# (a) direct to the canary IP must FAIL  -> default-deny is enforcing.
# (b) the proxy must reach an allowlisted host -> the sanctioned path works.
# (c) a DIRECT dial to that same host's resolved IP must FAIL -> no IP bypass.
PROBE_HOST="$(printf '%s' "${REPO_URL:-}" | sed -E 's#^(git@|ssh://git@|https://)##; s#[:/].*$##')"
[[ -n "$PROBE_HOST" ]] || PROBE_HOST="github.com"

if timeout 4 bash -c "exec 3<>/dev/tcp/${CANARY_BLOCKED_IP}/80" 2>/dev/null; then
    echo "FATAL: canary ${CANARY_BLOCKED_IP}:80 was reachable directly — egress filter NOT enforcing." >&2
    exit 1
fi
log "ok: direct egress to canary ${CANARY_BLOCKED_IP} is blocked"

if ! timeout 12 curl -fsS -o /dev/null -x "http://127.0.0.1:${PROXY_PORT}" "https://${PROBE_HOST}" 2>/dev/null; then
    echo "FATAL: proxy could not reach allowlisted host ${PROBE_HOST}:443." >&2
    echo "       Is ${PROBE_HOST} in WHITELIST_HOSTS? Check /var/log/tinyproxy/tinyproxy.log." >&2
    exit 1
fi
log "ok: proxy reaches allowlisted host ${PROBE_HOST}"

PROBE_IP="$(getent ahostsv4 "$PROBE_HOST" 2>/dev/null | awk '{print $1; exit}')"
if [[ -n "$PROBE_IP" ]]; then
    if timeout 4 bash -c "exec 3<>/dev/tcp/${PROBE_IP}/443" 2>/dev/null; then
        echo "FATAL: direct dial to ${PROBE_HOST} IP ${PROBE_IP}:443 succeeded — IP bypass is open." >&2
        exit 1
    fi
    log "ok: direct dial to ${PROBE_HOST} IP (${PROBE_IP}) is blocked — no IP bypass"
fi

# ---- 5. hand off (or, for --verify, stop here having proven the filter) ----
if [[ "${VERIFY_ONLY:-false}" == "true" ]]; then
    log "VERIFY_ONLY=true — egress filter + proxy verified; exiting without workload"
    exit 0
fi
run_workload
