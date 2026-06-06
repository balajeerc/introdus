#!/usr/bin/env bash
# Bootstraps a new project directory to use the remote-control-harness:
# prompts for config, writes .env, creates shared_data/, and drops a
# launch.sh wrapper that delegates to this harness's launch.sh.
#
# Run from an empty directory:
#   cd /path/to/empty-project-dir
#   /path/to/remote-control-harness/create-dev-container.sh
set -euo pipefail

HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(pwd)"

SAMPLE_ENV="$HARNESS_DIR/sample.env"
HARNESS_LAUNCH="$HARNESS_DIR/launch.sh"
[[ -f "$SAMPLE_ENV"    ]] || { echo "error: sample.env not found at $SAMPLE_ENV"     >&2; exit 1; }
[[ -f "$HARNESS_LAUNCH" ]] || { echo "error: launch.sh not found at $HARNESS_LAUNCH" >&2; exit 1; }

if [[ "$PROJECT_DIR" == "$HARNESS_DIR" ]]; then
    echo "error: run this from a separate project directory, not the harness directory itself." >&2
    exit 1
fi

if [[ -n "$(ls -A "$PROJECT_DIR" 2>/dev/null)" ]]; then
    echo "error: current directory is not empty: $PROJECT_DIR" >&2
    echo "       create-dev-container.sh expects to bootstrap into an empty directory." >&2
    exit 1
fi

# ---- prompt helpers --------------------------------------------------------

prompt_required() {
    local __var="$1" __label="$2" __reply
    while true; do
        read -r -p "  $__label: " __reply
        if [[ -n "$__reply" ]]; then
            printf -v "$__var" '%s' "$__reply"
            return
        fi
        echo "  (required)"
    done
}

prompt_default() {
    local __var="$1" __label="$2" __default="$3" __reply
    read -r -p "  $__label [$__default]: " __reply
    printf -v "$__var" '%s' "${__reply:-$__default}"
}

prompt_optional() {
    local __var="$1" __label="$2" __reply
    read -r -p "  $__label (optional, blank to skip): " __reply
    printf -v "$__var" '%s' "$__reply"
}

prompt_yesno() {
    local __var="$1" __label="$2" __default="$3" __reply __hint
    if [[ "$__default" == "true" ]]; then __hint="[Y/n]"; else __hint="[y/N]"; fi
    read -r -p "  $__label $__hint: " __reply
    __reply="${__reply:-$__default}"
    case "${__reply,,}" in
        y|yes|true|1) printf -v "$__var" 'true'  ;;
        *)            printf -v "$__var" 'false' ;;
    esac
}

slugify() {
    printf '%s' "$1" \
        | tr -c '[:alnum:]_-' '-' \
        | sed -E 's/-+/-/g; s/^-//; s/-$//'
}

# Generates a per-project ed25519 deploy key, prints the public half with a
# provider-specific hint, waits for you to register it, then prompts for the
# repo URL and validates it against the chosen provider. Sets DEPLOY_KEY_PATH
# and REPO_URL (normalized to ssh form so the mounted key is actually used —
# the container clones over ssh, and an https URL would ignore the key).
generate_deploy_key() {
    local slug provider host key_ui_path url_hint choice
    slug="$(slugify "$PROJECT_NAME")"
    [[ -n "$slug" ]] || { echo "  error: could not derive a slug from '$PROJECT_NAME'" >&2; exit 1; }

    echo
    echo "  Pick the git hosting provider:"
    echo "    1) GitHub     (github.com)"
    echo "    2) GitLab     (gitlab.com)"
    echo "    3) Bitbucket  (bitbucket.org)"
    while true; do
        read -r -p "  Choice [1-3]: " choice
        case "$choice" in
            1) provider="GitHub";    host="github.com";    key_ui_path="Repo -> Settings -> Deploy keys -> Add deploy key";     url_hint="git@github.com:user/repo.git  OR  https://github.com/user/repo(.git)"; break ;;
            2) provider="GitLab";    host="gitlab.com";    key_ui_path="Repo -> Settings -> Repository -> Deploy keys";         url_hint="git@gitlab.com:group/(subgroup/)repo.git  OR  https://gitlab.com/group/(subgroup/)repo(.git)"; break ;;
            3) provider="Bitbucket"; host="bitbucket.org"; key_ui_path="Repo -> Repository settings -> Access keys -> Add key"; url_hint="git@bitbucket.org:workspace/repo.git  OR  https://bitbucket.org/workspace/repo(.git)"; break ;;
            *) echo "  invalid choice; enter 1, 2, or 3." >&2 ;;
        esac
    done

    local key_path="$HOME/.ssh/${slug}_deploy_key"
    if [[ -f "$key_path" ]]; then
        echo "  error: key already exists at $key_path (aborting to avoid overwrite)." >&2
        echo "         delete it first, or choose a different project name." >&2
        exit 1
    fi

    mkdir -p "$HOME/.ssh"
    chmod 700 "$HOME/.ssh"
    echo
    echo "  ==> Generating ed25519 deploy key at $key_path"
    ssh-keygen -t ed25519 -f "$key_path" -C "${slug} deploy key" -N "" >/dev/null
    chmod 600 "$key_path"
    chmod 644 "${key_path}.pub"

    echo
    echo "  ============================================================"
    echo "   PUBLIC KEY -- add to ${provider}:"
    echo "     ${key_ui_path}"
    echo "  ============================================================"
    cat "${key_path}.pub"
    echo "  ============================================================"
    read -r -p "  Press Enter once you've added the deploy key on ${provider}... " _

    echo
    echo "  Expected URL formats for ${provider}:"
    echo "     ${url_hint}"
    local repo_url url_host repo_path
    while true; do
        read -r -p "  Paste the repo URL: " repo_url
        [[ -n "${repo_url// }" ]] || { echo "  (required)"; continue; }

        if   [[ "$repo_url" =~ ^git@([^:]+):(.+)$ ]];       then url_host="${BASH_REMATCH[1]}";     repo_path="${BASH_REMATCH[2]}"
        elif [[ "$repo_url" =~ ^ssh://git@([^/]+)/(.+)$ ]]; then url_host="${BASH_REMATCH[1]}";     repo_path="${BASH_REMATCH[2]}"
        elif [[ "$repo_url" =~ ^https?://([^/]+)/(.+)$ ]];  then url_host="${BASH_REMATCH[1]##*@}"; repo_path="${BASH_REMATCH[2]}"
        else echo "  unrecognized URL format; try again." >&2; continue
        fi

        if [[ "$url_host" == *:* ]]; then
            echo "  URLs with non-standard ports aren't supported; try again." >&2; continue
        fi
        if [[ "$url_host" != "$host" ]]; then
            echo "  URL host '$url_host' doesn't match selected provider host '$host'; try again." >&2; continue
        fi
        repo_path="${repo_path%/}"; repo_path="${repo_path%.git}"
        if [[ ! "$repo_path" =~ ^[^/[:space:]]+(/[^/[:space:]]+)+$ ]]; then
            echo "  couldn't parse owner/repo from path: $repo_path; try again." >&2; continue
        fi
        break
    done

    REPO_URL="git@${host}:${repo_path}.git"
    DEPLOY_KEY_PATH="$key_path"
}

cat <<EOF
============================================================
  create-dev-container.sh
============================================================
  harness:  $HARNESS_DIR
  project:  $PROJECT_DIR

  This will prompt for config, write .env, create shared_data/,
  and drop a ./launch.sh wrapper that calls the harness.
============================================================

EOF

DEFAULT_PROJECT_NAME="$(basename "$PROJECT_DIR")"

prompt_default  PROJECT_NAME           "Project name (container + workdir slug)"             "$DEFAULT_PROJECT_NAME"

prompt_yesno GENERATE_DEPLOY_KEY "Generate a new per-project deploy key now?" "true"
if [[ "$GENERATE_DEPLOY_KEY" == "true" ]]; then
    generate_deploy_key   # sets REPO_URL and DEPLOY_KEY_PATH
else
    prompt_required REPO_URL        "Git repo URL (e.g. git@github.com:org/repo.git)"
    prompt_required DEPLOY_KEY_PATH "Absolute path to deploy key (private key)"
fi
prompt_default  WEBAPP_PORT            "Port the webapp binds to inside the container"        "3000"
prompt_default  MEM_LIMIT              "Container memory limit (e.g. 4g, 8g, 16g)"            "8g"
prompt_default  CPU_LIMIT              "Container CPU limit (number of CPUs)"                 "8"
prompt_optional ON_LAUNCH_SCRIPT       "Command to run on every container start (e.g. 'pnpm dev')"
prompt_optional EXTRA_PORTS            "Extra ports to publish to 127.0.0.1, space-separated (e.g. '8123 16379:6379')"
prompt_yesno    EXPOSE_WEBAPP          "Expose webapp via Cloudflare quick tunnel?"           "false"
prompt_yesno    ENABLE_NOTIFY_SH_ALERTS "Enable mobile push notifications via ntfy.sh?"       "false"
NTFY_SH_TOPIC=""
if [[ "$ENABLE_NOTIFY_SH_ALERTS" == "true" ]]; then
    prompt_required NTFY_SH_TOPIC "ntfy.sh topic name (treat like a password)"
fi

if [[ ! -f "$DEPLOY_KEY_PATH" ]]; then
    echo
    echo "  warning: DEPLOY_KEY_PATH does not exist yet: $DEPLOY_KEY_PATH"
    echo "  (proceeding anyway — launch.sh will fail until the key is in place)"
fi

# ---- shared_data dir -------------------------------------------------------

SHARED_DATA_PATH="$PROJECT_DIR/shared_data"
mkdir -p "$SHARED_DATA_PATH"

# ---- WHITELIST_HOSTS — copy block verbatim from sample.env -----------------
# The harness needs WHITELIST_HOSTS set; reproduce sample.env's default block
# so the generated .env is usable as-is. User can trim later.

WHITELIST_BLOCK="$(awk '/^WHITELIST_HOSTS="/{f=1} f{print} f && /^"$/{exit}' "$SAMPLE_ENV")"
if [[ -z "$WHITELIST_BLOCK" ]]; then
    echo "error: failed to extract WHITELIST_HOSTS block from $SAMPLE_ENV" >&2
    exit 1
fi

# ---- write .env ------------------------------------------------------------

ENV_OUT="$PROJECT_DIR/.env"
{
    echo "# Generated by create-dev-container.sh on $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "# Harness: $HARNESS_DIR"
    echo "# See $SAMPLE_ENV for the full list of tunables (PIDS_LIMIT,"
    echo "# CANARY_BLOCKED_IP, RESOLVE_INTERVAL, etc.) — add them here as needed."
    echo
    echo "PROJECT_NAME=$PROJECT_NAME"
    echo "REPO_URL=$REPO_URL"
    echo "DEPLOY_KEY_PATH=$DEPLOY_KEY_PATH"
    echo "WEBAPP_PORT=$WEBAPP_PORT"
    echo "MEM_LIMIT=$MEM_LIMIT"
    echo "CPU_LIMIT=$CPU_LIMIT"
    echo
    echo "SHARED_DATA_PATH=$SHARED_DATA_PATH"
    if [[ -n "$ON_LAUNCH_SCRIPT" ]]; then
        printf 'ON_LAUNCH_SCRIPT=%q\n' "$ON_LAUNCH_SCRIPT"
    fi
    if [[ -n "$EXTRA_PORTS" ]]; then
        printf 'EXTRA_PORTS=%q\n' "$EXTRA_PORTS"
    fi
    if [[ "$EXPOSE_WEBAPP" == "true" ]]; then
        echo "EXPOSE_WEBAPP=true"
    fi
    if [[ "$ENABLE_NOTIFY_SH_ALERTS" == "true" ]]; then
        echo "ENABLE_NOTIFY_SH_ALERTS=true"
        printf 'NTFY_SH_TOPIC=%q\n' "$NTFY_SH_TOPIC"
    fi
    echo
    echo "$WHITELIST_BLOCK"
} > "$ENV_OUT"
chmod 600 "$ENV_OUT"

# ---- launch.sh wrapper -----------------------------------------------------
# cd to wrapper's own dir before exec so it works from any cwd; the harness
# launch.sh reads .env relative to its caller's cwd.

WRAPPER="$PROJECT_DIR/launch.sh"
cat > "$WRAPPER" <<EOF
#!/usr/bin/env bash
# Wrapper generated by create-dev-container.sh — delegates to the
# remote-control-harness launch.sh with this project's .env.
set -euo pipefail
cd "\$(cd "\$(dirname "\${BASH_SOURCE[0]}")" && pwd)"
exec "$HARNESS_LAUNCH" "\$@"
EOF
chmod +x "$WRAPPER"

cat <<EOF

============================================================
  Setup complete.
============================================================
  .env:           $ENV_OUT
  shared_data:    $SHARED_DATA_PATH  (mounted read-only at /root/shared_data)
  launch.sh:      $WRAPPER  -> $HARNESS_LAUNCH

  Next:
    cd "$PROJECT_DIR"
    ./launch.sh                 # bring the container up
    ./launch.sh --rebuild-base  # force a base-image rebuild
    ./launch.sh --pull          # fast-forward repo on next start
============================================================
EOF
