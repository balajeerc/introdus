//! Everything the launch path derives from a project's [`Config`]: podman
//! object names, the per-container assets directory (materialized bash core +
//! build context), the generated proxy allowlist, and the resolved cloudflared
//! tunnel IPs. Bundling it keeps `image`/`run` declarative.

use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use introdus_core::process::Cmd;
use introdus_core::{agents, assets, egress, names, paths, Config};

/// Resolved launch context for one project.
pub struct LaunchContext {
    pub config: Config,
    /// Read by the tmux session model (M4) as the session cwd.
    #[allow(dead_code)]
    pub project_dir: PathBuf,
    /// Read when persisting a generated suffix back to `.env` (M5).
    #[allow(dead_code)]
    pub image_suffix: String,
    pub container_name: String,
    pub legacy_container_name: String,
    pub volume_name: String,
    pub image_name: String,
    /// Materialized bash core + Dockerfile build context.
    pub assets_dir: PathBuf,
    pub allowlist_file: PathBuf,
    /// Ordered hostnames the proxy enforces (git host + whitelist + tunnel).
    pub container_whitelist: Vec<String>,
    /// Cloudflared edge IPs (empty unless `EXPOSE_WEBAPP`).
    pub tunnel_edge_ips: Vec<String>,
    /// Resolved `api.trycloudflare.com` IPs (empty unless `EXPOSE_WEBAPP`).
    pub tunnel_api_ips: Vec<String>,
    /// Resolved `relay.paseo.sh` IPs (empty unless `INSTALL_PASEO`). Allowed
    /// directly on 443 by the nft filter because paseo's relay WebSocket ignores
    /// the HTTP proxy.
    pub paseo_relay_ips: Vec<String>,
}

impl LaunchContext {
    /// Build the context for `config` rooted at `project_dir`, materializing the
    /// embedded assets and computing all derived names.
    pub fn resolve(config: Config, project_dir: PathBuf) -> Result<Self> {
        let image_suffix = config
            .image_suffix
            .clone()
            .unwrap_or_else(|| names::fallback_suffix(&config.project_name, &hostname()));

        let container_name = names::container_name(&config.project_name, &image_suffix);
        let legacy_container_name = format!("introdus-{}", config.project_name);
        let volume_name = names::volume_name(&config.project_name);
        let image_name = names::image_name(&config.project_name, &image_suffix);

        let assets_dir = paths::assets_dir(&container_name)?;
        assets::materialize(&assets_dir)?;
        let allowlist_file = paths::allowlist_file(&container_name)?;

        let container_whitelist = egress::container_whitelist(
            &config.repo_url,
            &config.whitelist_hosts,
            config.expose_webapp,
        );

        let (tunnel_edge_ips, tunnel_api_ips) = if config.expose_webapp {
            (
                egress::TUNNEL_EDGE_IPS
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect(),
                resolve_tunnel_api_ips(),
            )
        } else {
            (Vec::new(), Vec::new())
        };

        // paseo's daemon dials the relay over a WebSocket that ignores the proxy,
        // so (like cloudflared) it needs a direct-by-IP nft hole. Only resolve
        // when paseo is opted in; empty otherwise = no hole.
        let paseo_relay_ips = if config.install_paseo {
            resolve_ipv4(agents::paseo::RELAY_HOST)
        } else {
            Vec::new()
        };

        Ok(Self {
            config,
            project_dir,
            image_suffix,
            container_name,
            legacy_container_name,
            volume_name,
            image_name,
            assets_dir,
            allowlist_file,
            container_whitelist,
            tunnel_edge_ips,
            tunnel_api_ips,
            paseo_relay_ips,
        })
    }

    /// The materialized firewall entrypoint (runtime bind-mount source).
    pub fn entrypoint(&self) -> PathBuf {
        assets::entrypoint(&self.assets_dir)
    }

    /// The materialized `setup.sh` (runtime bind-mount source).
    pub fn setup_script(&self) -> PathBuf {
        assets::setup_script(&self.assets_dir)
    }

    /// The materialized tinyproxy config (runtime bind-mount source).
    pub fn tinyproxy_conf(&self) -> PathBuf {
        assets::tinyproxy_conf(&self.assets_dir)
    }

    /// The Dockerfile used to build the base image.
    pub fn dockerfile(&self) -> PathBuf {
        assets::dockerfile(&self.assets_dir)
    }

    /// Write the proxy allowlist file from the container whitelist.
    pub fn write_allowlist(&self) -> Result<()> {
        let body = egress::render_allowlist(&self.container_whitelist);
        std::fs::write(&self.allowlist_file, body)
            .with_context(|| format!("writing allowlist {}", self.allowlist_file.display()))
    }
}

/// The host's name, for the fallback image suffix. Best-effort; `localhost` when
/// the `hostname` command is unavailable.
fn hostname() -> String {
    Cmd::new("hostname")
        .stdout()
        .unwrap_or_else(|_| "localhost".to_owned())
}

/// Resolve `api.trycloudflare.com` to its IPv4 addresses. cloudflared POSTs the
/// quick-tunnel registration directly (not via the HTTP proxy), so these are
/// allowed by IP on 443 in the nft filter. Anycast + stable, so a launch-time
/// resolve is fine; empty on failure (registration then just warns).
fn resolve_tunnel_api_ips() -> Vec<String> {
    resolve_ipv4(egress::TUNNEL_API_HOST)
}

/// Resolve `host` to its sorted, de-duped IPv4 addresses (port 443). Used for the
/// direct-by-IP nft holes needed by clients that bypass the HTTP proxy
/// (cloudflared's tunnel API, paseo's relay WebSocket). Empty on failure — the
/// caller then just can't reach that host directly and surfaces its own warning.
fn resolve_ipv4(host: &str) -> Vec<String> {
    let mut v4: Vec<String> = (host, 443u16)
        .to_socket_addrs()
        .map(|it| {
            it.filter(|a| a.is_ipv4())
                .map(|a| a.ip().to_string())
                .collect()
        })
        .unwrap_or_default();
    v4.sort();
    v4.dedup();
    v4
}

/// The per-project config subdirectory: `<project>/.introdus`. Namespaces our
/// config (and any future per-project artifacts) so it never collides with the
/// repo's own `.env`, and keeps the project root tidy.
pub fn config_subdir(project_dir: &Path) -> PathBuf {
    project_dir.join(".introdus")
}

/// The canonical config file we always *write*: `<project>/.introdus/config.env`.
/// (`Config::save` creates the `.introdus` dir as needed.)
pub fn config_write_path(project_dir: &Path) -> PathBuf {
    config_subdir(project_dir).join("config.env")
}

/// The legacy config location, from before configs moved under `.introdus/`.
fn legacy_env_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".env")
}

/// The config file to *read* for `project_dir`: the canonical
/// `.introdus/config.env` if it exists, else the legacy `.env` if it exists,
/// else the canonical path (the not-yet-created case a fresh wizard writes to).
pub fn env_path(project_dir: &Path) -> PathBuf {
    let canonical = config_write_path(project_dir);
    if canonical.exists() {
        return canonical;
    }
    let legacy = legacy_env_path(project_dir);
    if legacy.exists() {
        return legacy;
    }
    canonical
}

/// True when this project still keeps its config at the legacy `./.env` and has
/// not been migrated to `.introdus/config.env` yet.
pub fn has_legacy_config(project_dir: &Path) -> bool {
    !config_write_path(project_dir).exists() && legacy_env_path(project_dir).exists()
}

/// On the interactive entry points (launch / init), offer to move a legacy
/// `./.env` into `.introdus/config.env`. Declining leaves it in place (it still
/// loads via [`env_path`]'s fallback), so this is a one-time nudge, never a
/// blocker. Best-effort: a failed move is reported but not fatal.
pub fn migrate_legacy_config(project_dir: &Path) -> Result<()> {
    if !has_legacy_config(project_dir) {
        return Ok(());
    }
    let legacy = legacy_env_path(project_dir);
    let canonical = config_write_path(project_dir);
    let prompt = format!(
        "Move this project's config from {} into {}?",
        legacy.display(),
        canonical.display()
    );
    if !crate::ui::confirm(&prompt, true)? {
        return Ok(());
    }
    if let Some(parent) = canonical.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::rename(&legacy, &canonical)
        .with_context(|| format!("moving {} -> {}", legacy.display(), canonical.display()))?;
    println!("  moved config to {}", canonical.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_proj(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "introdus-ctx-{}-{tag}-{}",
            std::process::id(),
            // Distinct per call so parallel tests don't collide.
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ta132_env_path_prefers_canonical_then_legacy() {
        let dir = temp_proj("resolve");

        // Nothing yet -> the canonical write path (what a fresh wizard uses).
        assert_eq!(env_path(&dir), dir.join(".introdus/config.env"));
        assert!(!has_legacy_config(&dir));

        // Only a legacy `.env` -> read that, and flag it as needing migration.
        std::fs::write(dir.join(".env"), "PROJECT_NAME=x\n").unwrap();
        assert_eq!(env_path(&dir), dir.join(".env"));
        assert!(has_legacy_config(&dir));

        // Canonical present wins even with a legacy file still around.
        std::fs::create_dir_all(dir.join(".introdus")).unwrap();
        std::fs::write(dir.join(".introdus/config.env"), "PROJECT_NAME=x\n").unwrap();
        assert_eq!(env_path(&dir), dir.join(".introdus/config.env"));
        assert!(!has_legacy_config(&dir));

        std::fs::remove_dir_all(&dir).ok();
    }
}
