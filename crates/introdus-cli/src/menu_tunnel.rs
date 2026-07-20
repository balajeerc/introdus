//! The control panel's "(Re)Expose app via Cloudflare Tunnel" action.
//!
//! Split out of [`crate::menu_actions`] (which stays under its file-size budget):
//! this is the one control-menu utility that reaches *back out to the public
//! internet* — it probes the cached quick-tunnel URL from the trusted host and,
//! when the tunnel has dropped, restarts cloudflared in place for a fresh URL.
//! It leans on a few `pub(crate)` helpers from `menu_actions` (`exec`,
//! `require_running`, `offer_recreate`, `save_config`).

use anyhow::{Context, Result};
use introdus_core::process::Cmd;

use crate::context::LaunchContext;
use crate::frontend::Frontend;
use crate::menu_actions::{exec, offer_recreate, require_running, save_config};
use crate::panel::Ui;

/// (Re)Expose the in-container app via a Cloudflare quick tunnel.
///
/// Two paths, chosen by whether the tunnel is already turned on:
/// - **Not yet exposed** — flip `EXPOSE_WEBAPP=true` and offer a recreate. The
///   recreate is what installs the nft edge-IP holes + adds `api.trycloudflare.com`
///   to the proxy allowlist, so it is genuinely required the first time.
/// - **Already exposed** — the running container has those holes, so probe the
///   cached quick-tunnel URL from *this host* (the workload can't reach a
///   `*.trycloudflare.com` subdomain through the proxy) and, only if it is no
///   longer routing, restart cloudflared in place for a fresh URL — no recreate,
///   no volume churn.
pub fn reexpose_webapp(ctx: &LaunchContext, ui: &mut Ui) -> Result<()> {
    if !ctx.config.expose_webapp {
        if !ui.confirm(
            "Expose the app to the internet via a Cloudflare tunnel?",
            false,
        )? {
            return Ok(());
        }
        let mut config = ctx.config.clone();
        config.expose_webapp = true;
        save_config(ctx, ui, &config)?;
        return offer_recreate(ctx, ui, "EXPOSE_WEBAPP=true");
    }

    require_running(ctx)?;

    // Config says exposed, but a container created *before* the tunnel was turned
    // on (e.g. the recreate was declined) has no edge-IP holes, so cloudflared
    // there can never reach the edge — that case still needs a recreate.
    if !container_has_tunnel_holes(ctx) {
        ui.log("  this container predates the Cloudflare tunnel egress rules —");
        ui.log("  a recreate is needed to (re)expose the app.");
        return offer_recreate(ctx, ui, "EXPOSE_WEBAPP=true");
    }

    refresh_running_tunnel(ctx, ui)
}

/// Refresh the quick tunnel of an already-exposed, running container that has the
/// edge-IP holes: probe the cached URL from *this host* and, only if it is no
/// longer routing (or absent), restart cloudflared in place for a fresh URL — no
/// recreate. Shared by the control panel and the CLI `expose-webapp`.
pub(crate) fn refresh_running_tunnel(ctx: &LaunchContext, f: &mut impl Frontend) -> Result<()> {
    match cached_tunnel_url(ctx) {
        Some(url) => {
            f.log(format!("  checking the current tunnel: {url}"));
            match probe_tunnel(&url) {
                TunnelState::Routing => {
                    f.log("  still reachable — leaving it up.");
                    return Ok(());
                }
                TunnelState::Down => f.log("  not responding — restarting cloudflared…"),
                TunnelState::Unknown => {
                    f.log("  couldn't verify reachability from this host (curl missing/blocked);");
                    f.log("  leaving the tunnel as-is. To force a restart, delete");
                    f.log("  ~/.logs/tunnel-url.txt in the container and try again.");
                    return Ok(());
                }
            }
        }
        None => f.log("  no active tunnel found — starting cloudflared…"),
    }

    // Reuse the pinned edge IPs baked into the container's env; the script waits
    // for the new URL, re-caches it, and prints it (streamed into the pane).
    exec(ctx, Some("dev"))
        .args(["bash", "/setup.sh", "restart-tunnel"])
        .run()
        .context("restarting the cloudflared tunnel")?;
    f.log("  done — use 'Show tunnel URL' to copy the new address.");
    Ok(())
}

/// Whether the running container was created with the cloudflared edge-IP egress
/// holes (i.e. `EXPOSE_WEBAPP` was true at `podman run`). Its `TUNNEL_EDGE_IPS`
/// env is non-empty exactly then; without it an in-place cloudflared restart
/// could never reach Cloudflare's edge (the nft filter would drop it).
pub(crate) fn container_has_tunnel_holes(ctx: &LaunchContext) -> bool {
    exec(ctx, Some("dev"))
        .args(["sh", "-c", r#"[ -n "${TUNNEL_EDGE_IPS:-}" ]"#])
        .ok()
}

/// The quick-tunnel URL the container cached at its last (re)start, or `None` if
/// absent or not a well-formed `*.trycloudflare.com` URL. The shape check is a
/// trust boundary: the file is written inside the (untrusted) container, and we
/// are about to hand its contents to `curl` on the trusted host.
fn cached_tunnel_url(ctx: &LaunchContext) -> Option<String> {
    let out = exec(ctx, Some("dev"))
        .args(["sh", "-c", "cat ~/.logs/tunnel-url.txt 2>/dev/null || true"])
        .stdout_quiet()
        .unwrap_or_default();
    let url = out.trim();
    is_quick_tunnel_url(url).then(|| url.to_owned())
}

/// Whether `s` is exactly `https://<label>.trycloudflare.com` with a non-empty
/// `[a-z0-9-]` label that isn't the `api.` registration host. Anchored on both
/// ends — nothing may follow the host — so no path/query smuggles through.
fn is_quick_tunnel_url(s: &str) -> bool {
    let Some(label) = s
        .strip_prefix("https://")
        .and_then(|r| r.strip_suffix(".trycloudflare.com"))
    else {
        return false;
    };
    !label.is_empty()
        && label != "api"
        && label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The outcome of probing a quick-tunnel URL from the host.
enum TunnelState {
    /// The edge answered with an HTTP status (even a 5xx from a down app) — the
    /// tunnel itself is routing.
    Routing,
    /// The edge returned Cloudflare's 1033 "no tunnel" page (HTTP 530) or the
    /// connection failed outright — cloudflared is not connected.
    Down,
    /// Couldn't probe (no `curl` on the host) — reachability is unknown.
    Unknown,
}

/// Probe `url` from the trusted host. A live quick tunnel returns *some* HTTP
/// status from the Cloudflare edge; a dead one returns 1033 as HTTP 530.
fn probe_tunnel(url: &str) -> TunnelState {
    if !Cmd::new("curl").arg("--version").ok() {
        return TunnelState::Unknown;
    }
    match Cmd::new("curl")
        .args([
            "-sS",
            "-o",
            "/dev/null",
            "-m",
            "10",
            "-w",
            "%{http_code}",
            url,
        ])
        .stdout_quiet()
    {
        Ok(code) if code.trim() == "530" => TunnelState::Down,
        Ok(_) => TunnelState::Routing,
        // Non-zero exit == connection/DNS/timeout failure: the edge is unreachable.
        Err(_) => TunnelState::Down,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta154_quick_tunnel_url_shape_is_anchored() {
        // Accepts a well-formed quick-tunnel URL.
        assert!(is_quick_tunnel_url(
            "https://brave-swift-otter.trycloudflare.com"
        ));
        assert!(is_quick_tunnel_url("https://a1.trycloudflare.com"));

        // Rejects the registration host, empty labels, and the bare apex.
        assert!(!is_quick_tunnel_url("https://api.trycloudflare.com"));
        assert!(!is_quick_tunnel_url("https://.trycloudflare.com"));
        assert!(!is_quick_tunnel_url("https://trycloudflare.com"));

        // Rejects wrong scheme / suffix and anything trailing the host (no
        // path/query/port can smuggle past — this feeds `curl` on the host).
        assert!(!is_quick_tunnel_url(
            "http://brave-swift-otter.trycloudflare.com"
        ));
        assert!(!is_quick_tunnel_url(
            "https://brave-swift-otter.trycloudflare.com/evil"
        ));
        assert!(!is_quick_tunnel_url(
            "https://brave-swift-otter.trycloudflare.com.evil.test"
        ));
        assert!(!is_quick_tunnel_url(
            "https://Brave_Swift.trycloudflare.com"
        ));
        assert!(!is_quick_tunnel_url(""));
    }
}
