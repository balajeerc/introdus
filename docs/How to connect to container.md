# How to connect to container

Back to the [project README](../README.md).

Throughout this doc, **container host** is the box running podman (a remote
machine or your laptop) and **dev machine** is the laptop you sit at. When
they're the same machine, "container host" and "dev machine" collapse.

## Connecting from your phone

Both published ports bind to `127.0.0.1` on the **container host**, not
`0.0.0.0`. To reach them from a phone you need your own secure tunnel to the
container host (Tailscale, SSH port forward, etc.) — this repo deliberately
doesn't expose anything on your LAN.

### Public tunnel for the webapp (opt-in)

For the `WEBAPP_PORT` specifically, the harness can also expose it to
the public internet via a Cloudflare quick tunnel. Set `EXPOSE_WEBAPP=true`
in `.env` and the next launch will:

- start `cloudflared` in a `tunnel` tmux session inside the container
- print a stable `https://*.trycloudflare.com` URL in the startup banner
- cache the URL at `/root/.logs/tunnel-url.txt` (also retrievable from
  inside the container via the `tunnel-url` command on `$PATH`)

The URL is stable for the container's lifetime and rotates on relaunch.
No Cloudflare account or domain required. The egress allowlist is
auto-extended with `api.trycloudflare.com` (for tunnel registration)
plus a pinned set of Cloudflare argotunnel edge IPs — these are
hardcoded in [launch.sh](../launch.sh) since cloudflared's normal SRV-
based edge discovery doesn't survive the container's restricted DNS.
If Cloudflare ever rotates these edges and the tunnel stops connecting,
refresh them by running `dig +short SRV _v2-origintunneld._tcp.argotunnel.com`
on a machine with normal DNS.

**What you're trading off:**

- The URL is the only access control. Anyone with it can reach the
  webapp — treat it like a secret.
- Traffic flows through Cloudflare's edge: they terminate TLS and
  re-encrypt over the tunnel, so request contents are visible to
  Cloudflare.
- Any vulnerability in the webapp is now exposed to the open internet
  rather than just `localhost`.
- Dev frameworks reject requests with non-`localhost` `Host` headers
  by default. **You must allow the trycloudflare hostname in your dev
  server config.** For Vite, in `vite.config.js`:

  ```js
  export default defineConfig({
    server: {
      // ...your existing options
      allowedHosts: ['.trycloudflare.com'],  // leading dot = subdomain wildcard
    },
  });
  ```

  Equivalent settings exist for Next (`allowedDevOrigins`), webpack-dev-
  server (`allowedHosts`), etc. This config lives in your project repo,
  not the harness — commit it once and every future launch works.

Only the webapp is tunneled. Remote control doesn't need a tunnel: it
polls the Anthropic API over outbound HTTPS and you reach the session
through the claude.ai/code or mobile-app pairing flow, not an inbound port.

If `EXPOSE_WEBAPP` is left unset (or set to anything other than `true`),
no tunnel is started and no extra hosts are added to the allowlist.

To pick up a change to `EXPOSE_WEBAPP` on an existing container, you
need to remove the container so the new env var is applied at create
time: `podman rm -f remote-code-<project>` (or `./launch.sh --reset`,
which also wipes the volume).

## Connecting from VSCode

VSCode's Dev Containers extension can attach to the running container via
podman's docker-compat socket. You get a VSCode window whose filesystem,
terminal, and extensions all live inside the container — the Claude Code
extension you install there uses the `claude` binary already in the image.

If the container host **is your dev machine** (you launched the container
locally), the only setting you need in your local VSCode `settings.json` is:

```jsonc
"dev.containers.dockerPath": "podman"
```

That tells the Dev Containers extension to drive `podman` instead of
`docker`; everything else (socket discovery, container listing) is
handled by the local podman binary.

If the container host is a **separate remote machine** (Hetzner, Oracle Cloud,
etc.), don't try to expose podman's socket over SSH — the
`docker.host` setting that used to do this was deprecated when the
Docker extension was renamed to Container Tools, and podman's built-in
SSH client is awkward to configure (it doesn't read `~/.ssh/config`).
Use VSCode's Remote-SSH flow instead — see
[Running on a remote host.md](Running%20on%20a%20remote%20host.md).

Then: Command Palette → **Dev Containers: Attach to Running Container…** →
pick `remote-code-<project>`. From the new window, install the Claude
Code extension — it lands in `/root/.vscode-server/extensions/` on the
persistent volume and survives across launches.

First attach downloads VSCode server (~100MB) into `/root/.vscode-server/`;
`update.code.visualstudio.com`, `vscode.download.prss.microsoft.com`, and
`marketplace.visualstudio.com` are in the default [sample.env](../sample.env)
allowlist for this reason. Subsequent launches reuse the cached server.

The extension starts its own `claude` when you open it — it does not
share state with the tmux `claude` session that [`run-claude`](#claude-remote-control)
starts. If you want that one, open a terminal in VSCode and
`tmux attach -t claude`.

## Claude remote-control

Remote control is **on by default** for every Claude Code session in the
container — [container/claude/settings.json](../container/claude/settings.json)
sets `"remoteControlAtStartup": true`, so the bridge registers automatically
whenever `claude` starts. It works by making outbound HTTPS calls to the
Anthropic API and polling for work; it does **not** open an inbound port on
the container, so there's nothing to publish or tunnel for pairing.

Start a session with the bundled `run-claude` helper, which cds into the
repo, opens a tmux session named `claude`, and launches Claude Code with
`--dangerously-skip-permissions`. Run this **on the container host** (over SSH
if it's remote), or from a VS Code terminal already attached to the container:

```bash
podman exec -it remote-code-<project> run-claude
```

Re-running `run-claude` re-attaches to the existing `claude` session instead
of spawning a second one (`Ctrl-a d` detaches without killing it; the
container's tmux prefix is remapped from `C-b` to `C-a` so it doesn't collide
with a host-side tmux you're attaching through).

The first time you connect, pair the session from **claude.ai/code** or the
**Claude mobile app** — the pairing prompt appears in the session itself.
Auth persists in the volume, so subsequent launches don't need re-pairing,
and you can then drive the agent from your phone.
