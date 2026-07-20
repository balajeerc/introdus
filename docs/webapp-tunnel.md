# Public webapp tunnel

> Part of [introdus](../README.md#features). Expose your in-container webapp to the public internet via a Cloudflare quick tunnel.

By default the webapp binds to `127.0.0.1` on the container host — reachable only
locally or over your own [SSH/Tailscale tunnel](remote-host.md#reaching-the-published-ports).
Opt in and introdus can instead expose it publicly through a **Cloudflare quick
tunnel** — no Cloudflare account, no domain.

## Prerequisites

- A webapp actually listening on `WEBAPP_PORT` inside the container (e.g. started
  by a [launch hook](launch-hooks.md)).
- Your dev server must **allow the trycloudflare hostname** in its `Host`-header
  check (see below) — otherwise you get "Blocked request" pages instead of your
  app.

## Usage

Set `EXPOSE_WEBAPP=true` in
[config](setup-and-configuration.md#configuration-reference) (or turn it on from
the [control panel](control-panel.md) → "(Re)Expose app via Cloudflare Tunnel").
The next launch:

- starts `cloudflared` in a `tunnel` tmux session inside the container,
- prints a `https://*.trycloudflare.com` URL in the startup banner,
- caches it at `/home/dev/.logs/tunnel-url.txt` (also via the `tunnel-url`
  command inside the container, and the panel's "Show tunnel URL").

The URL is stable for the container's lifetime and **rotates on relaunch**.

### If the tunnel drops

Cloudflare quick tunnels can disconnect on their own. When that happens, pick
**"(Re)Expose app via Cloudflare Tunnel"** (`e`) again: the panel probes the
cached URL from the host and, if it is no longer routing, restarts `cloudflared`
in place — reusing the container's existing edge-IP holes — and prints a fresh
URL (no recreate, no volume churn). If the current URL is still reachable it is
left untouched. (Under the hood this runs `setup.sh restart-tunnel` inside the
container.)

Allow the hostname in your dev server config (this lives in *your* repo — commit
it once). For Vite:

```js
export default defineConfig({
  server: {
    allowedHosts: ['.trycloudflare.com'],  // leading dot = subdomain wildcard
  },
});
```

Next (`allowedDevOrigins`), webpack-dev-server (`allowedHosts`), etc. have
equivalents.

> Changing `EXPOSE_WEBAPP` on an existing container needs a
> [`introdus recreate`](persistence-and-lifecycle.md) — the env var is applied at
> container-create time.

## What you're trading off

- **The URL is the only access control.** Anyone with it can reach the webapp —
  treat it like a secret.
- **Traffic flows through Cloudflare's edge** — they terminate TLS and re-encrypt
  over the tunnel, so request contents are visible to Cloudflare.
- **Any webapp vulnerability is now exposed to the open internet**, not just
  `localhost`.

## How it works

Enabling the tunnel auto-extends the [egress allowlist](egress-filtering.md) with
`api.trycloudflare.com` (for tunnel registration) plus a pinned set of Cloudflare
**argotunnel edge IPs**, allowed directly by IP on port 7844 in the nft filter —
cloudflared's edge protocol can't go through the HTTP proxy, and its normal
SRV-based edge discovery doesn't survive the container's restricted DNS. Those
IPs are hardcoded as `TUNNEL_EDGE_IPS` in
[egress.rs](../crates/introdus-core/src/egress.rs); the tunnel session is started
by [setup.sh](../setup.sh).

If Cloudflare ever rotates the edges and the tunnel stops connecting, refresh
them with `dig +short SRV _v2-origintunneld._tcp.argotunnel.com` on a machine with
normal DNS.

> Only the webapp is tunneled. [Claude remote control](claude-remote-control.md)
> needs no tunnel — it polls the Anthropic API over outbound HTTPS.
