# introdus full-experience test harness

Drives the **real** introdus experience — `introdus launch` → tmux session →
rootless podman dev container → egress firewall → clone → live control TUI —
inside a **rootless podman-in-podman** container, and asserts on it. This is
what the fast `cargo test` suite can't reach: the actual container + tmux +
firewall behaviour, not just the pure logic.

It is a **heavy, opt-in tier** — not part of `cargo test`.

## Run it

```sh
test-harness/harness.sh            # all: verify + full menu drive (default)
test-harness/harness.sh verify     # egress spike only (fast-ish)
test-harness/harness.sh launch     # container up + clone through the proxy
test-harness/harness.sh menu       # drive the live control TUI over tmux
```

Requires a rootless-podman host with `/dev/fuse` and `/dev/net/tun`.

First run builds the Ubuntu base image nested (a few minutes); it's cached in
the `introdus-harness-storage` volume afterwards, so later runs are fast. To
force a clean rebuild: `podman volume rm introdus-harness-storage`.

## What each target proves

| Target | Proves |
|--------|--------|
| `verify` | Nested podman builds the base image; the egress firewall self-check passes inside the dev container (nft default-deny, tinyproxy allowlist, canary blocked, allowlisted host reachable via proxy, direct-IP dial dropped). |
| `launch` | The full dev container comes up nested and a small public repo clones **through** the (still-enforced) egress proxy → "up and running". |
| `menu` | `introdus launch` builds the real tmux session (main-control / notify / dev-container); the control menu renders live status + grouped sections; a Refresh shows the container running; "Open a dev terminal" spawns a `dev-bash` window that exec's into the container as `uid=1000(dev)`. |

## How it works

- **Image** (`Dockerfile`): `quay.io/podman/stable` (subuid/subgid,
  fuse-overlayfs preconfigured) + tmux/git/passt + the `introdus` release
  binary. A nested `containers.conf` sets `utsns=private` (introdus passes
  `--hostname`, rejected under the host UTS ns) and `default_sysctls=[]` (the
  `ping_group_range` default can't be written to the read-only `/proc` when
  nested).
- **Outer run flags** (`harness.sh`): `--privileged` so the inner container can
  mount its own `/proc` and set up namespaces, `--device /dev/fuse`,
  `--device /dev/net/tun` (pasta's tap device), and a storage volume to persist
  the built base image. The inner podman still runs **rootless** as the `podman`
  user, so introdus's non-root preflight and the egress firewall under test are
  unchanged. The outer (trusted) container has full egress by design.
- **XDG_RUNTIME_DIR** (`driver-common.sh`) lives under `/tmp` — the home overlay
  fs can't host pasta's netns bind-mounts; `/tmp` can.

## Clone mocking

The production clone is SSH deploy-key → `ProxyCommand`, which needs a key
registered on a real host (not hermetic). Instead the harness points `REPO_URL`
at a tiny **public** repo over HTTPS and, **test-only**, overlays `https_proxy`
onto the base image so git routes through the in-container proxy. The proxy
still enforces the hostname allowlist — this only selects the transport, so the
test still exercises real egress enforcement + a real clone. Override the repo
with `HARNESS_REPO_URL`.
