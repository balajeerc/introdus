# Running on a remote host

> Part of [introdus](../README.md#features). Push the container host onto a beefy remote box; keep your laptop thin.

The three-tier setup: your **dev machine** (laptop) drives a **container host** —
a remote Linux box (Hetzner, AWS, DO, Oracle Cloud, x86_64 or aarch64) running
**rootless podman** — which runs the **dev container**. You launch on the remote
host and attach [VS Code](vscode.md) from your laptop; task-completion
[notifications](notifications.md) tunnel back. **Nothing in the harness itself
changes** versus running it all on one machine.

## Prerequisites

- **On the host:** the [launch prerequisites](../README.md#prerequisites)
  (rootless podman + pasta + tmux). No podman socket to enable; egress is
  filtered [inside the container](egress-filtering.md).
- **On the laptop:** SSH key auth to the host without a password prompt
  (an `~/.ssh/config` alias is ideal), plus the **Remote - SSH** and **Dev
  Containers** VS Code extensions.
- A per-user `sshd` [forwarding allowance](notifications.md#host-ssh-forwarding-requirement)
  on the host — VS Code Remote-SSH needs local (`-L`) forwarding, which
  `AllowTcpForwarding no` blocks (you'll hang on "Waiting for port forwarding to
  be ready").

## Usage

### On the remote host (one-time)

```bash
cargo build --release
./target/release/introdus install    # copies introdus to ~/.local/bin
cd ~/myproject && introdus           # wizard, then launch
```

To forward notifications to your laptop, set `RC_FORWARD_ADDR` in that project's
config (see [Notifications](notifications.md)).

### On your laptop

1. Confirm `ssh my-remote-alias` drops you in without prompting.
2. Install **Remote - SSH**; F1 → **Remote-SSH: Connect to Host…** → your alias.
   The status bar reads `SSH: my-remote-alias`.
3. In that **remote** window, install **Dev Containers**.
4. F1 → **Dev Containers: Attach to Running Container…** →
   `introdus-<project>-<suffix>` — from this window's vantage point, podman is
   just local.
5. Install the **Claude Code** extension; it persists on the
   [volume](persistence-and-lifecycle.md).

No `docker.host`, `DOCKER_HOST`, `dev.containers.dockerPath`, or podman
connection plumbing on your laptop — the Remote-SSH layer handles every
cross-host call.

### Reaching the published ports

Ports bind to `127.0.0.1` on the remote host. Tunnel them over SSH:

```bash
ssh -L 8080:127.0.0.1:8080 -L 9090:127.0.0.1:9090 user@remote-host
```

Or use Tailscale / a WireGuard mesh. For a *public* webapp URL instead, see the
[webapp tunnel](webapp-tunnel.md).

## What feels different vs. local

- **First attach is slow** — VS Code pulls ~100MB of server over your
  laptop↔remote link, then caches it on the volume.
- **Terminal latency tracks your RTT** — same-continent is fine; transatlantic is
  usable but noticeable.
- **[Claude remote control](claude-remote-control.md) needs no tunnel** — it polls
  the Anthropic API over outbound HTTPS, so pairing works regardless of where the
  container runs.
