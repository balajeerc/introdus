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
    },
    Agent {
        id: "codex",
        label: "Codex (OpenAI)",
        method: InstallMethod::Pnpm,
        spec: "@openai/codex",
        cmd: "codex",
        hosts: "api.openai.com auth.openai.com chatgpt.com",
        prebaked: false,
    },
    Agent {
        id: "antigravity",
        label: "Antigravity (Google)",
        method: InstallMethod::Script,
        spec: "https://antigravity.google/cli/install.sh",
        cmd: "agy",
        hosts: "antigravity.google antigravity-cli-auto-updater-974169037036.us-central1.run.app storage.googleapis.com accounts.google.com oauth2.googleapis.com www.googleapis.com cloudcode-pa.googleapis.com iamcredentials.googleapis.com",
        prebaked: false,
    },
    Agent {
        id: "opencode",
        label: "Opencode (Open source)",
        method: InstallMethod::Pnpm,
        spec: "opencode-ai",
        cmd: "opencode",
        hosts: "opencode.ai models.dev openrouter.ai",
        prebaked: false,
    },
    Agent {
        id: "pi",
        label: "Pi agent (Open source)",
        method: InstallMethod::Pnpm,
        spec: "@earendil-works/pi-coding-agent",
        cmd: "pi",
        hosts: "console.anthropic.com openrouter.ai",
        prebaked: false,
    },
    Agent {
        id: "kilocode",
        label: "Kilocode CLI (kilo.sh)",
        method: InstallMethod::Pnpm,
        spec: "@kilocode/cli",
        cmd: "kilo",
        hosts: "kilo.ai api.kilo.ai",
        prebaked: false,
    },
];

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
}
