//! The tmux session model. `introdus launch` puts each container inside one
//! tmux session: a `main-control` window (the control TUI) and a
//! `dev-container` window (the podman logs). Utilities later spawn `root-bash`,
//! `dev-bash`, and per-agent windows.

use std::convert::Infallible;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use introdus_core::{session as names, tmux, Config};

use crate::context::env_path;
use crate::launch::LaunchOpts;
use crate::preflight;

/// The control-TUI window (window 1).
const MAIN_WINDOW: &str = "main-control";
/// The container-logs window (window 2).
const DEV_WINDOW: &str = "dev-container";
/// The notification-service window.
const NOTIFY_WINDOW: &str = "notify";

/// `introdus launch`: ensure the tmux session exists (creating the control +
/// container windows on first run) and attach to it.
pub fn launch(_opts: LaunchOpts) -> Result<()> {
    preflight::check_session()?;
    let dir = std::env::current_dir()?;
    let env = env_path(&dir);
    if !env.exists() {
        // First run for this project: set it up interactively, then launch.
        crate::wizard::run(&dir)?;
    }

    let mut config = Config::load(&env)?;
    let session = ensure_session_name(&mut config, &env)?;

    if tmux::has_session(&session) {
        println!("==> attaching to existing session {session}");
        return attach(&session).map(|_| ());
    }

    let bin = current_exe()?;
    println!("==> creating tmux session {session}");
    tmux::new_detached_session(&session, MAIN_WINDOW, &window_cmd(&bin, "menu"), &dir)?;
    // Notification service first, so the FIFO exists before the container mounts
    // it; then the container window.
    tmux::new_window(
        &session,
        NOTIFY_WINDOW,
        &notify_cmd(&bin, &config),
        false,
        &dir,
    )?;
    tmux::new_window(&session, DEV_WINDOW, &window_cmd(&bin, "up"), false, &dir)?;
    attach(&session).map(|_| ())
}

/// Return the session name, generating and persisting one to `.env` on first
/// launch.
fn ensure_session_name(config: &mut Config, env: &Path) -> Result<String> {
    if let Some(name) = &config.session_name {
        return Ok(name.clone());
    }
    let name = names::generate(&config.project_name);
    config.session_name = Some(name.clone());
    config
        .save(env)
        .with_context(|| format!("persisting SESSION_NAME to {}", env.display()))?;
    println!("==> minted session name {name} (saved to .env)");
    Ok(name)
}

/// Build the shell command a tmux window runs: `exec '<bin>' <sub>` so the
/// window's shell is replaced by introdus (the window closes when it exits).
fn window_cmd(bin: &Path, sub: &str) -> String {
    format!(
        "exec {} {sub}",
        crate::util::shell_quote(&bin.to_string_lossy())
    )
}

/// Build the notify-host window command, exporting the project's notification
/// settings so the service renders ntfy / forwards / desktop popups correctly.
fn notify_cmd(bin: &Path, config: &Config) -> String {
    let q = crate::util::shell_quote;
    let mut prefix = format!(
        "ENABLE_NOTIFY_SH_ALERTS={} ",
        config.enable_notify_sh_alerts
    );
    if let Some(topic) = &config.ntfy_sh_topic {
        prefix.push_str(&format!("NTFY_SH_TOPIC={} ", q(topic)));
    }
    if let Some(addr) = &config.rc_forward_addr {
        prefix.push_str(&format!("RC_FORWARD_ADDR={} ", q(addr)));
    }
    format!("{prefix}exec {} notify-host", q(&bin.to_string_lossy()))
}

/// Attach the terminal to `session` (never returns on success).
fn attach(session: &str) -> Result<Infallible> {
    tmux::attach(session).exec()
}

fn current_exe() -> Result<PathBuf> {
    std::env::current_exe().context("cannot determine the introdus binary path")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta65_window_cmd_execs_binary() {
        let cmd = window_cmd(Path::new("/opt/introdus"), "up");
        assert_eq!(cmd, "exec '/opt/introdus' up");
    }
}
