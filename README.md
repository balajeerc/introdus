# introdus

## Overview

`introdus` runs your dev environment — including Claude Code —
inside a network-hardened, rootless **podman** container. Your real machine
stays insulated from supply-chain attacks (increasingly common), and even a
full compromise is confined to the container's blast radius.

It spans up to three tiers, and any of them can collapse onto a single machine:

1. **Your dev machine** — the laptop you sit at. Attach VS Code over SSH, drive
   Claude Code, and receive native desktop task-completion notifications.
2. **Container host** — a remote KVM/VPS ([Hetzner/AWS/DigitalOcean](docs/Running%20on%20a%20remote%20host.md))
   or the same laptop. Runs rootless podman and the notification listener.
3. **Dev container** — your repo clone + Claude Code, with egress filtering
   enforced *inside* the container so it can only reach whitelisted hosts.

Run everything on one laptop, or push the container host onto a beefy remote box
and keep your laptop thin — the same control path and task-completion
notifications work either way. When the host is remote, you still drive Claude
from your laptop over SSH, and "task done / awaiting input" alerts tunnel back to
a popup + sound on your laptop (see
[Running on a remote host](docs/Running%20on%20a%20remote%20host.md)).

![introdus architecture: dev machine to remote container host to hardened dev container](docs/architecture.svg)

## introdus — the single-binary control plane

`introdus` is one self-contained Rust binary that runs on the container host and
drives everything: podman, the tmux session, the control TUI, and the
notification service. The **container-side security core stays bash** —
`introdus` embeds `firewall-entrypoint.sh`, `tinyproxy.conf`, `setup.sh`, and
`container/bin/*` and bind-mounts them at launch, so the egress guarantee rests
on audited shell + nft + tinyproxy.

```bash
cargo build --release            # produces target/release/introdus (one binary)
./target/release/introdus install  # copies it onto ~/.local/bin (PATH)

mkdir ~/myproject && cd ~/myproject
introdus                         # first run: setup wizard writes .introdus/config.env, then launches
```

`introdus` puts each container inside one **tmux session** with a persistent
**control TUI** (`main-control` window) beside the container logs
(`dev-container` window); the notification service runs detached in the
background. From the control menu you show the tunnel URL, install/launch
agents, list and add egress-allowlist hosts, open root/dev terminals, copy files
in, toggle the webapp tunnel / ntfy, and recreate/reset — persisting to `.env`
where it matters.

Subcommands: `introdus [launch]`, `up`, `menu`, `verify`, `recreate`, `reset`,
`update`, `rebuild-base`, `notify-host`, `notify-listen`, `send-files`,
`install`.

`introdus send-files` (run on your laptop) is a two-pane file transfer TUI: pick
a container running on this machine or on a host from your `~/.ssh/config`, then
browse the laptop filesystem beside the container's and send a file/folder into a
chosen directory (`podman cp` locally, a tar-stream over ssh for a remote host).

## Highlights

### Container host

- Runs your dev containers in rootless mode using podman, cloning your code into each one.
- Linux rootless podman — no host firewall changes, no sudo; egress filtering runs entirely inside each container.
- Optionally mounts a host directory read-only into the container (`SHARED_DATA_PATH` in `.env`).
- Optionally publishes extra container ports to `127.0.0.1` for local tools (DBeaver against an in-container ClickHouse, a debugger attaching, `redis-cli`, etc.) via `EXTRA_PORTS` in `.env`.
- Optionally exposes the webapp to the public internet via a Cloudflare quick tunnel (opt-in via `EXPOSE_WEBAPP` in `.env`).
- On teardown, checks every repo for uncommitted / un-pushed work before taking the container down.

### Dev container

- Enforces egress rules that block the workload from reaching any non-whitelisted host: an in-container hostname-allowlist proxy backed by a default-deny nft filter, with a startup self-check that the filter is actually active.
- Persists container data (repo, `node_modules`, toolchains) across restarts on a per-project volume.
- Choose which coding agents to install — Claude, Codex, Antigravity, Opencode, Pi, Kilocode — from a checklist in the setup wizard (`INSTALL_AGENTS` in `.env`). Nothing is baked into the base image, so an agent you don't pick isn't installed. npm-published agents install with `pnpm add -g --ignore-scripts` to minimize supply-chain exposure (claude uses `--allow-build` so its postinstall can place its native binary, shipped as an npm optionalDependency); the registry lives in [container/agents.sh](container/agents.sh).
- Claude Code, when picked, has remote-control on by default so you can drive it from your phone.
- Optional per-project launch hook on every container start — bring up your dev server, run migrations, warm caches, etc. (`ON_LAUNCH_SCRIPT` in `.env`).
- LazyVim built in for a fully capable terminal editor (handy inside `podman exec -it --user dev <container> /bin/bash`).
- `mise` installed to set up any other toolchains you need.

### Dev machine

- Attach VS Code to the running container ("Attach to Running Container") — directly when the host is local, or via Remote-SSH first when it's remote.

### Notifications

- Built-in task-completion notifications: the container signals the host over a FIFO; when the host is remote, the host forwards to your laptop over an SSH reverse tunnel as a native popup + sound, each tagged with the project name so you can tell many containers apart.
- Optional mobile push via [ntfy.sh](https://ntfy.sh) alongside the desktop alert whenever an agent is awaiting input or finishes a task (opt-in via `ENABLE_NOTIFY_SH_ALERTS` + `NTFY_SH_TOPIC` in `.env`).

## Prerequisites

### On the container host (where podman runs — a remote box or your laptop)

- **Linux rootless podman** (the only supported configuration): `podman` 4.4+,
  `pasta` (`apt install passt`), kernel ≥ 5.x with cgroup v2. No host nftables,
  no systemd-host requirement, and no sudo — the egress filter is installed and
  enforced inside each container, not on the host.
- **SSH forwarding** is needed only if the container host is a **separate box**
  and you want desktop notifications on your laptop and/or VS Code Remote-SSH —
  hardened hosts often ship `AllowTcpForwarding no`, which blocks both. The
  exact (narrow) `sshd` allowance is in
  [Notifications → host SSH-forwarding requirement](docs/Notifications.md#host-ssh-forwarding-requirement).
  Not needed when the container host is your laptop.

### On your dev machine (laptop) — only if it's separate from the host

- An SSH client with key access to the container host (you already use this
  to reach the box). Use a passphrase-less key, or an agent reachable from your
  `systemd --user` session — the tunnel runs with `BatchMode=yes`.
- VS Code with the **Remote-SSH** and **Dev Containers** extensions, to edit
  inside the container.
- `autossh` (recommended) — for a self-healing `ssh -R` reverse tunnel when you
  want desktop notifications forwarded from a *remote* container host to your
  laptop, alongside `introdus notify-listen`. See
  [Notifications](docs/Notifications.md).

> If your laptop **is** the container host, you only need the host
> prerequisites above — everything renders and runs locally.

### Repo access

- A deploy key with commit + pull access to the repo you want to work on. It
  lives on the **container host** and is mounted read-only into the container
  for cloning.

## Usage

Build the binary once and put it on `PATH` (do this **on the container host** —
the box that runs podman, a remote machine or your laptop), then work
per-project:

```bash
cargo build --release              # -> target/release/introdus
./target/release/introdus install  # copies it onto ~/.local/bin (PATH)

mkdir ~/myproject && cd ~/myproject
introdus                           # first run: setup wizard writes .introdus/config.env, then launches
```

The first `introdus` in a project directory runs the **setup wizard** (repo +
deploy key + agent picks), writes the project's `.introdus/config.env`, and
launches. Later runs attach straight to that project's tmux session + control
TUI. You can also write the config by hand from [sample.env](sample.env) and skip
the wizard. (Projects from before this move keep a top-level `.env`; introdus
reads it and offers to relocate it into `.introdus/` on the next launch.)

Other subcommands, run from the project directory:

```bash
introdus                    # launch (or re-attach to) the session + control TUI
introdus recreate           # drop the container, keep the /home/dev volume
introdus reset              # drop the container AND wipe the volume
introdus rebuild-base       # rebuild the shared base image
introdus verify             # egress smoke test in a throwaway container
introdus update             # in-container refresh (apt, mise, agents, LazyVim)
introdus init               # (re)write the config via the wizard, without launching
introdus --disable-network-block   # launch with NO egress filtering (debug only)
```

Most of these are also on the persistent control menu, so day-to-day you drive
them from the TUI rather than the CLI. When the container host is remote and you
want task-completion alerts on your laptop, set `RC_FORWARD_ADDR` in `.env` and
run `introdus notify-listen` on the laptop — see
[Notifications](docs/Notifications.md) and
[Running on a remote host](docs/Running%20on%20a%20remote%20host.md).

The egress filter lives inside the container, so there is nothing to tear down
on the host when the container stops; `introdus` removes only the podman
network / warmup slice it installed.

## Egress hardening

Egress filtering runs *inside* the container, not on the host. PID 1
(`firewall-entrypoint.sh`) installs an nft **default-deny** filter plus a
loopback hostname-allowlist proxy, self-checks that the filter is actually
enforcing, then drops `CAP_NET_ADMIN` and runs the workload as the non-root
`dev` user. That workload has **no direct internet egress** — its only way out is
the proxy, which permits only the hostnames in `WHITELIST_HOSTS`, and it can't
disable the filter (non-root, no `NET_ADMIN`, `no-new-privileges`).

Full mechanics — the uid-segregated nft rules, IP-bypass protection, the
DNS-tunnelling residual, and how git/apt/cloudflared reach the network — are in
[Technical details → Egress filtering](docs/Technical%20details.md#egress-filtering).

## More docs

### [Technical details](docs/Technical%20details.md)

- [Overview](docs/Technical%20details.md#overview) — the end-state that `introdus` produces (base image, volume, ports, egress allowlist, container posture).
- [Container capabilities](docs/Technical%20details.md#container-capabilities) — which caps are added back, why, and why it's safe.
- [Egress filtering](docs/Technical%20details.md#egress-filtering) — the in-container default-deny nft filter + hostname-allowlist proxy, the startup self-check, and IP-bypass protection.
- [Persistence](docs/Technical%20details.md#persistence) — what survives restarts, how config edits propagate, `--reset` semantics.
- [Updates](docs/Technical%20details.md#updates) — what `--update` refreshes and what it deliberately won't touch.
- [Sharing host data (read-only)](docs/Technical%20details.md#sharing-host-data-read-only) — `SHARED_DATA_PATH`.
- [Adjusting the allowlist](docs/Technical%20details.md#adjusting-the-allowlist) — when a tool hangs on network.
- [What goes where](docs/Technical%20details.md#what-goes-where) — file-by-file map of the harness.
- [Best practices](docs/Technical%20details.md#best-practices) — deploy-key scoping.

### [How to connect to container](docs/How%20to%20connect%20to%20container.md)

- [Connecting from your phone](docs/How%20to%20connect%20to%20container.md#connecting-from-your-phone) — tunneling the 127.0.0.1-bound ports.
- [Connecting from VSCode](docs/How%20to%20connect%20to%20container.md#connecting-from-vscode) — Dev Containers extension + the podman socket.
- [Claude remote-control](docs/How%20to%20connect%20to%20container.md#claude-remote-control) — `run-claude`, on-by-default remote control, pairing from your phone.

### [Running on a remote host](docs/Running%20on%20a%20remote%20host.md)

- Launching the harness on a remote Linux box (Hetzner/AWS/DO, x86_64 or aarch64) and attaching to it from your laptop's VSCode over SSH.

### [Notifications](docs/Notifications.md)

- How "task complete / awaiting input" reaches you — local render vs. the two-hop SSH reverse tunnel to your laptop, the [host SSH-forwarding requirement](docs/Notifications.md#host-ssh-forwarding-requirement), per-container labels, ntfy phone push, and the best-effort limitations.
