# Attach VS Code

> Part of [introdus](../README.md#features). Edit inside the container with a full VS Code window.

VS Code's Dev Containers extension attaches to the running container via podman's
docker-compat socket. You get a window whose filesystem, terminal, and
extensions all live **inside** the container.

## Prerequisites

- VS Code with the **Dev Containers** extension (`ms-vscode-remote.remote-containers`).
- For a [remote host](remote-host.md), also the **Remote - SSH** extension
  (`ms-vscode-remote.remote-ssh`).
- The VS Code marketplace + server download hosts are already in the default
  [allowlist](egress-filtering.md) (`update.code.visualstudio.com`,
  `vscode.download.prss.microsoft.com`, `marketplace.visualstudio.com`) so the
  first attach — which pulls ~100MB of VS Code server — works through the egress
  proxy.

## Usage

### Local container host

If you launched the container locally, the only setting your VS Code needs is:

```jsonc
"dev.containers.dockerPath": "podman"
```

Then: Command Palette → **Dev Containers: Attach to Running Container…** → pick
`introdus-<project>-<suffix>` (run `podman ps` for the exact name — the
per-project suffix keeps each project's cached attach config distinct).

### Remote container host

Don't try to expose podman's socket over SSH. Use **Remote-SSH first, then
attach** — from the remote window's perspective the container is local. Full
walkthrough in [Running on a remote host](remote-host.md).

### After attaching

Install the **Claude Code** extension in the new window — it lands in
`/home/dev/.vscode-server/extensions/` on the
[persistent volume](persistence-and-lifecycle.md) and survives across launches.
Any extension/VSIX pulls through the same egress proxy, so its hostnames must be
in `WHITELIST_HOSTS` (the marketplace hosts already are).

## How it works

Dev Containers caches its attach config under **both** `imageConfigs/` (keyed by
image name) and `nameConfigs/` (keyed by container name). A shared name in either
would make it confuse one project's container for another's — including the same
project on two hosts. The per-project `<suffix>` on both the image tag and the
container name (`IMAGE_SUFFIX`, random per project per host) keeps them separate.
See [architecture → naming](architecture.md#image-and-container-naming).

The extension starts its **own** `claude`, separate from the `run-claude` tmux
session — see [Claude remote control](claude-remote-control.md).
