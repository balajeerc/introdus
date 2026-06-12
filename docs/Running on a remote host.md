# Running on a remote host

Back to the [project README](../README.md).

This is the three-tier setup: your **dev machine** (laptop) drives a **container
host** — a remote Linux box (Hetzner, AWS, DO, Oracle Cloud, etc., including
aarch64) running **rootless podman** — which runs the **dev container**. You launch the container on the
remote container host and attach VS Code to it from your laptop; task-completion
notifications tunnel back to the laptop. Nothing in the harness itself changes
versus running it all on one machine.

The recommended VSCode flow is **Remote-SSH first, then attach to
container**. From the remote VSCode window's perspective, the container
is local, so you skip the entire dance of exposing podman's socket over
SSH (which used to rely on the now-deprecated `docker.host` setting and
podman's built-in SSH client that doesn't read `~/.ssh/config`).

## On the remote container host (one-time)

1. Install the [prerequisites](../README.md#prerequisites): rootless podman
   and pasta. Egress is filtered **inside** the container (a default-deny nft
   filter plus a hostname proxy), so the host does no firewall work and needs
   no sudo for it.
2. Clone this repo and run `./host_install.sh` — answer **yes** to forwarding
   notifications and pick a port. Then `create-dev-container.sh` per project
   (or, for the single-project flow, copy `sample.env` to `.env` and run
   `./launch_dev_container.sh`).

That's it on the host — no podman socket to enable.

## On your laptop

1. Confirm SSH key auth works without a password prompt. If you use an
   alias in `~/.ssh/config`, it just works:

   ```bash
   ssh my-remote-alias    # should drop you in without prompting
   ```

2. Install VSCode's **Remote - SSH** extension
   (`ms-vscode-remote.remote-ssh`).

3. F1 → **Remote-SSH: Connect to Host…** → pick your alias. A new VSCode
   window opens; the status bar bottom-left reads `SSH: my-remote-alias`.

4. In that remote window (not your local one), install **Dev Containers**
   (`ms-vscode-remote.remote-containers`). It installs on the remote and
   is independent of any Dev Containers install on your laptop.

5. F1 → **Dev Containers: Attach to Running Container…** →
   `remote-code-<project>-<suffix>` shows up because, from this VSCode's
   vantage point, podman is just local. The per-project `<suffix>` (distinct
   per host) ensures this remote container's name doesn't collide with a
   same-named project's container on your laptop, so VS Code keeps their
   attach configs separate.

6. Once attached, install the **Claude Code** extension. It lands in
   `/home/dev/.vscode-server/extensions/` on the persistent volume and
   survives across launches (same as local mode).

No `docker.host`, `DOCKER_HOST`, `dev.containers.dockerPath`, or podman
connection plumbing is needed on your laptop. The Remote-SSH layer
handles every cross-host call.

> **VS Code Remote-SSH needs local (`-L`) forwarding**, which hardened hosts
> block with `AllowTcpForwarding no` (you'll hang on "Waiting for port
> forwarding to be ready"). The same per-user `sshd` allowance that enables
> notifications covers this — see
> [Notifications → host SSH-forwarding requirement](Notifications.md#host-ssh-forwarding-requirement).

## Reaching the published ports

`launch_dev_container.sh` binds the webapp port to `127.0.0.1` on the remote host, not
`0.0.0.0` — same posture as the local-only setup described in
[How to connect to container.md](How%20to%20connect%20to%20container.md#connecting-from-your-phone).
To reach it from your laptop's browser or phone, tunnel it over SSH:

```bash
ssh -L 8080:127.0.0.1:8080 -L 9090:127.0.0.1:9090 user@remote-host
```

Or use Tailscale / a WireGuard mesh if you'd rather not keep an SSH
session open.

## Task-completion notifications back to your laptop

When the container host is remote rather than your laptop, "task complete /
awaiting input" alerts tunnel back to a native popup + sound on your laptop over
an SSH reverse tunnel. Setup is one command on the host (`./host_install.sh`,
answer yes to forwarding) and one on the laptop
(`./install_dev_machine_listener.sh <ssh-alias> 8765`), plus a small per-user
`sshd` forwarding allowance. The full mechanics, the exact SSH config, the
per-container label, and the optional ntfy phone-push live in
[Notifications](Notifications.md).

## Things that feel different vs. local

- **First attach is slow.** VSCode pulls ~100MB of server into the
  container over your laptop↔remote link. It's cached in the persistent
  volume after that, so subsequent attaches are quick.
- **Terminal latency tracks your RTT.** A laptop in the US talking to a
  Hetzner Helsinki box is usable but noticeable; same-continent is fine.
- **Claude remote control needs no tunnel.** It polls the Anthropic API
  over outbound HTTPS, so you can pair from claude.ai/code or the mobile
  app regardless of where the container runs — no port to forward.
