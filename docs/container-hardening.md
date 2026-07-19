# Container hardening

> Part of [introdus](../README.md#features). Rootless podman, a minimal capability set, and a scoped deploy key.

Beyond [egress filtering](egress-filtering.md), the container is built so that a
compromise stays confined: it runs rootless, drops almost every Linux
capability, blocks privilege escalation, and mounts only a narrowly-scoped deploy
key.

## Prerequisites

Rootless podman (the only supported configuration) — see the
[launch prerequisites](../README.md#prerequisites). Nothing else; there are no
tunable knobs here, this is the default posture.

## How it works

### Rootless podman — host isolation

Containers run under **rootless podman**: no host nftables changes, no
systemd-host requirement, no sudo. Root-in-container maps to your unprivileged
host uid, so a container breakout lands as a **non-privileged host user**, not
root. The [egress filter](egress-filtering.md) is installed and enforced *inside*
each container, so there's nothing to tear down on the host and the host firewall
is never touched.

### The capability set

The container starts from `--cap-drop=ALL` and adds back only what `apt install`
and typical service daemons need at runtime, plus `NET_ADMIN` — which is held
**only** by the root PID 1 entrypoint long enough to install the nft egress
filter, then dropped before the workload starts.

| Cap | Why it's added | Why it's safe |
| --- | -------------- | ------------- |
| `CHOWN` | `dpkg` chowns extracted files during `apt install`. | Namespace-scoped — only affects UIDs in the container's mapped range. |
| `DAC_OVERRIDE` / `FOWNER` / `FSETID` | `dpkg` writes/chmods/preserves setuid bits during package ops. | Bounded to the container filesystem; no host paths are mounted rw. `no-new-privileges` neuters any setuid bit. |
| `SETFCAP` | `dpkg` installs file caps (e.g. `ping` → `cap_net_raw+ep`). | You can only grant caps already in the bounding set — the dangerous ones were never added. |
| `MKNOD` | A few packages `mknod` `/dev/*` entries while unpacking. | A device node doesn't grant hardware access — the kernel still gates that. |
| `SETUID` / `SETGID` | Daemons (ClickHouse, postgres, nginx …) start as root and drop to a service user. | Under a userns, limited to mapped subuids — none map to host-root; `no-new-privileges` blocks escalation. |

Deliberately **not** added: `NET_RAW`, `SYS_ADMIN` (the "near-root" catch-all),
`SYS_PTRACE`, `NET_BIND_SERVICE`, `IPC_LOCK`/`SYS_NICE` — the caps that enable
container escape or host-state manipulation.

### Runtime posture

`no-new-privileges` is set (so setuid-root binaries on disk can't escalate a
descendant), the workload runs as the non-root **`dev`** user (uid 1000), and
resource caps default to **8G RAM / 8 CPU / 16384 pids** (override with
`MEM_LIMIT` / `CPU_LIMIT` / `PIDS_LIMIT`). All published ports bind to
`127.0.0.1` only — never `0.0.0.0`.

### Deploy-key handling

A per-project deploy key lives on the **host**, mounted read-only, and is copied
into `dev`'s `~/.ssh` at startup (mode 600, dev-owned) while the entrypoint is
still root. **Scope it to the single repo.** Because a compromise is confined to
the container, the worst an attacker can do with the key is what the key itself
permits — one repo — which is why scoping it tightly matters when you pair Claude
Code + remote control with the Node package ecosystem.
[`introdus reset`](persistence-and-lifecycle.md) / `destroy` delete the local key
on teardown.

### The threat model

The container host and your laptop are **trusted**; `introdus` runs there with
full privilege by design. The security boundary is the rootless userns + the
[in-container egress filter](egress-filtering.md) + the read-only deploy key +
127.0.0.1-only port binds — **not** the container's internal rootfs (which
[persists](persistence-and-lifecycle.md) across launches). The full write-up,
including residual risks, is in
[05_security.md](../agent_rules/05_security.md).
