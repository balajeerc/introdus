# shellcheck shell=bash
# Single source of truth for the selectable coding agents.
#
# Sourced (never executed) by BOTH sides of the harness:
#   - create-dev-container.sh  (host wizard) — renders the checklist and appends
#                               each selected agent's egress hosts to WHITELIST.
#   - install-agents           (in-container) — installs the selected agents.
#
# Keep it a pure data file: only array definitions, no side effects, so both a
# host bash and the container bash can source it safely. Requires bash 4+
# (associative arrays) — the harness already relies on bash 4 elsewhere.
#
# Supply-chain posture: every npm-published agent is installed with
# `pnpm add -g --ignore-scripts` (no lifecycle scripts run). Agents that are NOT
# on npm (method=script) run a vendor installer instead — those are inherently
# less contained and are flagged as such in the wizard.

# Display / selection order.
AGENT_IDS=(claude codex antigravity opencode pi kilocode)

declare -A AGENT_LABEL=(
    [claude]="Claude (Anthropic)"
    [codex]="Codex (OpenAI)"
    [antigravity]="Antigravity (Google)"
    [opencode]="Opencode (Open source)"
    [pi]="Pi agent (Open source)"
    [kilocode]="Kilocode CLI (kilo.sh)"
)

# How each agent is installed:
#   pnpm    -> pnpm add -g --ignore-scripts <spec>   (spec = npm package)
#   script  -> curl <spec> | bash   (spec = installer URL) — NOT contained by
#              --ignore-scripts; runs vendor code at container setup.
declare -A AGENT_METHOD=(
    [claude]=pnpm
    [codex]=pnpm
    [antigravity]=script
    [opencode]=pnpm
    [pi]=pnpm
    [kilocode]=pnpm
)

declare -A AGENT_SPEC=(
    [claude]="@anthropic-ai/claude-code"
    [codex]="@openai/codex"
    [antigravity]="https://antigravity.google/cli/install.sh"
    [opencode]="opencode-ai"
    [pi]="@earendil-works/pi-coding-agent"
    [kilocode]="@kilocode/cli"
)

# The command each agent installs, for post-install verification / idempotency
# and the "how to run it" banner. (antigravity's binary is named `agy`.)
declare -A AGENT_CMD=(
    [claude]=claude
    [codex]=codex
    [antigravity]=agy
    [opencode]=opencode
    [pi]=pi
    [kilocode]=kilo
)

# Extra egress hosts each agent needs, appended to WHITELIST_HOSTS by the wizard
# when the agent is selected. INSTALL is only ever the npm registry (already in
# the default whitelist) for pnpm agents; these are best-effort RUNTIME/auth
# hosts. Intentionally tight — if an agent still gets blocked, `egress-log`
# inside the container surfaces the missing host to add to WHITELIST_HOSTS.
declare -A AGENT_HOSTS=(
    [claude]=""                              # covered by the default whitelist
    # codex: verified from the shipped Rust binary. chatgpt.com is the ChatGPT-
    # auth model backend, auth.openai.com the login, api.openai.com the API-key
    # path. (Suffix matching means chatgpt.com also covers ab.chatgpt.com.)
    [codex]="api.openai.com auth.openai.com chatgpt.com"
    # antigravity (Gemini-backed): install/update hosts + Google OAuth + the
    # Cloud Code (Gemini Code Assist) model API. Derived from the installed `agy`
    # binary. storage.googleapis.com is where the vendor installer downloads the
    # CLI tarball from (without it the install 403s). Optional telemetry
    # (safebrowsing/play/statsig) is left out to keep egress tight — add via
    # egress-log if you actually need it.
    [antigravity]="antigravity.google antigravity-cli-auto-updater-974169037036.us-central1.run.app storage.googleapis.com accounts.google.com oauth2.googleapis.com www.googleapis.com cloudcode-pa.googleapis.com iamcredentials.googleapis.com"
    # opencode: its own infra — opencode.ai (auth/zen; suffix-covers
    # api./app./console./dev.opencode.ai) and models.dev (the model registry it
    # loads at startup). opencode is BYO-provider; the one custom provider we
    # support here is OpenRouter (openrouter.ai/api/v1). Claude works too with no
    # extra host — api.anthropic.com is already in the default whitelist.
    [opencode]="opencode.ai models.dev openrouter.ai"
    # pi: defaults to Claude, whose api.anthropic.com + claude.ai are already in
    # the default whitelist — console.anthropic.com is the OAuth login gap. Also
    # allows OpenRouter (openrouter.ai), the one custom provider we support.
    [pi]="console.anthropic.com openrouter.ai"
    [kilocode]="kilo.ai api.kilo.ai"
)

# Agents already baked into the base image (installed at build time with direct
# egress). The in-container installer skips these — it never has to reinstall
# them, and it must not clobber claude's build-time native-binary step.
declare -A AGENT_PREBAKED=(
    [claude]=true
)
