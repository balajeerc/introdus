# Coding agents

> Part of [introdus](../README.md#features). Pick which AI coding agents get installed — nothing is baked into the image.

You choose which coding agents to install per project — Claude, Codex,
Antigravity, Opencode, Pi, Kilocode. **Nothing is baked into the base image**, so
an agent you don't pick is genuinely absent, and installs are done with
supply-chain exposure minimized.

![The setup wizard's agent checklist, with install-method risk flags](img/wizard-agents.svg)

## Prerequisites

None beyond a launched container. Some agents need **extra egress hosts**; those
are appended to the [allowlist](egress-filtering.md) automatically when the agent
is selected (`AGENT_HOSTS` in
[container/agents.sh](../container/agents.sh)). If an agent's install or run hangs
on the network, check `egress-log` inside the container.

## Usage

Pick agents in the [setup wizard](setup-and-configuration.md)'s checklist, or set
`INSTALL_AGENTS` in [config](setup-and-configuration.md#configuration-reference)
(space-separated ids; `""` installs none):

```bash
INSTALL_AGENTS="claude codex"
# Valid ids: claude codex antigravity opencode pi kilocode
```

- Add an agent later and [`introdus update`](persistence-and-lifecycle.md#updates)
  installs it into the running container.
- Start one from the [control panel](control-panel.md) → "Launch an installed
  agent", or [install one](control-panel.md) → "Install a coding agent".
- Claude Code has [remote control](claude-remote-control.md) on by default so you
  can drive it from your phone.
- For a phone-first multi-agent workflow, add the [paseo](paseo.md) orchestrator.

## How it works — supply-chain posture

Install methods are declared per agent in
[agents.rs](../crates/introdus-core/src/agents.rs) (host-side, for the
wizard/launch) and mirrored in the in-container installer
[container/agents.sh](../container/agents.sh) — **change both together**:

| Method | Command | What runs |
| ------ | ------- | --------- |
| **`Pnpm`** (default for npm agents) | `pnpm add -g --ignore-scripts <spec>` | No package lifecycle scripts run. |
| **`PnpmBuild`** | `pnpm add -g --allow-build=<spec>` | The package's own postinstall *is* allowed — only claude-code, whose `install.cjs` places its native binary (shipped as an npm optionalDependency). Still registry-only; flagged in the wizard. |
| **`Script`** | `curl <spec> \| bash` | A vendor installer **not** contained by `--ignore-scripts` (e.g. Antigravity). Flagged as higher-risk in the wizard. |

Because agents run inside the [hardened](container-hardening.md),
[egress-filtered](egress-filtering.md) container, even a compromised agent or
dependency is confined to the container's blast radius — which is the entire
point of running them here. See
[05_security.md → supply-chain posture](../agent_rules/05_security.md).
