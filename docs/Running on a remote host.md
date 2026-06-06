# Running on a remote host

Back to the [project README](../README.md).

The harness runs identically on a remote Linux box (Hetzner, AWS, DO,
Oracle Cloud, etc.) — including aarch64 instances. You launch the
container on the remote host and attach VSCode to it from your laptop.
Nothing in the harness itself changes.

The recommended VSCode flow is **Remote-SSH first, then attach to
container**. From the remote VSCode window's perspective, the container
is local, so you skip the entire dance of exposing podman's socket over
SSH (which used to rely on the now-deprecated `docker.host` setting and
podman's built-in SSH client that doesn't read `~/.ssh/config`).

## On the remote host (one-time)

1. Install the [prerequisites](../README.md#prerequisites) for your launch
   mode (rootless is recommended for remote — fewer SSH-side gotchas).
2. Clone this repo, copy `sample.env` to `.env`, fill it in, and run
   `./launch.sh` the same way you would locally.

That's it on the remote — no podman socket to enable, no extra services.

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
   `remote-code-<project>` shows up because, from this VSCode's vantage
   point, podman is just local.

6. Once attached, install the **Claude Code** extension. It lands in
   `/root/.vscode-server/extensions/` on the persistent volume and
   survives across launches (same as local mode).

No `docker.host`, `DOCKER_HOST`, `dev.containers.dockerPath`, or podman
connection plumbing is needed on your laptop. The Remote-SSH layer
handles every cross-host call.

## Reaching the published ports

`launch.sh` binds the webapp port to `127.0.0.1` on the remote host, not
`0.0.0.0` — same posture as the local-only setup described in
[How to connect to container.md](How%20to%20connect%20to%20container.md#connecting-from-your-phone).
To reach it from your laptop's browser or phone, tunnel it over SSH:

```bash
ssh -L 8080:127.0.0.1:8080 -L 9090:127.0.0.1:9090 user@remote-host
```

Or use Tailscale / a WireGuard mesh if you'd rather not keep an SSH
session open.

## Things that feel different vs. local

- **First attach is slow.** VSCode pulls ~100MB of server into the
  container over your laptop↔remote link. It's cached in the persistent
  volume after that, so subsequent attaches are quick.
- **Terminal latency tracks your RTT.** A laptop in the US talking to a
  Hetzner Helsinki box is usable but noticeable; same-continent is fine.
- **Claude remote control needs no tunnel.** It polls the Anthropic API
  over outbound HTTPS, so you can pair from claude.ai/code or the mobile
  app regardless of where the container runs — no port to forward.
