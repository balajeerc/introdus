# Task-completion notifications

Back to the [project README](../README.md).

Claude Code signals "task complete" / "awaiting input" from inside the **dev
container**; the harness turns that into a native desktop popup + sound on your
**dev machine** (and, optionally, a phone push). How the signal gets to you
depends on whether the container host is your laptop or a separate remote box.

## The path

```
[dev container] rc-notify --FIFO--> [container host] host_listener.py
    --> host_notify.sh
          ├── host IS your laptop  -> render popup + sound here
          └── host is remote       -> forward over the SSH reverse (-R) tunnel
                                        --> [laptop] host_listener.py -> popup + sound
```

- **Dev container:** Claude's `Stop` / `Notification` / `PermissionRequest`
  hooks call [`rc-notify`](../container/bin/rc-notify), which writes
  `event<TAB>project` to the host endpoint mounted at `/run/notify` (a Linux
  FIFO under `XDG_RUNTIME_DIR` on the host). It never blocks a hook for more
  than a few seconds and never fails it, even if no listener is present.
- **Container host:** a single [`host_listener.py`](../host_listener.py) service
  reads the endpoint and runs [`host_notify.sh`](../host_notify.sh). The event
  is validated against a fixed whitelist (`done` / `waiting`) and the project
  label is stripped to `[A-Za-z0-9._-]` (≤40 chars) at this trust boundary, so a
  compromised container can't spoof arbitrary text or inject control characters
  under the "Claude Code" brand.

One listener relays **every** container on the host, so this is a host-level
setup, not per-project.

## Local host (host = laptop)

Nothing to configure. `./host_install.sh` installs the listener as a persistent
`systemd --user` service, and `host_notify.sh` renders the popup + plays
`notification_sound.wav` on the same machine.

## Remote host → laptop (two hops)

When the container host is a remote/headless box, it has no desktop to render
to, so the event makes a second hop to your laptop over an SSH **reverse**
(`-R`) tunnel. Your laptop dials *out* to the host (the same SSH you already
use), so nothing needs an inbound port — which matters when your laptop is
behind NAT.

### On the container host

`./host_install.sh` asks "forward to another machine?"; answer yes and it
records this in the **harness** `.env` (read by `host_notify.sh`):

```bash
RC_FORWARD_ADDR=127.0.0.1:8765
```

`host_notify.sh` then forwards each validated event to that loopback port
instead of rendering. The whitelist + label sanitization run here **and** again
on the laptop.

### Host SSH-forwarding requirement

The laptop reaches that loopback port via the reverse tunnel, so the host's
`sshd` must permit forwarding for your user. Hardened hosts commonly ship
`AllowTcpForwarding no`, which silently blocks it (the laptop installer's
preflight will tell you). Grant the minimum for your user only, in a drop-in
such as `/etc/ssh/sshd_config.d/zz-notify-tunnel.conf`.

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

From your checkout of this repo, install the always-on services (they survive
reboot and sleep):

```bash
./install_dev_machine_listener.sh <ssh-alias-for-the-host> 8765
```

It installs two `systemd --user` units:

- `rc-notify-listener.service` — `host_listener.py` in TCP mode; renders the
  popup + plays `notification_sound.wav`.
- `rc-notify-tunnel.service` — the `ssh -R` reverse tunnel to your alias
  (requires `autossh` for self-healing reconnects; the installer errors out
  without it — `sudo apt install autossh`, etc.).

The port must match `RC_FORWARD_ADDR`. The alias must accept key-based SSH
without a prompt (passphrase-less key, or an agent reachable from your
`systemd --user` session) since the tunnel runs with `BatchMode=yes`. Manage
with `systemctl --user status rc-notify-tunnel.service`; remove with
`./install_dev_machine_listener.sh --uninstall`.

For a quick foreground tunnel without systemd (a one-off), use
`./laptop_notify_tunnel.sh <ssh-alias> 8765` instead.

## Which container fired it?

Each notification's title is suffixed with the container's project name — e.g.
*"Claude Code — myproject"* — so when you run many containers on one host you
can tell them apart at a glance. Derived from `PROJECT_NAME`; override
per-container with the `RC_LABEL` env var. (The label is baked into the image,
so an existing container picks it up only after `./launch.sh --rebuild-base
--recreate`; until then it shows a bare "Claude Code".)

## Phone push (ntfy.sh)

Independently of the desktop path, set `ENABLE_NOTIFY_SH_ALERTS=true` and
`NTFY_SH_TOPIC=<your-private-topic>` in the harness `.env` to also push each
alert to your phone via [ntfy.sh](https://ntfy.sh) (install the app, subscribe
to the topic). Sent from the container host over outbound HTTPS, so it needs no
forwarding. Treat the topic name like a password — anyone who knows it can
publish and read.

## Limitations

The desktop path is **best-effort, fire-and-forget**: if the laptop is offline
or the tunnel is mid-reconnect when an event fires, that notification is dropped
(no queue, no retry). The forward never blocks — it fails fast on a refused
connection and is capped at 5s otherwise, so a down tunnel never wedges a Claude
hook or queues one container's event behind another's. For a durable record that
doesn't depend on the laptop being up, pair it with the ntfy.sh push; the two
are independent and can run together.
