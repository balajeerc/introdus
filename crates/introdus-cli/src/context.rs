//! Everything the launch path derives from a project's [`Config`]: podman
//! object names, the per-container assets directory (materialized bash core +
//! build context), the generated proxy allowlist, and the resolved cloudflared
//! tunnel IPs. Bundling it keeps `image`/`run` declarative.

use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use introdus_core::process::Cmd;
use introdus_core::{assets, egress, names, paths, Config};

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
    let mut v4: Vec<String> = (egress::TUNNEL_API_HOST, 443u16)
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

/// Ancestor helper for tests / callers: is a path a project dir (has `.env`)?
pub fn env_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".env")
}
