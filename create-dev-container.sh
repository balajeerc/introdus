#!/usr/bin/env bash
# Bootstraps a new project to use the remote-control-harness: walks you through
# the git repo + deploy key, creates a project subdirectory in the current
# directory, writes .env, creates shared_data/, drops a launch.sh wrapper that
# delegates to this harness's launch.sh, and optionally brings the container up.
#
# Install it onto your PATH with ./host_install.sh, then run from anywhere:
#   cd ~
#   create-dev-container.sh        # creates ~/<project>/ and sets it up
#
# Or run it in place from the harness checkout:
#   cd /path/to/empty-base-dir
#   /path/to/remote-control-harness/create-dev-container.sh
set -euo pipefail

# ---- locate the harness ----------------------------------------------------
# Resolve symlinks so that when this script is installed (symlinked) into
# ~/.local/bin it still finds sample.env / launch.sh in the real harness repo.
SOURCE="${BASH_SOURCE[0]}"
while [[ -L "$SOURCE" ]]; do
    DIR="$(cd -P "$(dirname "$SOURCE")" && pwd)"
    SOURCE="$(readlink "$SOURCE")"
    [[ "$SOURCE" != /* ]] && SOURCE="$DIR/$SOURCE"
done
HARNESS_DIR="$(cd -P "$(dirname "$SOURCE")" && pwd)"

BASE_DIR="$(pwd)"

SAMPLE_ENV="$HARNESS_DIR/sample.env"
HARNESS_LAUNCH="$HARNESS_DIR/launch_dev_container.sh"
AGENTS_REGISTRY="$HARNESS_DIR/container/agents.sh"
[[ -f "$SAMPLE_ENV"     ]] || { echo "error: sample.env not found at $SAMPLE_ENV"     >&2; exit 1; }
[[ -f "$HARNESS_LAUNCH" ]] || { echo "error: launch_dev_container.sh not found at $HARNESS_LAUNCH" >&2; exit 1; }
[[ -f "$AGENTS_REGISTRY" ]] || { echo "error: agent registry not found at $AGENTS_REGISTRY" >&2; exit 1; }
# shellcheck source=container/agents.sh
source "$AGENTS_REGISTRY"   # AGENT_IDS / AGENT_LABEL / AGENT_METHOD / AGENT_SPEC / AGENT_HOSTS

if [[ "$BASE_DIR" == "/" ]]; then
    echo "error: refusing to run from /" >&2
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

# Interactive checklist of coding agents to install (from the shared registry).
# Claude is pre-ticked as the common default, but it's opt-out-able — untick it
# and it is genuinely not installed (nothing is baked into the base image).
# Sets INSTALL_AGENTS (space-separated ids, registry order) and AGENT_EXTRA_HOSTS
# (per-agent egress hosts to fold into the WHITELIST block).
choose_agents() {
    local -A picked=()
    local id
    for id in "${AGENT_IDS[@]}"; do picked[$id]=0; done
    picked[claude]=1

    echo
    echo "  Coding agents to install in the container:"
    echo "    (npm agents install with 'pnpm add -g --ignore-scripts'; claude uses"
    echo "     --allow-build so it can place its native binary; agents not on npm run"
    echo "     a vendor installer, flagged below. Untick all for no agent.)"
    while true; do
        echo
        local i=1 mark note
        local -a idx_to_id=()
        for id in "${AGENT_IDS[@]}"; do
            idx_to_id[$i]="$id"
            mark=" "; [[ "${picked[$id]}" == 1 ]] && mark="x"
            note=""; [[ "${AGENT_METHOD[$id]}" == script ]] && note="   (vendor script — runs remote code)"
            printf "    %d) [%s] %s%s\n" "$i" "$mark" "${AGENT_LABEL[$id]}" "$note"
            ((i++))
        done
        echo
        local reply n
        read -r -p "  Toggle by number (space-separated), or Enter to confirm: " reply
        [[ -z "${reply// }" ]] && break
        for n in $reply; do
            if [[ "$n" =~ ^[0-9]+$ && -n "${idx_to_id[$n]:-}" ]]; then
                id="${idx_to_id[$n]}"; picked[$id]=$(( 1 - picked[$id] ))
            else
                echo "  ignoring invalid selection: $n"
            fi
        done
    done

    INSTALL_AGENTS=""
    for id in "${AGENT_IDS[@]}"; do
        [[ "${picked[$id]}" == 1 ]] && INSTALL_AGENTS+="${INSTALL_AGENTS:+ }$id"
    done
    # No force-claude fallback: if the user unticked everything, INSTALL_AGENTS
    # stays empty and no coding agent is installed.
    echo
    if [[ -n "$INSTALL_AGENTS" ]]; then
        echo "  ==> Agents: $INSTALL_AGENTS"
    else
        echo "  ==> Agents: (none selected — no coding agent will be installed)"
    fi

    for id in $INSTALL_AGENTS; do
        if [[ "${AGENT_METHOD[$id]}" == script ]]; then
            echo
            echo "  NOTE: ${AGENT_LABEL[$id]} is not on npm — it installs inside the container by"
            echo "        piping a vendor script to bash (curl ${AGENT_SPEC[$id]} | bash)."
            echo "        That runs remote code and is NOT contained by --ignore-scripts."
        fi
    done

    AGENT_EXTRA_HOSTS=()
    local h
    for id in $INSTALL_AGENTS; do
        for h in ${AGENT_HOSTS[$id]:-}; do AGENT_EXTRA_HOSTS+=("$h"); done
    done
}

# ---- repo / deploy-key helpers ---------------------------------------------

# Prompts for the git hosting provider. Sets PROVIDER, HOST (empty for the
# self-hosted "Other" option), KEY_UI_PATH and URL_HINT.
choose_provider() {
    local choice
    echo
    echo "  Pick the git hosting provider:"
    echo "    1) GitHub     (github.com)"
    echo "    2) GitLab     (gitlab.com)"
    echo "    3) Bitbucket  (bitbucket.org)"
    echo "    4) Other / self-hosted (any host, incl. ssh aliases)"
    while true; do
        read -r -p "  Choice [1-4]: " choice
        case "$choice" in
            1) PROVIDER="GitHub";    HOST="github.com";    KEY_UI_PATH="Repo -> Settings -> Deploy keys -> Add deploy key";     URL_HINT="git@github.com:user/repo.git  OR  https://github.com/user/repo(.git)"; break ;;
            2) PROVIDER="GitLab";    HOST="gitlab.com";    KEY_UI_PATH="Repo -> Settings -> Repository -> Deploy keys";         URL_HINT="git@gitlab.com:group/(subgroup/)repo.git  OR  https://gitlab.com/group/(subgroup/)repo(.git)"; break ;;
            3) PROVIDER="Bitbucket"; HOST="bitbucket.org"; KEY_UI_PATH="Repo -> Repository settings -> Access keys -> Add key"; URL_HINT="git@bitbucket.org:workspace/repo.git  OR  https://bitbucket.org/workspace/repo(.git)"; break ;;
            4) PROVIDER="Other";     HOST="";              KEY_UI_PATH="your provider's deploy-key / access-key settings";      URL_HINT="git@host:owner/repo.git  OR  ssh://git@host/owner/repo.git"; break ;;
            *) echo "  invalid choice; enter 1-4." >&2 ;;
        esac
    done
}

# Prompts for the repo URL and validates it. For a named provider the URL host
# must match the provider; for "Other" any host is accepted. ssh forms are
# normalized to git@host:owner/repo.git so the mounted deploy key is used by the
# container clone; https URLs are kept as-is (and won't use the key). Sets
# REPO_URL and REPO_PATH.
prompt_repo_url() {
    local repo_url url_host repo_path is_https
    echo
    echo "  Expected URL formats:"
    echo "     ${URL_HINT}"
    while true; do
        read -r -p "  Paste the repo URL: " repo_url
        [[ -n "${repo_url// }" ]] || { echo "  (required)"; continue; }
        is_https=false
        if   [[ "$repo_url" =~ ^git@([^:]+):(.+)$ ]];       then url_host="${BASH_REMATCH[1]}";     repo_path="${BASH_REMATCH[2]}"
        elif [[ "$repo_url" =~ ^ssh://git@([^/]+)/(.+)$ ]]; then url_host="${BASH_REMATCH[1]}";     repo_path="${BASH_REMATCH[2]}"
        elif [[ "$repo_url" =~ ^https?://([^/]+)/(.+)$ ]];  then url_host="${BASH_REMATCH[1]##*@}"; repo_path="${BASH_REMATCH[2]}"; is_https=true
        else echo "  unrecognized URL format; try again." >&2; continue
        fi

        if [[ "$url_host" == *:* ]]; then
            echo "  URLs with non-standard ports aren't supported; try again." >&2; continue
        fi
        if [[ -n "$HOST" && "$url_host" != "$HOST" ]]; then
            echo "  URL host '$url_host' doesn't match selected provider host '$HOST'; try again." >&2; continue
        fi
        repo_path="${repo_path%/}"; repo_path="${repo_path%.git}"
        if [[ ! "$repo_path" =~ ^[^/[:space:]]+(/[^/[:space:]]+)+$ ]]; then
            echo "  couldn't parse owner/repo from path: $repo_path; try again." >&2; continue
        fi

        REPO_PATH="$repo_path"
        if [[ "$is_https" == "true" ]]; then
            REPO_URL="$repo_url"   # kept as given; a deploy key won't apply over https
        else
            REPO_URL="git@${url_host}:${repo_path}.git"
        fi
        break
    done
}

# Generates a per-project ed25519 deploy key from the given slug, prints the
# public half with the provider hint, and waits for you to register it on the
# provider. Sets DEPLOY_KEY_PATH.
create_deploy_key() {
    local slug="$1" key_path
    key_path="$HOME/.ssh/${slug}_deploy_key"
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
    echo "   PUBLIC KEY -- add to ${PROVIDER}:"
    echo "     ${KEY_UI_PATH}"
    echo "  ============================================================"
    cat "${key_path}.pub"
    echo "  ============================================================"
    read -r -p "  Press Enter once you've added the deploy key on ${PROVIDER}... " _

    DEPLOY_KEY_PATH="$key_path"
}

# ---- intro -----------------------------------------------------------------

cat <<EOF
============================================================
  create-dev-container.sh
============================================================
  harness:   $HARNESS_DIR
  base dir:  $BASE_DIR

  This walks you through the repo + deploy key, creates a
  project subdirectory under the base dir above, writes its
  .env and launch.sh wrapper, and can bring the container up.
============================================================
EOF

# ---- 1. repo + provider ----------------------------------------------------

choose_provider
prompt_repo_url   # sets REPO_URL and REPO_PATH

# ---- 2. project name + target directory ------------------------------------

DEFAULT_PROJECT_NAME="$(slugify "$(basename "$REPO_PATH")")"

prompt_default PROJECT_NAME "Project name (subdir + container + workdir slug)" "$DEFAULT_PROJECT_NAME"

PROJECT_SLUG="$(slugify "$PROJECT_NAME")"
[[ -n "$PROJECT_SLUG" ]] || { echo "error: could not derive a slug from project name '$PROJECT_NAME'." >&2; exit 1; }

PROJECT_DIR="$BASE_DIR/$PROJECT_SLUG"

if [[ "$PROJECT_DIR" == "$HARNESS_DIR" ]]; then
    echo "error: refusing to bootstrap into the harness directory itself." >&2
    exit 1
fi
if [[ -e "$PROJECT_DIR" && -n "$(ls -A "$PROJECT_DIR" 2>/dev/null)" ]]; then
    echo "error: target project directory is not empty: $PROJECT_DIR" >&2
    echo "       pick a different project name, or clear that directory." >&2
    exit 1
fi
mkdir -p "$PROJECT_DIR"
echo
echo "  ==> Project directory: $PROJECT_DIR"

# ---- 3. deploy key ---------------------------------------------------------

prompt_yesno GENERATE_DEPLOY_KEY "Generate a new per-project deploy key now?" "true"
if [[ "$GENERATE_DEPLOY_KEY" == "true" ]]; then
    create_deploy_key "$PROJECT_SLUG"   # sets DEPLOY_KEY_PATH
else
    prompt_required DEPLOY_KEY_PATH "Absolute path to deploy key (private key)"
fi

if [[ "$REPO_URL" == http* ]]; then
    echo
    echo "  warning: REPO_URL is an https URL, so the deploy key won't be used"
    echo "           when the container clones. Use an ssh URL (git@…) instead."
fi
if [[ ! -f "$DEPLOY_KEY_PATH" ]]; then
    echo
    echo "  warning: DEPLOY_KEY_PATH does not exist yet: $DEPLOY_KEY_PATH"
    echo "  (proceeding anyway — launch.sh will fail until the key is in place)"
fi

# ---- 4. remaining container config -----------------------------------------

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

# Notification forwarding (RC_FORWARD_ADDR) is a HOST-level setting configured
# once by host_install.sh, not here — a single listener relays every container
# on this host. Nothing per-project to do for notifications.

# ---- 4b. coding agents to install ------------------------------------------

choose_agents   # sets INSTALL_AGENTS and AGENT_EXTRA_HOSTS

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

# ---- per-project image suffix ----------------------------------------------
# Each project's container is created from its own tag of the shared base image
# (remote-code-<project>-<suffix>) so VS Code Dev Containers — which caches
# attach config keyed by image NAME — keeps each project's container distinct.
# Generate a random 4-char suffix and persist it so the tag is stable across
# launches. Because every host runs this wizard to build its own .env, the SAME
# project bootstrapped on two hosts gets two different suffixes, so their images
# (and VS Code's cached state) never collide across hosts.
IMAGE_SUFFIX="$(od -An -N2 -tx1 /dev/urandom 2>/dev/null | tr -d ' \n')"
if [[ ${#IMAGE_SUFFIX} -ne 4 ]]; then
    # Fallback if /dev/urandom is unavailable: hash of project name + pid.
    IMAGE_SUFFIX="$(printf '%s' "${PROJECT_NAME}-$$" | cksum | cut -c1-4)"
fi

# ---- write .env ------------------------------------------------------------

ENV_OUT="$PROJECT_DIR/.env"
{
    echo "# Generated by create-dev-container.sh on $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "# Harness: $HARNESS_DIR"
    echo "# See $SAMPLE_ENV for the full list of tunables (PIDS_LIMIT,"
    echo "# CANARY_BLOCKED_IP, INTERNAL_ALLOW_CIDRS, etc.) — add them here as needed."
    echo
    echo "PROJECT_NAME=$PROJECT_NAME"
    echo "IMAGE_SUFFIX=$IMAGE_SUFFIX"
    echo "REPO_URL=$REPO_URL"
    echo "DEPLOY_KEY_PATH=$DEPLOY_KEY_PATH"
    echo "WEBAPP_PORT=$WEBAPP_PORT"
    echo "MEM_LIMIT=$MEM_LIMIT"
    echo "CPU_LIMIT=$CPU_LIMIT"
    echo
    echo "# Coding agents installed in the container (see container/agents.sh)."
    # Quoted so a multi-agent list ("claude codex") and an empty selection ("")
    # both round-trip through launch_dev_container.sh's `set -a; source .env`.
    echo "INSTALL_AGENTS=\"$INSTALL_AGENTS\""
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
    if ((${#AGENT_EXTRA_HOSTS[@]})); then
        # Splice the selected agents' egress hosts into the WHITELIST_HOSTS block,
        # just before its closing quote line, so the generated .env allows them.
        printf '%s\n' "${WHITELIST_BLOCK%$'\n"'}"
        echo "# --- hosts required by the selected coding agents ---"
        printf '%s\n' "${AGENT_EXTRA_HOSTS[@]}" | awk '!seen[$0]++'
        printf '%s\n' '"'
    else
        echo "$WHITELIST_BLOCK"
    fi
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
  project dir:    $PROJECT_DIR
  .env:           $ENV_OUT
  shared_data:    $SHARED_DATA_PATH  (mounted read-only at /home/dev/shared_data)
  launch.sh:      $WRAPPER  -> $HARNESS_LAUNCH
============================================================
EOF

# ---- 5. optionally spawn the container -------------------------------------

prompt_yesno LAUNCH_NOW "Bring the container up now?" "true"
if [[ "$LAUNCH_NOW" == "true" ]]; then
    echo
    echo "==> Launching: (cd $PROJECT_DIR && ./launch.sh)"
    echo
    exec "$WRAPPER"
fi

cat <<EOF

  Not launching now. When you're ready:
    cd "$PROJECT_DIR"
    ./launch.sh                 # bring the container up
    ./launch.sh --rebuild-base  # force a base-image rebuild
    ./launch.sh --pull          # fast-forward repo on next start
============================================================
EOF
