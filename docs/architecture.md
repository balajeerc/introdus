# Architecture

> Part of [introdus](../README.md#features). The shape of the system and where each piece lives.

![What lives on each tier: the dev machine, the container host, and the dev container, with the control and notification flows between them](img/architecture.svg)

## The three tiers

`introdus` is a Rust control plane that runs **on the container host** — where it
has full access to `.env`, `podman`, deploy keys, and `podman exec -u root`. It
drives everything through a persistent tmux session with a full-screen
[control panel](control-panel.md). The **dev container itself runs no Rust**: its
[security core](egress-filtering.md) is audited bash that `introdus` embeds and
bind-mounts at launch.

1. **Dev machine (laptop)** — attach [VS Code](vscode.md), drive
   [Claude](claude-remote-control.md), receive
   [notifications](notifications.md). Runs `introdus notify-listen`.
2. **Container host** — rootless podman + the `introdus` binary + the detached
   `notify-host` service.
3. **Dev container** — your repo clone + agents, with
   [egress filtering](egress-filtering.md) enforced *inside*.

Any tier can collapse onto one machine; running it all on one laptop and pushing
the host to a [remote box](remote-host.md) use the same control path.

## Launch flow

`introdus launch` (the default) runs, end to end: **preflight** (rootless podman
+ pasta + tmux) → **config** (typed [`Config`](setup-and-configuration.md), or
the wizard) → **context** (podman object names, per-container assets dir, the
generated proxy allowlist, resolved tunnel IPs) → **image** (build the shared
`introdus-base:latest` when stale, tag a per-project alias) → **lifecycle**
(cleanup / `--recreate` / `--reset`) → **run** (the full `podman run` set, then
hand off). Everything then lives inside **one tmux session per container**.

## Image and container naming

A local base image `introdus-base:latest` is built once from the
[Dockerfile](../Dockerfile) and reused across all projects (it bakes in apt
packages, `mise`, Node LTS, LazyVim — but **not** [agents](coding-agents.md)).
Each project's container is created from a cheap per-project *tag*
(`introdus-<project>-<suffix>:latest`, a `podman tag` alias — no rebuild), and
named `introdus-<project>-<suffix>`.

The 4-char `IMAGE_SUFFIX` is generated randomly per project by the wizard and
persisted in config, so it's stable across launches — and because each host runs
its own wizard, the **same project on two hosts gets two different suffixes**.
This keeps images, container names, and [VS Code](vscode.md) cached attach state
distinct across hosts (VS Code caches attach config by *both* image and container
name). Configs without `IMAGE_SUFFIX` fall back to a deterministic hash of
project name + hostname.

## What stays bash, and why

The security-critical container core is **not** rewritten in Rust — it's embedded
via `include_str!`, materialized, and bind-mounted at launch, so the egress
guarantee rests on the same battle-tested shell + nft + tinyproxy, and edits apply
on a plain relaunch with no image rebuild:

- [container/egress/firewall-entrypoint.sh](../container/egress/firewall-entrypoint.sh)
  — PID 1: nft default-deny + tinyproxy + [egress self-check](egress-filtering.md#startup-self-check), then drops privilege.
- [container/egress/tinyproxy.conf](../container/egress/tinyproxy.conf) — the hostname-allowlist proxy.
- [setup.sh](../setup.sh) — post-firewall: clone repo, run [launch hooks](launch-hooks.md), start the workload.
- [container/agents.sh](../container/agents.sh) — in-container [agent](coding-agents.md) installer (mirror of `agents.rs`).
- [container/bin/*](../container/bin/) — `run-claude`, `install-agents`, `rc-notify`, `egress-log`.

The [agent registry](coding-agents.md) exists in two hand-synced copies —
[agents.rs](../crates/introdus-core/src/agents.rs) (host-side) and
`container/agents.sh` (in-container) — **change both together**.

## Source map

Two Rust crates. Full file-by-file map:
[03_source-code-overview.md](../agent_rules/03_source-code-overview.md).

- **`introdus-core`** — pure library: typed config, host paths, podman naming,
  the embedded bash assets, the agent registry, egress-allowlist logic, the
  notification trust boundary, thin `podman`/`tmux`/process wrappers.
- **`introdus-cli`** — the `introdus` binary: clap CLI, launch orchestration, the
  ratatui TUI ([wizard](setup-and-configuration.md) +
  [control panel](control-panel.md)), the tmux session model, and the
  notification services.

## Config & persistence

A per-project `.introdus/config.env` is the on-disk source of truth (typed
`Config` ⇄ env via `dotenvy`). Host-side generated artifacts (the bind-mounted
allowlist, materialized bash core) live under `$XDG_STATE_HOME/introdus`;
per-project data (repo, toolchains, `node_modules`)
[persists](persistence-and-lifecycle.md) in a podman volume. The dev-machine
`notify-listen` settings live at `$XDG_CONFIG_HOME/introdus/notify-listen.env`.
