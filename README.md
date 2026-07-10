# remote-code-harness

## Overview

`remote-control-harness` runs your dev environment — including Claude Code —
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

![remote-control-harness architecture: dev machine to remote container host to hardened dev container](docs/architecture.svg)

## introdus — the single-binary control plane

The control plane is being consolidated into **`introdus`**, one self-contained
Rust binary that replaces the pile of host scripts (`launch_dev_container.sh`,
`create-dev-container.sh`, `host_install.sh`, the `host_listener.py` notifier).
The **container-side security core stays bash** — `introdus` embeds
`firewall-entrypoint.sh`, `tinyproxy.conf`, and `setup.sh` and bind-mounts them
at launch, exactly as before — so the egress guarantee is unchanged.

```bash
cargo build --release            # produces target/release/introdus (one binary)
./target/release/introdus install  # copies it onto ~/.local/bin (PATH)

mkdir ~/myproject && cd ~/myproject
introdus                         # first run: setup wizard writes .env, then launches
```

`introdus` puts each container inside one **tmux session** with a persistent
**control TUI** (`main-control` window) beside the container logs
(`dev-container` window) and the notification service (`notify` window). From
the control menu you can, on the host where it's actually possible: show the
tunnel URL, install/launch agents, list and add egress-allowlist hosts, open
root/dev terminals, copy files in, toggle the webapp tunnel / ntfy, and
recreate/reset — persisting to `.env` where it matters.

Subcommands: `introdus [launch]`, `up`, `menu`, `verify`, `recreate`, `reset`,
`update`, `rebuild-base`, `notify-host`, `notify-listen`, `install`.

> The legacy bash workflow below (`./launch.sh`, `create-dev-container.sh`,
> `host_install.sh`) still works and documents the same architecture in prose.
> See [PLAN.md](PLAN.md) for the rewrite's design and status.

## Highlights

Each item is tagged with **where** it runs — *dev machine* (your laptop),
*container host* (the box running podman; can be the same laptop), or *dev
container* (the hardened container itself). See the diagram above.

- **[Container host]** Runs your dev containers in rootless mode using podman, cloning your code into each one.
- **[Container host]** Linux rootless podman — no host firewall changes, no sudo; egress filtering runs entirely inside each container.
- **[Dev container]** Enforces egress rules that block the workload from reaching any non-whitelisted host: an in-container hostname-allowlist proxy backed by a default-deny nft filter, with a startup self-check that the filter is actually active.
- **[Dev container]** Persists container data (repo, `node_modules`, toolchains) across restarts on a per-project volume.
- **[Dev machine]** Attach VS Code to the running container ("Attach to Running Container") — directly when the host is local, or via Remote-SSH first when it's remote.
- **[Dev container]** Claude Code pre-installed (with the NodeJS + pnpm it needs), remote-control on by default so you can drive it from your phone.
- **[Dev container]** Optionally install additional coding agents — Codex, Antigravity, Opencode, Pi, Kilocode — chosen from a checklist in the setup wizard (`INSTALL_AGENTS` in `.env`). npm-published agents install with `pnpm add -g --ignore-scripts` to minimize supply-chain exposure; the registry lives in [container/agents.sh](container/agents.sh).
- **[Dev container]** LazyVim built in for a fully capable terminal editor (handy inside `podman exec -it --user dev <container> /bin/bash`).
- **[Dev container]** `mise` installed to set up any other toolchains you need.
- **[Container host → dev container]** Optionally mounts a host directory read-only into the container (`SHARED_DATA_PATH` in `.env`).
- **[Dev container]** Optional per-project launch hook on every container start — bring up your dev server, run migrations, warm caches, etc. (`ON_LAUNCH_SCRIPT` in `.env`).
- **[Container host]** Optionally publishes extra container ports to `127.0.0.1` for local tools (DBeaver against an in-container ClickHouse, a debugger attaching, `redis-cli`, etc.) via `EXTRA_PORTS` in `.env`.
- **[Container host]** Optionally exposes the webapp to the public internet via a Cloudflare quick tunnel (opt-in via `EXPOSE_WEBAPP` in `.env`).
- **[Container host]** On teardown, checks every repo for uncommitted / un-pushed work before taking the container down.
- **[Dev container → host → dev machine]** Built-in task-completion notifications: the container signals the host over a FIFO; when the host is remote, the host forwards to your laptop over an SSH reverse tunnel as a native popup + sound, each tagged with the project name so you can tell many containers apart.
- **[Container host → phone]** Optional mobile push via [ntfy.sh](https://ntfy.sh) alongside the desktop alert whenever Claude Code is awaiting input or finishes a task (opt-in via `ENABLE_NOTIFY_SH_ALERTS` + `NTFY_SH_TOPIC` in `.env`).

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
- `autossh` — required only if you want desktop notifications forwarded from a
  *remote* container host (`install_dev_machine_listener.sh` errors out without
  it). See [Notifications](docs/Notifications.md).

> If your laptop **is** the container host, you only need the host
> prerequisites above — everything renders and runs locally.

### Repo access

- A deploy key with commit + pull access to the repo you want to work on. It
  lives on the **container host** and is mounted read-only into the container
  for cloning.

## Usage

### Recommended: per-project flow

**On the container host** (the box that runs podman — a remote machine or your
laptop), run the host installer once after cloning, then bootstrap each project
in its own directory:

```bash
./host_install.sh                  # one-time per host: puts create-dev-container.sh
                                   # on PATH, sets up notifications + the listener
mkdir ~/myproject && cd ~/myproject
create-dev-container.sh            # walks through repo + deploy key, writes .env,
                                   # and launches the container
```

`host_install.sh` also asks whether this host should forward task-completion
notifications to another machine (e.g. a remote box reporting back to your
laptop). If so, run `install_dev_machine_listener.sh` once **on your dev
machine (laptop)** to receive them — see [Notifications](docs/Notifications.md).

### Direct: single project in place

```bash
cp sample.env .env
$EDITOR .env      # set PROJECT_NAME, REPO_URL, DEPLOY_KEY_PATH, etc.
./launch_dev_container.sh                    # rootless; egress filter runs inside the container
./launch_dev_container.sh path/to/other.env  # use a different env file
./launch_dev_container.sh --rebuild-base     # rebuild the base image
./launch_dev_container.sh --reset            # wipe the persistent volume
./launch_dev_container.sh --verify           # run an egress smoke test
./launch_dev_container.sh --update           # in-container refresh (see Updates below)
```

(`./launch.sh` still works — it's a back-compat symlink to
`launch_dev_container.sh`. The per-project flow above generates a `launch.sh`
wrapper in each project dir that calls the same engine.)

The egress filter lives inside the container, so there is nothing to tear down
on the host when you exit `claude` (or the container otherwise stops);
`launch_dev_container.sh` removes only the podman network / warmup slice it
installed.

## How egress hardening works

Egress filtering runs *inside* the container, not on the host. PID 1 is
`firewall-entrypoint.sh`, which starts as root with `CAP_NET_ADMIN`. It:

1. Installs an nft **default-deny** egress filter — only the proxy's uid may
   reach the internet.
2. Starts a loopback hostname-allowlist forward proxy (tinyproxy).
3. Runs an egress self-check to confirm the filter is actually active.
4. Drops `CAP_NET_ADMIN` and `exec`s the workload as the non-root **`dev`**
   user.

The workload — Claude Code and your code — therefore runs as non-root `dev`
with **no direct internet egress**. Its only way out is the proxy, which
permits only hostnames listed in `WHITELIST_HOSTS` (subdomain matches count;
the git host and `api.trycloudflare.com` are auto-added). A direct dial to a
raw IP is dropped, so a whitelisted host that shares a CDN IP with something
else is not a bypass. `INTERNAL_ALLOW_CIDRS` permits direct-IP access to fixed
internal targets. DNS stays open, which leaves DNS tunnelling as the residual
exfiltration channel. The workload cannot disable the firewall: it is non-root,
has no `NET_ADMIN`, and runs with `no-new-privileges`.

Concretely: git-over-SSH clones tunnel through the proxy (via an `ssh`
`ProxyCommand`), `apt` and HTTP(S) tools are proxy-configured, and
cloudflared's edge IPs are allowed directly by IP (they can't be proxied).

## More docs

### [Technical details](docs/Technical%20details.md)

- [Overview](docs/Technical%20details.md#overview) — the end-state that `./launch_dev_container.sh` produces (base image, volume, ports, egress allowlist, container posture).
- [Container capabilities](docs/Technical%20details.md#container-capabilities) — which caps are added back, why, and why it's safe.
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
