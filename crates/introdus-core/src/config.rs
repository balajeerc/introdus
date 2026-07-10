//! The typed project configuration and its `.env` round-trip.
//!
//! `.env` remains the on-disk source of truth (bash-sourced by the old
//! `launch.sh`, read here via `dotenvy`). [`Config::load`] parses it into a
//! typed struct with the same defaults the shell used; [`Config::render`]
//! writes a canonical, briefly-commented `.env` back. The TUI/wizard is the
//! primary editor now, so a save normalizes the file — the exhaustive guidance
//! lives in `sample.env` and the docs.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};

use crate::env_file::{quote_scalar, read_map, split_list};

/// Default egress allowlist — the hosts the base image, package managers, and
/// Claude need. Mirrors `WHITELIST_HOSTS` in `sample.env`.
pub const DEFAULT_WHITELIST: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com",
    "codeload.github.com",
    "raw.githubusercontent.com",
    "api.github.com",
    "registry.npmjs.org",
    "pypi.org",
    "files.pythonhosted.org",
    "api.anthropic.com",
    "claude.ai",
    "platform.claude.com",
    "statsig.anthropic.com",
    "sentry.io",
    "mise.jdx.dev",
    "archive.ubuntu.com",
    "security.ubuntu.com",
    "update.code.visualstudio.com",
    "vscode.download.prss.microsoft.com",
    "marketplace.visualstudio.com",
];

const DEFAULT_MEM_LIMIT: &str = "8g";
const DEFAULT_CPU_LIMIT: &str = "8";
const DEFAULT_PIDS_LIMIT: u64 = 16384;
const DEFAULT_ROOT_TIMEOUT: u32 = 600;
const DEFAULT_CANARY_IP: &str = "93.184.216.34";

/// A project's full configuration, the typed mirror of its `.env`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    // ---- required identity ----
    pub project_name: String,
    pub repo_url: String,
    pub deploy_key_path: String,
    pub webapp_port: u16,

    // ---- agents & egress ----
    pub install_agents: Vec<String>,
    pub whitelist_hosts: Vec<String>,
    pub internal_allow_cidrs: Vec<String>,

    // ---- launch hooks ----
    pub on_launch_script: Option<String>,
    pub on_launch_root_script: Option<String>,
    pub on_launch_root_timeout: u32,

    // ---- ports & resources ----
    pub extra_ports: Vec<String>,
    pub mem_limit: String,
    pub cpu_limit: String,
    pub pids_limit: u64,

    // ---- identity / mounts / tmux ----
    pub image_suffix: Option<String>,
    pub shared_data_path: Option<String>,
    pub session_name: Option<String>,

    // ---- exposure & notifications ----
    pub expose_webapp: bool,
    pub enable_notify_sh_alerts: bool,
    pub ntfy_sh_topic: Option<String>,
    pub rc_forward_addr: Option<String>,

    // ---- egress self-check ----
    pub canary_blocked_ip: String,
}

impl Config {
    /// A minimal config with the four required fields set and everything else at
    /// its default — the starting point the wizard fills in.
    pub fn new(
        project_name: String,
        repo_url: String,
        deploy_key_path: String,
        webapp_port: u16,
    ) -> Self {
        Self {
            project_name,
            repo_url,
            deploy_key_path,
            webapp_port,
            install_agents: vec!["claude".to_owned()],
            whitelist_hosts: DEFAULT_WHITELIST.iter().map(|s| (*s).to_owned()).collect(),
            internal_allow_cidrs: Vec::new(),
            on_launch_script: None,
            on_launch_root_script: None,
            on_launch_root_timeout: DEFAULT_ROOT_TIMEOUT,
            extra_ports: Vec::new(),
            mem_limit: DEFAULT_MEM_LIMIT.to_owned(),
            cpu_limit: DEFAULT_CPU_LIMIT.to_owned(),
            pids_limit: DEFAULT_PIDS_LIMIT,
            image_suffix: None,
            shared_data_path: None,
            session_name: None,
            expose_webapp: false,
            enable_notify_sh_alerts: false,
            ntfy_sh_topic: None,
            rc_forward_addr: None,
            canary_blocked_ip: DEFAULT_CANARY_IP.to_owned(),
        }
    }

    /// Parse a `.env` file into a `Config`, applying the shell defaults for any
    /// unset optional field and erroring on a missing required one.
    pub fn load(path: &Path) -> Result<Self> {
        let m = read_map(path)?;
        let cfg = Self {
            project_name: required(&m, "PROJECT_NAME")?,
            repo_url: required(&m, "REPO_URL")?,
            deploy_key_path: required(&m, "DEPLOY_KEY_PATH")?,
            webapp_port: required(&m, "WEBAPP_PORT")?
                .parse()
                .context("WEBAPP_PORT must be a port number")?,
            install_agents: list_or(&m, "INSTALL_AGENTS", &["claude"]),
            whitelist_hosts: list_or(&m, "WHITELIST_HOSTS", DEFAULT_WHITELIST),
            internal_allow_cidrs: list_or(&m, "INTERNAL_ALLOW_CIDRS", &[]),
            on_launch_script: opt(&m, "ON_LAUNCH_SCRIPT"),
            on_launch_root_script: opt(&m, "ON_LAUNCH_ROOT_SCRIPT"),
            on_launch_root_timeout: parse_or(&m, "ON_LAUNCH_ROOT_TIMEOUT", DEFAULT_ROOT_TIMEOUT)?,
            extra_ports: list_or(&m, "EXTRA_PORTS", &[]),
            mem_limit: opt(&m, "MEM_LIMIT").unwrap_or_else(|| DEFAULT_MEM_LIMIT.to_owned()),
            cpu_limit: opt(&m, "CPU_LIMIT").unwrap_or_else(|| DEFAULT_CPU_LIMIT.to_owned()),
            pids_limit: parse_or(&m, "PIDS_LIMIT", DEFAULT_PIDS_LIMIT)?,
            image_suffix: opt(&m, "IMAGE_SUFFIX"),
            shared_data_path: opt(&m, "SHARED_DATA_PATH"),
            session_name: opt(&m, "SESSION_NAME"),
            expose_webapp: flag(&m, "EXPOSE_WEBAPP"),
            enable_notify_sh_alerts: flag(&m, "ENABLE_NOTIFY_SH_ALERTS"),
            ntfy_sh_topic: opt(&m, "NTFY_SH_TOPIC"),
            rc_forward_addr: opt(&m, "RC_FORWARD_ADDR"),
            canary_blocked_ip: opt(&m, "CANARY_BLOCKED_IP")
                .unwrap_or_else(|| DEFAULT_CANARY_IP.to_owned()),
        };
        Ok(cfg)
    }

    /// Render a canonical, briefly-commented `.env`. `load(render(cfg)) == cfg`.
    pub fn render(&self) -> String {
        let mut o = String::new();
        let _ = writeln!(
            o,
            "# introdus project config. Generated/edited by `introdus`; hand-editable."
        );
        let _ = writeln!(o, "# Full field docs live in sample.env and the docs/.\n");

        section(&mut o, "Required identity");
        scalar(&mut o, "PROJECT_NAME", &self.project_name);
        scalar(&mut o, "REPO_URL", &self.repo_url);
        scalar(&mut o, "DEPLOY_KEY_PATH", &self.deploy_key_path);
        scalar(&mut o, "WEBAPP_PORT", &self.webapp_port.to_string());

        section(
            &mut o,
            "Coding agents (space-separated ids; see container/agents.sh)",
        );
        inline_list(&mut o, "INSTALL_AGENTS", &self.install_agents);

        section(&mut o, "Egress: proxy hostname allowlist (default-deny)");
        multiline_list(&mut o, "WHITELIST_HOSTS", &self.whitelist_hosts);
        inline_list(&mut o, "INTERNAL_ALLOW_CIDRS", &self.internal_allow_cidrs);
        scalar(&mut o, "CANARY_BLOCKED_IP", &self.canary_blocked_ip);

        section(&mut o, "Launch hooks");
        opt_multiline(
            &mut o,
            "ON_LAUNCH_ROOT_SCRIPT",
            self.on_launch_root_script.as_deref(),
        );
        scalar(
            &mut o,
            "ON_LAUNCH_ROOT_TIMEOUT",
            &self.on_launch_root_timeout.to_string(),
        );
        opt_multiline(&mut o, "ON_LAUNCH_SCRIPT", self.on_launch_script.as_deref());

        section(&mut o, "Ports & resources");
        multiline_list(&mut o, "EXTRA_PORTS", &self.extra_ports);
        scalar(&mut o, "MEM_LIMIT", &self.mem_limit);
        scalar(&mut o, "CPU_LIMIT", &self.cpu_limit);
        scalar(&mut o, "PIDS_LIMIT", &self.pids_limit.to_string());

        section(&mut o, "Identity / mounts / tmux session");
        opt_scalar(&mut o, "IMAGE_SUFFIX", self.image_suffix.as_deref());
        opt_scalar(&mut o, "SHARED_DATA_PATH", self.shared_data_path.as_deref());
        opt_scalar(&mut o, "SESSION_NAME", self.session_name.as_deref());

        section(&mut o, "Exposure & notifications");
        scalar(&mut o, "EXPOSE_WEBAPP", bool_str(self.expose_webapp));
        scalar(
            &mut o,
            "ENABLE_NOTIFY_SH_ALERTS",
            bool_str(self.enable_notify_sh_alerts),
        );
        opt_scalar(&mut o, "NTFY_SH_TOPIC", self.ntfy_sh_topic.as_deref());
        opt_scalar(&mut o, "RC_FORWARD_ADDR", self.rc_forward_addr.as_deref());
        o
    }

    /// Write the rendered config to `path`.
    pub fn save(&self, path: &Path) -> Result<()> {
        std::fs::write(path, self.render())
            .with_context(|| format!("writing config to {}", path.display()))
    }
}

// ---- parse helpers ----------------------------------------------------------

fn opt(m: &HashMap<String, String>, key: &str) -> Option<String> {
    m.get(key)
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

fn required(m: &HashMap<String, String>, key: &str) -> Result<String> {
    opt(m, key).with_context(|| format!("{key} is required but missing/empty in .env"))
}

fn flag(m: &HashMap<String, String>, key: &str) -> bool {
    opt(m, key).as_deref() == Some("true")
}

fn list_or(m: &HashMap<String, String>, key: &str, default: &[&str]) -> Vec<String> {
    match m.get(key) {
        Some(v) => split_list(v),
        None => default.iter().map(|s| (*s).to_owned()).collect(),
    }
}

fn parse_or<T: std::str::FromStr>(m: &HashMap<String, String>, key: &str, default: T) -> Result<T>
where
    T::Err: std::fmt::Display,
{
    match opt(m, key) {
        None => Ok(default),
        Some(v) => v
            .parse()
            .map_err(|e| anyhow::anyhow!("{key} is invalid: {e}")),
    }
}

// ---- render helpers ---------------------------------------------------------

fn bool_str(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

fn section(o: &mut String, title: &str) {
    let _ = writeln!(o, "\n# --- {title} ---");
}

fn scalar(o: &mut String, key: &str, value: &str) {
    let _ = writeln!(o, "{key}={}", quote_scalar(value));
}

fn opt_scalar(o: &mut String, key: &str, value: Option<&str>) {
    if let Some(v) = value {
        scalar(o, key, v);
    }
}

fn inline_list(o: &mut String, key: &str, items: &[String]) {
    let _ = writeln!(o, "{key}={}", quote_scalar(&items.join(" ")));
}

fn multiline_list(o: &mut String, key: &str, items: &[String]) {
    if items.is_empty() {
        let _ = writeln!(o, "{key}=\"\"");
        return;
    }
    let _ = writeln!(o, "{key}=\"");
    for item in items {
        let _ = writeln!(o, "{item}");
    }
    let _ = writeln!(o, "\"");
}

fn opt_multiline(o: &mut String, key: &str, value: Option<&str>) {
    if let Some(v) = value {
        // Multi-line script: double-quote, escaping only `"`, `\`, backtick so
        // `$VAR` in the hook is preserved literally for bash to expand later.
        let mut esc = String::with_capacity(v.len());
        for c in v.chars() {
            if matches!(c, '"' | '\\' | '`') {
                esc.push('\\');
            }
            esc.push(c);
        }
        let _ = writeln!(o, "{key}=\"{esc}\"");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A distinctly-named temp path under the OS temp dir (no external crates).
    fn temp_env_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("introdus-cfg-{}-{tag}.env", std::process::id()))
    }

    fn sample() -> Config {
        let mut c = Config::new(
            "myproj".to_owned(),
            "git@github.com:org/repo.git".to_owned(),
            "/home/you/.ssh/deploy".to_owned(),
            3000,
        );
        c.install_agents = vec!["claude".to_owned(), "codex".to_owned()];
        c.internal_allow_cidrs = vec!["10.2.5.131".to_owned()];
        c.extra_ports = vec!["8123".to_owned(), "16379:6379".to_owned()];
        c.on_launch_script = Some("pnpm install\npnpm dev --host 0.0.0.0".to_owned());
        c.on_launch_root_script = Some("clickhouse start".to_owned());
        c.image_suffix = Some("ab12".to_owned());
        c.shared_data_path = Some("/data/in".to_owned());
        c.session_name = Some("introdus-fast-roving-car".to_owned());
        c.expose_webapp = true;
        c.enable_notify_sh_alerts = true;
        c.ntfy_sh_topic = Some("secret-topic-7c4a".to_owned());
        c.mem_limit = "12g".to_owned();
        c
    }

    #[test]
    fn round_trip_preserves_config() {
        let cfg = sample();
        let path = temp_env_path("roundtrip");
        cfg.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(cfg, loaded, "render/load round-trip must be lossless");
    }

    #[test]
    fn defaults_applied_for_minimal_env() {
        let path = temp_env_path("minimal");
        std::fs::write(
            &path,
            "PROJECT_NAME=web\nREPO_URL=git@github.com:o/r.git\nDEPLOY_KEY_PATH=/k\nWEBAPP_PORT=5173\n",
        )
        .unwrap();
        let c = Config::load(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(c.install_agents, vec!["claude".to_owned()]);
        assert_eq!(c.whitelist_hosts.len(), DEFAULT_WHITELIST.len());
        assert_eq!(c.mem_limit, "8g");
        assert_eq!(c.pids_limit, 16384);
        assert_eq!(c.on_launch_root_timeout, 600);
        assert_eq!(c.canary_blocked_ip, "93.184.216.34");
        assert!(!c.expose_webapp);
        assert!(c.session_name.is_none());
    }

    #[test]
    fn missing_required_field_errors() {
        let path = temp_env_path("bad");
        std::fs::write(&path, "PROJECT_NAME=web\n").unwrap();
        let err = Config::load(&path).unwrap_err();
        std::fs::remove_file(&path).ok();
        assert!(err.to_string().contains("REPO_URL"));
    }
}
