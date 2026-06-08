# Running on a remote host

Back to the [project README](../README.md).

This is the three-tier setup: your **dev machine** (laptop) drives a **container
host** — a remote Linux box (Hetzner, AWS, DO, Oracle Cloud, etc., including
aarch64) — which runs the **dev container**. You launch the container on the
remote container host and attach VS Code to it from your laptop; task-completion
notifications tunnel back to the laptop. Nothing in the harness itself changes
versus running it all on one machine.

The recommended VSCode flow is **Remote-SSH first, then attach to
container**. From the remote VSCode window's perspective, the container
is local, so you skip the entire dance of exposing podman's socket over
SSH (which used to rely on the now-deprecated `docker.host` setting and
podman's built-in SSH client that doesn't read `~/.ssh/config`).

## On the remote container host (one-time)

1. Install the [prerequisites](../README.md#prerequisites) for your launch
   mode (rootless is recommended for remote — fewer SSH-side gotchas).
2. Clone this repo and run `./host_install.sh` — answer **yes** to forwarding
   notifications and pick a port. Then `create-dev-container.sh` per project
   (or, for the single-project flow, copy `sample.env` to `.env` and run
   `./launch.sh`).

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

## Task-completion notifications back to your laptop

The harness's desktop notification (popup + sound on "task complete" /
"awaiting input") normally renders on the same machine as the container. On a
remote host there's no desktop to render to, so the signal has to make a
second hop back to your laptop. There are two hops in total:

```
[dev container] --FIFO--> [remote host] host_notify.sh
    --(TCP to 127.0.0.1:PORT)--> (ssh -R reverse tunnel)
        --> [laptop] host_listener.py --> popup + sound
```

The first hop (container → remote host) is the harness's existing FIFO
transport and is unchanged. The second hop (remote host → laptop) rides an
SSH **reverse** tunnel: your laptop dials *out* to the remote host (the same
SSH you already use), so nothing needs an inbound port — which matters when
your laptop is behind NAT.

This is **host-level**, not per-container: one host relays every container's
events to one laptop through a single listener. So the setting lives in the
**harness** `.env` (the one next to `host_notify.sh`), not a per-project
`.env`. `host_install.sh` asks "forward to another machine?" and, if yes,
writes this line to the harness `.env` for you and installs the persistent
listener service.

**On the remote host**, this is what `host_install.sh` records in the harness
`.env` (you can also set it by hand):

```bash
RC_FORWARD_ADDR=127.0.0.1:8765
```

`host_notify.sh` now forwards each validated event to that loopback port
instead of trying to render. The event whitelist runs here and again on the
laptop, and the container label is stripped to `[A-Za-z0-9._-]` (max 40
chars) before it reaches any notification — a compromised container can't
spoof arbitrary text or inject control characters under the "Claude Code"
brand.

**On your laptop**, from your checkout of this repo, install the always-on
services (recommended):

```bash
./install_dev_machine_listener.sh <ssh-alias-for-remote-host> 8765
```

This installs two `systemd --user` units that survive reboot and sleep:

- `rc-notify-listener.service` — `host_listener.py` in TCP mode, which renders
  the native popup + plays `notification_sound.wav`.
- `rc-notify-tunnel.service` — the `ssh -R` reverse tunnel to your alias
  (autossh if installed, for self-healing reconnects).

The port must match `RC_FORWARD_ADDR`. The alias must accept key-based SSH
without a prompt (passphrase-less key, or an agent reachable from your
`systemd --user` session) since the tunnel runs with `BatchMode=yes`. Manage
with `systemctl --user status rc-notify-tunnel.service` and remove with
`./install_dev_machine_listener.sh --uninstall`.

For a quick foreground tunnel without systemd (e.g. a one-off), use
`./laptop_notify_tunnel.sh <ssh-alias> 8765` instead.

### Which container fired it?

Each notification's title is suffixed with the container's project name —
e.g. *"Claude Code — myproject"* — so when you run many containers on one
host you can tell them apart at a glance. (Derived from `PROJECT_NAME`;
override per-container with the `RC_LABEL` env var.)

### Limitations

This path is **best-effort, fire-and-forget**: if the laptop is offline or
the tunnel is mid-reconnect when an event fires, that desktop notification is
dropped (there's no queue or retry). The forward never blocks — it fails fast
on a refused connection and is capped at 5s otherwise, so a down tunnel never
wedges a Claude hook or one container's event behind another's. For a durable
record that doesn't depend on the laptop being up, pair this with the ntfy.sh
phone push above; the two are independent and can run together.

## Things that feel different vs. local

- **First attach is slow.** VSCode pulls ~100MB of server into the
  container over your laptop↔remote link. It's cached in the persistent
  volume after that, so subsequent attaches are quick.
- **Terminal latency tracks your RTT.** A laptop in the US talking to a
  Hetzner Helsinki box is usable but noticeable; same-continent is fine.
- **Claude remote control needs no tunnel.** It polls the Anthropic API
  over outbound HTTPS, so you can pair from claude.ai/code or the mobile
  app regardless of where the container runs — no port to forward.
