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
- cache the URL at `/home/dev/.logs/tunnel-url.txt` (also retrievable from
  inside the container via the `tunnel-url` command on `$PATH`)

The URL is stable for the container's lifetime and rotates on relaunch.
No Cloudflare account or domain required. The egress allowlist is
auto-extended with `api.trycloudflare.com` (for tunnel registration)
plus a pinned set of Cloudflare argotunnel edge IPs — these are
hardcoded in [egress.rs](../crates/introdus-core/src/egress.rs) (`TUNNEL_EDGE_IPS`)
since cloudflared's normal SRV-based edge discovery doesn't survive the
container's restricted DNS.
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
time: `introdus recreate` (keeps the volume) or `introdus reset` (also
wipes the volume) — or `podman rm -f introdus-<project>-<suffix>`. The
container name carries a per-project `<suffix>` — run `podman ps` to see
the exact name.

## Connecting from VSCode

VSCode's Dev Containers extension can attach to the running container via
podman's docker-compat socket. You get a VSCode window whose filesystem,
terminal, and extensions all live inside the container — the Claude Code
extension you install there uses the `claude` binary already in the image.

If the container host **is your dev machine** (you launched the container
locally on Linux with rootless podman), the only setting you need in your
local VSCode `settings.json` is:

```jsonc
"dev.containers.dockerPath": "podman"
```

That tells the Dev Containers extension to drive `podman` instead of
`docker`; everything else (container listing) is handled by the local
rootless podman binary.

If the container host is a **separate remote Linux machine** (Hetzner, Oracle
Cloud, etc.), don't try to expose podman's socket over SSH. Use VSCode's
Remote-SSH flow instead — see
[Running on a remote host.md](Running%20on%20a%20remote%20host.md).

Then: Command Palette → **Dev Containers: Attach to Running Container…** →
pick `introdus-<project>-<suffix>` (run `podman ps` for the exact name;
the per-project suffix keeps each project's container — and VS Code's cached
attach config for it — distinct, even when the same project runs on more than
one host). From the new window, install the Claude Code extension — it lands
in `/home/dev/.vscode-server/extensions/` on the persistent volume and
survives across launches.

First attach downloads VSCode server (~100MB) into `/home/dev/.vscode-server/`.
The download goes through the in-container egress proxy (default-deny);
`update.code.visualstudio.com`, `vscode.download.prss.microsoft.com`, and
`marketplace.visualstudio.com` are already in the default `WHITELIST_HOSTS`
allowlist ([sample.env](../sample.env)) for this reason. Any extension or VSIX
you install pulls through the same proxy, so its hostnames must be in
`WHITELIST_HOSTS` too — the marketplace hosts already are. Subsequent launches
reuse the cached server.

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

The easiest way to start an agent is from the **`introdus` control menu**
("install/launch agents"), which spawns it in its own tmux window in the
session. Or start Claude directly with the bundled `run-claude` helper, which
cds into the repo, opens a tmux session named `claude`, and launches Claude Code
with `--dangerously-skip-permissions`. Run this **on the container host** (over
SSH if it's remote), or from a VS Code terminal already attached to the
container:

```bash
podman exec -it --user dev introdus-<project>-<suffix> run-claude
```

The `--user dev` is required: the workload, its files under `/home/dev`, and
its per-uid tmux socket all belong to the non-root `dev` user. Drop in with a
shell the same way — `podman exec -it --user dev introdus-<project>-<suffix> bash`.

Re-running `run-claude` re-attaches to the existing `claude` session instead
of spawning a second one (`Ctrl-a d` detaches without killing it; the
container's tmux prefix is remapped from `C-b` to `C-a` so it doesn't collide
with a host-side tmux you're attaching through).

The first time you connect, pair the session from **claude.ai/code** or the
**Claude mobile app** — the pairing prompt appears in the session itself.
Auth persists in the volume, so subsequent launches don't need re-pairing,
and you can then drive the agent from your phone.
