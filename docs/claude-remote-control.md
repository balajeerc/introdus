# Claude remote control

> Part of [introdus](../README.md#features). Drive Claude Code in the container from claude.ai/code or your phone.

Remote control is **on by default** for every Claude Code session in the
container, so you can pair it from a browser or the Claude mobile app and drive
the agent from anywhere — no inbound port, no tunnel.

## Prerequisites

- [Claude installed](coding-agents.md) as an agent (`claude` in
  `INSTALL_AGENTS`).
- Nothing else: remote control makes **outbound** HTTPS calls to the Anthropic
  API and polls for work — `api.anthropic.com` / `claude.ai` are already in the
  default [allowlist](egress-filtering.md).

## Usage

Start a Claude session:

- From the [control panel](control-panel.md) → "Launch an installed agent" (spawns
  it in its own tmux window), **or**
- directly with the bundled `run-claude` helper, run on the container host (over
  SSH if remote) or from a [VS Code](vscode.md) terminal attached to the
  container:

```bash
podman exec -it --user dev introdus-<project>-<suffix> run-claude
```

`--user dev` is required — the workload, its files under `/home/dev`, and its
per-uid tmux socket all belong to the non-root `dev` user. Drop into a shell the
same way with `bash` instead of `run-claude` (run `podman ps` for the exact
container name).

`run-claude` cds into the repo, opens a tmux session named `claude`, and launches
Claude Code with `--dangerously-skip-permissions`. Re-running it **re-attaches**
to the existing session instead of spawning a second one (`Ctrl-a d` detaches
without killing it; the container's tmux prefix is remapped `C-b` → `C-a` so it
doesn't collide with a host-side tmux you attach through).

**The first time**, pair the session from **claude.ai/code** or the **Claude
mobile app** — the pairing prompt appears in the session itself. Auth persists on
the [volume](persistence-and-lifecycle.md), so later launches don't re-pair, and
you can then drive the agent from your phone.

## How it works

[container/claude/settings.json](../container/claude/settings.json) sets
`"remoteControlAtStartup": true`, so the bridge registers automatically whenever
`claude` starts. It opens **no inbound port** — pairing and control flow entirely
over the outbound HTTPS poll, which is why remote control works identically
whether the container runs on your laptop or a [remote host](remote-host.md).

> The [VS Code](vscode.md) Claude extension starts its **own** `claude` and does
> not share state with the `run-claude` tmux session. To use that one, open a VS
> Code terminal and `tmux attach -t claude`.
