//! The selectable coding-agent registry — the Rust mirror of
//! `container/agents.sh` (which remains the source of truth consumed by the
//! in-container `install-agents` script). Kept in lock-step by hand; when you
//! add or change an agent, update both this table and `agents.sh`.

/// How an agent is installed inside the container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMethod {
    /// `pnpm add -g --ignore-scripts <spec>` — no package lifecycle scripts run.
    Pnpm,
    /// `pnpm add -g --allow-build=<spec> <spec>` — the package's own postinstall
    /// IS allowed to run (claude-code's `install.cjs`, which copies the native
    /// binary shipped as an npm optionalDependency into place). Still pulls only
    /// from the npm registry; the sole relaxation vs. `Pnpm` is running that one
    /// lifecycle script. Flagged in the wizard.
    PnpmBuild,
    /// `curl <spec> | bash` — a vendor installer, NOT contained by
    /// `--ignore-scripts`. Flagged as higher-risk in the wizard.
    Script,
}

/// Whether an agent can bypass its approval/permission prompts, and how — used
/// to offer an unattended-mode launch. Launch-time only: the shell registry in
/// `container/agents.sh` has no counterpart, since nothing on that side ever
/// launches an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Yolo {
    /// A flag that bypasses ALL approval prompts (fully unattended, dangerous).
    Bypass(&'static str),
    /// A flag that auto-approves most actions but still honours deny rules — not
    /// a guaranteed skip-everything.
    Auto(&'static str),
    /// Always auto-approves by design; no flag needed.
    Always,
    /// No bypass/auto option.
    None,
}

/// A single coding agent the harness can install and launch.
#[derive(Debug, Clone, Copy)]
pub struct Agent {
    /// Stable id used in `INSTALL_AGENTS` and on the CLI (e.g. `codex`).
    pub id: &'static str,
    /// Human-facing label shown in the wizard checklist.
    pub label: &'static str,
    /// Install mechanism.
    pub method: InstallMethod,
    /// npm package name (Pnpm) or installer URL (Script).
    pub spec: &'static str,
    /// The command the agent installs, for verification and the run banner.
    pub cmd: &'static str,
    /// Extra egress hosts the agent needs, appended to `WHITELIST_HOSTS` when
    /// selected. Space-split, best-effort runtime/auth hosts.
    pub hosts: &'static str,
    /// Baked into the base image at build time; the in-container installer
    /// skips these and must not clobber their build-time native steps.
    pub prebaked: bool,
    /// How this agent can skip its approval prompts, offered at launch time.
    pub yolo: Yolo,
}

/// The full registry, in display/selection order. Mirrors `AGENT_IDS` and the
/// associative arrays in `container/agents.sh`.
pub const AGENTS: &[Agent] = &[
    Agent {
        id: "claude",
        label: "Claude (Anthropic)",
        // PnpmBuild, not Pnpm: claude-code's postinstall (install.cjs) copies the
        // native binary (an npm optionalDependency, so still fetched only from the
        // registry) over its placeholder. --ignore-scripts would leave a broken
        // stub, so the build script is allowed for this package alone.
        method: InstallMethod::PnpmBuild,
        spec: "@anthropic-ai/claude-code",
        cmd: "claude",
        hosts: "", // native binary ships via the npm registry — no extra host
        prebaked: false,
        yolo: Yolo::Bypass("--dangerously-skip-permissions"),
    },
    Agent {
        id: "codex",
        label: "Codex (OpenAI)",
        method: InstallMethod::Pnpm,
        spec: "@openai/codex",
        cmd: "codex",
        hosts: "api.openai.com auth.openai.com chatgpt.com",
        prebaked: false,
        // `--yolo` is the official alias; the long form is the documented flag.
        yolo: Yolo::Bypass("--dangerously-bypass-approvals-and-sandbox"),
    },
    Agent {
        id: "antigravity",
        label: "Antigravity (Google)",
        method: InstallMethod::Script,
        spec: "https://antigravity.google/cli/install.sh",
        cmd: "agy",
        hosts: "antigravity.google antigravity-cli-auto-updater-974169037036.us-central1.run.app storage.googleapis.com accounts.google.com oauth2.googleapis.com www.googleapis.com cloudcode-pa.googleapis.com iamcredentials.googleapis.com",
        prebaked: false,
        // agy mirrors claude-code's flag name (not Gemini CLI's `--yolo`).
        yolo: Yolo::Bypass("--dangerously-skip-permissions"),
    },
    Agent {
        id: "opencode",
        label: "Opencode (Open source)",
        method: InstallMethod::Pnpm,
        spec: "opencode-ai",
        cmd: "opencode",
        hosts: "opencode.ai models.dev openrouter.ai",
        prebaked: false,
        // `--auto` auto-approves but still honours deny rules — not a full bypass.
        yolo: Yolo::Auto("--auto"),
    },
    Agent {
        id: "pi",
        label: "Pi agent (Open source)",
        method: InstallMethod::Pnpm,
        spec: "@earendil-works/pi-coding-agent",
        cmd: "pi",
        hosts: "console.anthropic.com openrouter.ai",
        prebaked: false,
        yolo: Yolo::Always, // no permission system by design — always auto-approves
    },
    Agent {
        id: "kilocode",
        label: "Kilocode CLI (kilo.sh)",
        method: InstallMethod::Pnpm,
        spec: "@kilocode/cli",
        cmd: "kilo",
        hosts: "kilo.ai api.kilo.ai",
        prebaked: false,
        yolo: Yolo::Auto("--auto"), // autonomous mode; deny rules still apply
    },
];

/// Paseo — the optional agent *orchestrator* (not a coding agent itself). A
/// daemon you pair a phone/desktop/web app with (via the QR flow) and then use
/// to orchestrate the installed agents through the paseo relay: the daemon dials
/// OUT to the relay with end-to-end encryption, so nothing is exposed inbound.
/// The harness does not wrap agent launches in `paseo run` (headless mode isn't
/// the intended path) — it only installs the CLI and wires the relay egress.
/// Opted into separately from the agent checklist (`INSTALL_PASEO`), installed
/// via the same pnpm path. Mirrors the `PASEO_*` constants in
/// `container/agents.sh`.
pub mod paseo {
    /// npm package providing the paseo CLI + daemon.
    pub const SPEC: &str = "@getpaseo/cli";
    /// The command it installs.
    pub const CMD: &str = "paseo";
    /// Proxy-allowlist host for paseo's plain-HTTPS traffic (pairing/registration
    /// under `app.paseo.sh`, which honors `HTTPS_PROXY`). Suffix-matching also
    /// admits `relay.paseo.sh` at the proxy — but the daemon's relay link is a
    /// WebSocket via the `ws` lib, which ignores the proxy and dials the relay
    /// directly, so the proxy entry alone can't carry it. See [`RELAY_HOST`].
    pub const HOST: &str = "paseo.sh";
    /// The relay endpoint the daemon dials OUT to over a WebSocket
    /// (`wss://relay.paseo.sh/ws`). Because `ws` bypasses `HTTPS_PROXY`, this host
    /// is resolved at launch and its IPs are allowed directly on 443 by the nft
    /// filter (same shape as the cloudflared tunnel bypass) — without it, the
    /// workload's default-deny egress blackholes the relay and phone pairing
    /// times out. Anycast/stable enough for a launch-time resolve.
    pub const RELAY_HOST: &str = "relay.paseo.sh";
}

/// Look up an agent by its id.
pub fn find(id: &str) -> Option<&'static Agent> {
    AGENTS.iter().find(|a| a.id == id)
}

/// True if `id` names a known agent.
pub fn is_known(id: &str) -> bool {
    find(id).is_some()
}

impl Agent {
    /// The agent's extra egress hosts, split into individual hostnames.
    pub fn host_list(&self) -> Vec<&'static str> {
        self.hosts.split_whitespace().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta14_script_agents_use_url_specs() {
        for a in AGENTS {
            if a.method == InstallMethod::Script {
                assert!(
                    a.spec.starts_with("https://"),
                    "{} spec must be a URL",
                    a.id
                );
            }
        }
    }

    #[test]
    fn ta130_paseo_relay_host_is_under_the_proxy_allowlist_host() {
        // The relay bypass resolves RELAY_HOST by IP, but the same host must also
        // be admitted at the proxy (suffix of HOST) so paseo's non-WS traffic to
        // it isn't separately blocked.
        assert_eq!(paseo::RELAY_HOST, "relay.paseo.sh");
        assert!(
            paseo::RELAY_HOST.ends_with(paseo::HOST),
            "relay host {} must be covered by the proxy allowlist host {}",
            paseo::RELAY_HOST,
            paseo::HOST
        );
    }

    #[test]
    fn ta14_yolo_flags_match_the_known_cli_flags() {
        // Guards the exact (typo-prone, dangerous) flag strings per agent.
        let want = |id| find(id).map(|a| a.yolo);
        assert_eq!(
            want("claude"),
            Some(Yolo::Bypass("--dangerously-skip-permissions"))
        );
        assert_eq!(
            want("codex"),
            Some(Yolo::Bypass("--dangerously-bypass-approvals-and-sandbox"))
        );
        assert_eq!(
            want("antigravity"),
            Some(Yolo::Bypass("--dangerously-skip-permissions"))
        );
        assert_eq!(want("opencode"), Some(Yolo::Auto("--auto")));
        assert_eq!(want("pi"), Some(Yolo::Always));
        assert_eq!(want("kilocode"), Some(Yolo::Auto("--auto")));
        // Every Bypass/Auto flag is a real `--flag`.
        for a in AGENTS {
            if let Yolo::Bypass(f) | Yolo::Auto(f) = a.yolo {
                assert!(f.starts_with("--"), "{} yolo flag must be a --flag", a.id);
            }
        }
    }
}
