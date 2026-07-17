# Task-completion notifications

Back to the [project README](../README.md).

A coding agent signals "task complete" / "awaiting input" from inside the **dev
container**; the harness turns that into a native desktop popup + sound on your
**dev machine** (and, optionally, a phone push). How the signal gets to you
depends on whether the container host is your laptop or a separate remote box.

The whole path is folded into the `introdus` binary — there is no Python and no
per-machine installer script. `introdus notify-host` runs on the container host;
`introdus notify-listen` runs on the laptop.

## The path

```
[dev container] rc-notify --FIFO--> [container host] introdus notify-host
          ├── host IS your laptop  -> render popup + sound here
          └── host is remote       -> forward over the SSH reverse (-R) tunnel
                                        --> [laptop] introdus notify-listen -> popup + sound
```

- **Dev container:** an agent's `Stop` / `Notification` / `PermissionRequest`
  hooks call [`rc-notify`](../container/bin/rc-notify), which writes
  `event<TAB>project` to the host endpoint mounted at `/run/notify` (a Linux
  FIFO under `XDG_RUNTIME_DIR` on the host). It never blocks a hook for more
  than a few seconds and never fails it, even if no listener is present.
- **Container host:** `introdus notify-host` reads the endpoint. The event is
  validated against a fixed whitelist (`done` / `waiting`) and the project label
  is stripped to `[A-Za-z0-9._-]` (≤40 chars) at this trust boundary, so a
  compromised container can't spoof arbitrary text or inject control characters
  under the "Claude Code" brand. It then renders locally, or — when
  `RC_FORWARD_ADDR` is set — forwards over the reverse tunnel.

`introdus` starts `notify-host` **automatically** as a detached service for each
launched tmux session (it self-exits when that session ends). One service relays
**every** event from that session's container; nothing extra to install for the
local case.

## Local host (host = laptop)

Nothing to configure. `introdus` starts `notify-host` on launch, and it renders
the popup + plays `notification_sound.wav` on the same machine (Linux:
`notify-send` + `paplay`/`pw-play`/`aplay`/`ffplay`; macOS: `osascript` +
`afplay`). Install `libnotify-bin` and a PulseAudio/PipeWire player if they're
missing.

## Remote host → laptop (two hops)

When the container host is a remote/headless box, it has no desktop to render
to, so the event makes a second hop to your laptop over an SSH **reverse**
(`-R`) tunnel. Your laptop dials *out* to the host (the same SSH you already
use), so nothing needs an inbound port — which matters when your laptop is
behind NAT.

### On the container host

The first-run setup wizard asks *"Forward notifications to a separate dev machine
over an SSH reverse tunnel?"* — answer **yes** on a remote/headless host and it
writes the forward target for you. To set it by hand instead, put it in the
project's `.introdus/config.env`:

```bash
RC_FORWARD_ADDR=127.0.0.1:8765
```

`introdus notify-host` then forwards each validated event to that loopback port
instead of rendering. The whitelist + label sanitization run here **and** again
on the laptop.

> `notify-host` reads `RC_FORWARD_ADDR` **once, when the tmux session starts**. If
> you add or change it on an already-running session, use the control menu's
> **"Restart the notification service"** — it respawns `notify-host` so it picks
> up the new value, no container recreate or session bounce needed. (The
> **"Send a test host notification"** action also prints whether the config
> forwards or renders locally, to help you spot a mismatch.)

### Host SSH-forwarding requirement

The laptop reaches that loopback port via the reverse tunnel, so the host's
`sshd` must permit forwarding for your user. Hardened hosts commonly ship
`AllowTcpForwarding no`, which silently blocks it. Grant the minimum for your
user only, in a drop-in such as `/etc/ssh/sshd_config.d/zz-notify-tunnel.conf`.

For notifications **only** (reverse `-R` forward):

```
Match User <your-host-user>
    AllowTcpForwarding remote
    PermitListen 127.0.0.1:8765 localhost:8765
```

If you **also** attach with **VS Code Remote-SSH** (which needs *local* `-L`
forwarding), use `all`, and add `PermitOpen` to confine `-L` to loopback so the
host can't be turned into a network pivot to the LAN or cloud metadata:

```
Match User <your-host-user>
    AllowTcpForwarding all
    PermitListen 127.0.0.1:8765 localhost:8765
    PermitOpen 127.0.0.1:* localhost:* [::1]:*
```

then `sudo sshd -t && sudo systemctl reload ssh` (or `reload sshd`); other users
stay at `AllowTcpForwarding no`. Two gotchas:

- The **`localhost:8765`** entry in `PermitListen` is required — a no-bind `-R`
  forward (used so it works under the default `GatewayPorts no`) presents its
  listen address as `localhost`, not `127.0.0.1`.
- If the host runs file-integrity monitoring (AIDE, etc.), refresh its baseline
  after editing `/etc/ssh`.

### On your laptop

`introdus notify-listen` now owns both halves — the reverse tunnel *and* the
listener. Run it bare the first time and a short wizard collects the SSH alias
of the container host, the loopback port, and whether to install a `systemd
--user` service:

```bash
introdus notify-listen
```

It saves your answers to `~/.config/introdus/notify-listen.env`, so later runs
skip the wizard. You can also pass everything non-interactively:

```bash
introdus notify-listen --via <ssh-alias-for-the-host> --port 8765
```

Under the hood it opens `autossh -M 0 -N -o ExitOnForwardFailure=yes -o
ServerAliveInterval=30 -o ServerAliveCountMax=3 -R 8765:127.0.0.1:8765 <alias>`
(falling back to plain `ssh` when `autossh` isn't installed), then binds the
loopback port, forces local rendering, and never re-forwards. The port must
match `RC_FORWARD_ADDR` on the host. The alias must accept key-based SSH without
a prompt. `--dry-run` prints the resolved plan without opening anything;
`--no-tunnel` runs only the listener (bring your own tunnel).

**Run it on every login** with the built-in `systemd --user` unit — answer *yes*
to the wizard's service prompt, or:

```bash
introdus notify-listen --via <alias> --install-service
```

This writes and enables `~/.config/systemd/user/introdus-notify-listen.service`
(`WantedBy=default.target`, `Restart=on-failure`). It deliberately does **not**
enable linger: the service starts with your graphical session so `notify-send` /
`paplay` inherit its D-Bus and display — a boot-time lingering service would fire
popups into a session that isn't there. Re-running with the same settings is a
no-op; changing `--via`/`--port` rewrites and restarts the unit. On macOS,
where there's no `systemd`, wrap the foreground command in a launchd agent.

## Which container fired it?

Each notification's title is suffixed with the container's project name — e.g.
*"Claude Code — myproject"* — so when you run many containers on one host you
can tell them apart at a glance. Derived from `PROJECT_NAME` (a runtime env
`introdus` passes into the container), override per-container with `RC_LABEL`.
A label change takes effect the next time the container is (re)created
(`introdus recreate`), since the value is set at container-create time.

## Phone push (ntfy.sh)

Independently of the desktop path, set `ENABLE_NOTIFY_SH_ALERTS=true` and
`NTFY_SH_TOPIC=<your-private-topic>` in the project `.introdus/config.env` to also push each
alert to your phone via [ntfy.sh](https://ntfy.sh) (install the app, subscribe
to the topic). Sent from the container host over outbound HTTPS, so it needs no
forwarding. Treat the topic name like a password — anyone who knows it can
publish and read.

## Limitations

The desktop path is **best-effort, fire-and-forget**: if the laptop is offline
or the tunnel is mid-reconnect when an event fires, that notification is dropped
(no queue, no retry). The forward never blocks — it fails fast on a refused
connection and is capped at 5s otherwise, so a down tunnel never wedges an agent
hook or queues one container's event behind another's. For a durable record that
doesn't depend on the laptop being up, pair it with the ntfy.sh push; the two
are independent and can run together.
