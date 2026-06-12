# Technical details

Back to the [project README](../README.md).

## Overview

The harness spans three roles: your **dev machine** (the laptop you sit at),
the **container host** (the box running podman — a remote machine or the same
laptop), and the **dev container** itself. This document is mostly about the
container host and the dev container; for the dev-machine side (VS Code,
notification listener) see [How to connect to container](How%20to%20connect%20to%20container.md)
and [Running on a remote host](Running%20on%20a%20remote%20host.md).

You run `./launch_dev_container.sh` (usually via the per-project wrapper, or
`create-dev-container.sh`) **on the container host**, and you end up with:

- A local base image (`remote-code-base:latest`) built once from the
  checked-in [Dockerfile](../Dockerfile) and reused across all projects.
  The image bakes in apt packages (`git`, `curl`, `openssh-client`,
  `ca-certificates`), `mise`, Node LTS, and Claude Code — nothing in the
  install chain needs to run at container start. Each project's container
  is then created from a cheap per-project *tag* of this image
  (`remote-code-<project>-<suffix>:latest`, a `podman tag` alias — no
  rebuild), and the container itself is named `remote-code-<project>-<suffix>`.
  VS Code Dev Containers caches its attach configuration under **both**
  `imageConfigs/` (keyed by image name) and `nameConfigs/` (keyed by container
  name), so a shared image name — or, for the same project on two hosts, a
  shared container name — made attaching to one container drag in another's
  cached workspace/remote context. The per-project suffix on both names keeps
  them separate. The 4-char `IMAGE_SUFFIX` is generated randomly per project by
  `create-dev-container.sh` and persisted in `.env`, so it's stable across
  launches — and because each host runs the wizard to build its own `.env`, the
  same project on two hosts gets two different suffixes, keeping their images,
  container names, and VS Code state distinct across hosts. Legacy `.env` files
  without `IMAGE_SUFFIX` fall back to a deterministic hash of project name +
  hostname. On each launch, stale per-project image tags (from a previous
  suffix or a `--rebuild-base`) are untagged automatically and a pre-upgrade
  un-suffixed container is removed (its volume is reused, so no data is lost);
  the shared base tag is kept since it's the build cache.
- The repo cloned inside the resulting container, on a per-project
  persistent volume so uncommitted work, branches, and `node_modules`
  survive across launches. The volume is seeded from the image's `/home/dev`
  on first mount, so mise/node/pnpm/claude come along for free.
- The project's dev webapp running under `nohup`, forwarded to
  `127.0.0.1:$WEBAPP_PORT` on the container host.
- Claude Code with remote control enabled by default
  (`"remoteControlAtStartup": true`), which you pair from claude.ai/code or
  the mobile app and drive from your phone. Start a session with the bundled
  `run-claude` helper. Remote control polls the Anthropic API over outbound
  HTTPS and opens no inbound port.
- A per-project egress filter enforced **inside the container** that drops
  every outbound connection except those that go through a loopback
  hostname-allowlist proxy reaching only the hostnames you named in
  `WHITELIST_HOSTS`. The host does no firewall work (see
  [Egress filtering](#egress-filtering)).

Container posture: `--cap-drop=ALL` with a narrow allowlist added back
(see [Container capabilities](#container-capabilities)), plus
`--cap-add=NET_ADMIN` used only by the entrypoint to install the in-container
nft filter and then dropped before the workload starts.
`no-new-privileges`, 8G RAM / 8 CPU / 16384 pids cap (override via
`MEM_LIMIT` / `CPU_LIMIT` / `PIDS_LIMIT` in `.env`). The workload itself runs
as the non-root `dev` user (uid 1000). The deploy key is mounted read-only and
scoped to the one repo by whoever issued it.

## Container capabilities

The container starts from `--cap-drop=ALL` and adds back only the
capabilities needed for `apt install` and typical service daemons to
work at runtime, plus `NET_ADMIN` — which is held only by the root PID 1
entrypoint long enough to install the in-container nft egress filter, then
dropped before the workload starts (see [Egress filtering](#egress-filtering)).
This keeps the rest of the dangerous caps (`SYS_ADMIN`,
`SYS_PTRACE`, `SYS_MODULE`, `SYS_TIME`, `SYS_RAWIO`, etc.) permanently
off — those are the ones that enable container escape or host-state
manipulation.

| Cap            | Why it's added                                                                                    | Why it's safe here                                                                                                                                                                                              |
| -------------- | ------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `CHOWN`        | `dpkg` chowns extracted files to their declared owner during `apt install`.                       | Namespace-scoped: inside a userns the cap only affects UIDs in the container's mapped range — can't chown anything to host-root.                                                                                |
| `DAC_OVERRIDE` | Lets root bypass DAC when writing to dirs it owns but with restrictive modes (apt cache, `/lib`). | Only affects files visible inside the container — the volume mount and image filesystem. No host paths are mounted rw.                                                                                          |
| `FOWNER`       | `dpkg` runs `chmod`/`utime` on files during package upgrades.                                     | Bounded to the container filesystem; same scope argument as `DAC_OVERRIDE`.                                                                                                                                     |
| `FSETID`       | Preserves setuid/setgid bits across `chown` (needed for `su`, `sudo`, `ping` to install cleanly). | `no-new-privileges` is still set, so any setuid bit that lands on disk can't actually elevate a descendant process. The cap just avoids install-time warnings and keeps binaries intact.                        |
| `SETFCAP`      | Lets `dpkg` install file capabilities on binaries (e.g., `ping` gets `cap_net_raw+ep`).           | You can only grant file caps that are already in your bounding set — the dangerous caps were never added, so they can't be written onto a binary.                                                               |
| `MKNOD`        | A handful of packages ship `/dev/*` entries; `dpkg` calls `mknod` while unpacking.                | Creating a device node doesn't grant access to the underlying hardware — the kernel still gates that on ownership, DAC, and the rest of the cap set.                                                            |
| `SETUID`       | Service daemons (ClickHouse, postgres, nginx, …) start as root and drop to a service user.        | Under a userns, `setuid` is limited to UIDs in the container's mapped subuid range — none map to host-root. Combined with `no-new-privileges`, setuid-root binaries on disk can't escalate a descendant either. |
| `SETGID`       | Same pattern as `SETUID` — daemons drop to a service group.                                       | Same bounding argument as `SETUID`.                                                                                                                                                                             |

Notable caps deliberately **not** added:

- `NET_RAW` — would let code inside craft raw packets; never added.
  `NET_ADMIN` *is* added at create time, but it belongs to the root
  entrypoint only: it installs the nft egress filter and is then dropped,
  so the non-root `dev` workload never holds it and cannot touch the filter
  (non-root + no `NET_ADMIN` + `no-new-privileges`).
- `SYS_ADMIN` — the "near-root" catch-all cap; enables mount, unshare,
  and a long tail of escape vectors.
- `SYS_PTRACE` — blocks attaching to processes outside the container's
  pid namespace, and to host processes via `/proc`.
- `NET_BIND_SERVICE` — nothing in the image needs to bind a port below
  1024. If a package does (e.g. a custom `httpd` on :80), add it
  explicitly.
- `IPC_LOCK` / `SYS_NICE` — ClickHouse's installer flags these as
  "optional" and runs fine without them. Taskstats accounting stays
  disabled, which only matters for in-depth observability.

## Egress filtering

The harness runs on **Linux rootless podman** with `--network=pasta`. Egress
is filtered **inside the container**, not on the host — the host runs no
nft/iptables, needs no `sudo`, and keeps no per-container firewall state.
Container escape lands on your UID, not root.

The container's PID 1 is `firewall-entrypoint.sh`, which starts as **root with
`CAP_NET_ADMIN`** and, before handing off to the workload:

1. **Installs an nft default-deny egress filter** in the container's own
   network namespace, segregated by uid. The `output` chain drops everything
   except: loopback, established/related, DNS to the container's resolvers,
   the fixed internal targets in `INTERNAL_ALLOW_CIDRS`, the cloudflared edge
   IPs on TCP/7844, and — crucially — traffic owned by the egress-proxy user
   `rcproxy` (uid 1001). The non-root `dev` workload (uid 1000) matches none
   of these, so it has **no direct internet egress**.
2. **Starts a loopback-only hostname-allowlist forward proxy** (`tinyproxy`,
   `127.0.0.1:8888`) running as `rcproxy`. It is `FilterDefaultDeny`, so only
   hostnames matching `WHITELIST_HOSTS` are reachable, and `CONNECT` tunnels
   are limited to ports 443, 563, and 22.
3. **Runs an egress self-check**: a canary IP must be unreachable directly
   (default-deny is enforcing), the proxy must reach an allowlisted host (the
   sanctioned path works), and a direct dial to that host's resolved IP must
   fail (a known CDN IP can't be used to bypass the proxy). Any failure aborts
   the launch.
4. **Drops `CAP_NET_ADMIN`** and `setpriv`-execs the workload (`/setup.sh`) as
   the non-root `dev` user with an empty bounding set.

The net result: the only way out for the workload is the loopback proxy,
gated by hostname. Dialing a raw IP — even a shared-CDN IP that also serves an
allowlisted host — is dropped, so IP-level bypass isn't available. Because the
workload is non-root, holds no `NET_ADMIN`, and runs under `no-new-privileges`,
it cannot modify the filter. DNS is left open, so a DNS-tunnel exfil vector
remains a residual risk.

**Config knobs:**

- `WHITELIST_HOSTS` — the proxy's hostname allowlist (default-deny). Each entry
  matches the host and its subdomains (e.g. `github.com` matches `api.github.com`
  but not `notgithub.com`). The git host and `api.trycloudflare.com` are added
  automatically.
- `INTERNAL_ALLOW_CIDRS` — direct-IP allow for fixed internal targets that
  should bypass the proxy entirely (a private registry, an internal API).
- DNS stays open by design.

**Escape hatch.** `--disable-network-block` (sets `DISABLE_NETWORK_BLOCK=true`)
skips the filter and the proxy entirely and runs the workload with unfiltered
egress.

## Persistence

The container is created once per project and reused across launches
(no `--rm`), so its writable overlay persists. Anything installed or
changed at runtime survives a restart:

- `apt install` packages (`/usr/bin`, `/etc/*`, `/var/lib/*`)
- service data dirs (e.g. `/var/lib/clickhouse`, `/var/log/*`)
- `/etc` edits and generated configs
- everything under `/home/dev` — the repo working tree (at
  `/home/dev/work/<project>`), `node_modules`, pnpm store
  (`~/.local/share/pnpm`), mise toolchains, claude config

`/home/dev` is additionally backed by a named podman volume
`remote-code-vol-$PROJECT_NAME`, so it survives even a full container
removal (see `--reset` below). The rest of the filesystem lives on the
container's overlay and only survives as long as the container itself.

**How config edits propagate.** `podman run` flags (capabilities,
volumes, env vars, published ports, `--memory`, `--cpus`) are frozen at
container-create time. `podman start` does not re-apply them. So editing
`launch_dev_container.sh` — adding a cap, bumping `MEM_LIMIT` via `.env`, exposing a
new port — does not affect an already-created container. To pick up
those changes, remove the container:

```bash
./launch.sh --recreate                    # removes container, keeps /home/dev volume
# or
podman rm -f remote-code-<project>-<suffix>   # same; see `podman ps` for the exact name
# or
./launch.sh --reset                       # removes container AND wipes /home/dev
```

The next launch creates a fresh container with the current flag set.
Runtime state (installed packages, `/var` contents) is lost in either
case, since it lived on the old overlay.

Before wiping, `--reset` scans `/home/dev/work` in a throwaway container and
reports any repo with uncommitted changes, commits not reachable from
any remote, or stashes. If anything turns up it pauses for a `y/N`
confirmation — default `no`, so you won't accidentally destroy
in-progress work.

**Tradeoff worth knowing.** Full-filesystem persistence means a
compromised session — a bad dep's postinstall, a modified `/usr/bin`, an
agent going off the rails — carries into subsequent launches until you
`--reset`. This is an intentional relaxation: the real security boundary
is the rootless userns plus the in-container egress filter (default-deny nft
+ hostname-allowlist proxy, with the non-root `dev` workload unable to touch
either), the read-only deploy key, and 127.0.0.1-only port binds — not the
container's internal rootfs. The container is treated as a trusted dev
environment, not as a sandbox-per-session.

`pnpm install` for the project's own dependencies runs on demand inside
the container, not at launch.

## Updates

`./launch.sh --update` runs an in-container refresh against a
*currently-running* container, without touching the image or the
container's identity. It executes, in order:

- `apt-get update && apt-get -y upgrade` — Ubuntu packages (security
  patches, etc.)
- `mise self-update` + `mise upgrade` — mise itself and every
  mise-managed toolchain (so `node@lts` and `pnpm@latest` roll forward)
- `pnpm update -g @anthropic-ai/claude-code` plus a manual replay of
  claude-code's `install.cjs` (pnpm v10 blocks postinstall on global
  adds, so the Dockerfile runs it by hand — same dance on update)
- `nvim --headless "+Lazy! sync" +qa` — LazyVim plugin updates

Run it from a second terminal while the container is up:

```bash
./launch.sh                    # terminal 1: launches and blocks
./launch.sh --update           # terminal 2: refreshes in-container
```

The update requires the container to be running because it routes
through the egress filter and proxy that the container's entrypoint
installed at startup; a stopped container has neither in place.

**What `--update` does *not* do:**

- It doesn't rebuild the base image. Dockerfile edits — new apt
  packages, a bumped nvim tarball version, a new `RUN` step — don't
  apply. For those, `./launch.sh --rebuild-base` followed by `--reset`
  is still the path. Container overlays are bound to their base layers
  and can't be rebased in place.
- It doesn't update image-level non-apt content: the nvim binary at
  `/opt/nvim-linux-{x86_64,arm64}`, the tree-sitter CLI at
  `/usr/local/bin/tree-sitter`, or the LazyVim starter config seeded at
  image build. These drift until `--rebuild-base` + `--reset`.
- It won't recover a broken state. If an `apt upgrade` half-completes,
  it's still your container.

In practice `--update` handles the security-patch / day-to-day-freshness
case; `--rebuild-base --reset` is the escape hatch for anything structural.

## Sharing host data (read-only)

Set `SHARED_DATA_PATH` in `.env` to an absolute host directory and
`launch_dev_container.sh` mounts it read-only at `/home/dev/shared_data` inside the
container. Use this to feed in datasets, reference material, or anything
else the container should read but never modify. Leave the var unset to
skip the mount.

## Adjusting the allowlist

If a package manager or tool inside the container hangs on a network
call, it's almost always a missing host in `WHITELIST_HOSTS`. Add the
hostname and relaunch.

The proxy filters by **hostname**, not by IP, so there's no IP re-resolution
to worry about: a CDN-fronted host that rotates its A records mid-session keeps
working, since the proxy only ever checks the hostname in the request. There is
no host-side re-resolve loop and no live filter to mutate. The allowlist is
regenerated from `WHITELIST_HOSTS` each time the container's entrypoint runs, so
a change takes effect on the next launch.

If a tool needs to reach a fixed internal target by IP (a private registry, an
internal API) rather than by hostname through the proxy, add its CIDR to
`INTERNAL_ALLOW_CIDRS` instead — those are allowed directly by the nft filter.

## What goes where

- [sample.env](../sample.env) — config template; copy to `.env`.
- [Dockerfile](../Dockerfile) — base image. Holds everything shared across
  projects: apt packages, mise, Node LTS, claude code. Built once by
  `launch_dev_container.sh`, reused for every project, rebuilt with
  `--rebuild-base`.
- [launch_dev_container.sh](../launch_dev_container.sh) — host side (the engine;
  a `launch.sh` symlink and the per-project wrapper both point here). Validates
  env, builds the base image if missing, assembles the proxy hostname allowlist
  (git host + `api.trycloudflare.com` added to `WHITELIST_HOSTS`), and runs the
  rootless `--network=pasta` container with `--cap-add=NET_ADMIN` so its
  entrypoint can install the in-container egress filter. The host itself does no
  firewall work and needs no `sudo`.
- [container/egress/firewall-entrypoint.sh](../container/egress/firewall-entrypoint.sh)
  — the container's PID 1. Runs as root with `CAP_NET_ADMIN`: installs the nft
  default-deny egress filter, starts the `tinyproxy` hostname-allowlist proxy,
  runs the egress self-check, then drops the cap and `setpriv`-execs `setup.sh`
  as the non-root `dev` user (see [Egress filtering](#egress-filtering)).
- [container/egress/tinyproxy.conf](../container/egress/tinyproxy.conf) — the
  loopback-only forward proxy config (`127.0.0.1:8888`, `FilterDefaultDeny`,
  `CONNECT` limited to 443/563/22).
- [host_install.sh](../host_install.sh) — one-time **container-host** setup:
  puts create-dev-container.sh on PATH, records notification forwarding
  (`RC_FORWARD_ADDR`), and installs the persistent rc-notify listener service.
- [create-dev-container.sh](../create-dev-container.sh) — run on the **container
  host**, per project: walks through repo + deploy key, writes the project
  `.env` + a `launch.sh` wrapper, and launches the container.

The notification path spans all three roles:

- [container/bin/rc-notify](../container/bin/rc-notify) — in the **dev
  container**. Claude's Stop/Notification hooks call it; it writes
  `event<TAB>project` to the host endpoint mounted at `/run/notify`.
- [host_listener.py](../host_listener.py) — on the **container host** it reads
  that endpoint and spawns host_notify.sh; on the **dev machine** (TCP mode,
  via `install_dev_machine_listener.sh`) it receives forwarded events and
  renders them.
- [host_notify.sh](../host_notify.sh) — on the **container host**: renders the
  desktop popup + sound locally, or — when `RC_FORWARD_ADDR` is set — forwards
  the event over the SSH reverse tunnel to the dev machine. Also fires the
  optional ntfy.sh push.
- [install_dev_machine_listener.sh](../install_dev_machine_listener.sh) — run on
  the **dev machine** (laptop): installs systemd `--user` units for the TCP
  listener + `ssh -R` reverse tunnel so notifications arrive as native alerts.
- [setup.sh](../setup.sh) — runs inside the container as the **non-root `dev`
  user**, exec'd by `firewall-entrypoint.sh` after the egress filter and proxy
  are up (the entrypoint stages the deploy key while still root). Clones the
  repo to `/home/dev/work/<project>` (or reuses an existing checkout),
  optionally starts a `tunnel` tmux session running `cloudflared` when
  `EXPOSE_WEBAPP=true` (see
  [Connecting from your phone](How%20to%20connect%20to%20container.md#connecting-from-your-phone)),
  runs `ON_LAUNCH_SCRIPT` if set, then blocks to keep the container alive.
  `podman exec -it ... run-claude` to start Claude Code (remote control on by
  default), or `podman exec -it ... bash` to get a shell. Every step is
  idempotent.

  Because the workload has no direct egress, proxy-aware tools are wired to the
  loopback proxy: git-over-SSH clones tunnel through it via an `ssh`
  `ProxyCommand` (`nc -X connect`, with `StrictHostKeyChecking=accept-new` to
  record the host key on first contact — `ssh-keyscan` would dial :22 directly
  and be dropped); `apt` uses an `apt.conf` proxy setting; and `HTTP(S)_PROXY`
  point HTTP-aware tools at it. `cloudflared`'s edge protocol can't go through
  an HTTP proxy, so its edge IPs (`TUNNEL_EDGE_IPS`) are allowed directly by IP
  on 7844 in the nft filter.
- [TODO.md](../TODO.md) — known limitations of the current hardening.

## Best practices

Configuring your .env file with the repo URL and deployment keys (which have access only to the repo you are working) on ensures that should your instance be compromised, the risk of container escape and access to your local machine is minimized. Which is a signficant concern when using claude code + remote control + the crazy state of the Node package ecosystem.
