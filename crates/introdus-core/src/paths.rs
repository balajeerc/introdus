//! Filesystem locations the control plane uses on the container host.
//!
//! The host-side state directory holds per-container generated artifacts (the
//! proxy allowlist that gets bind-mounted read-only into the container, and the
//! materialized copies of the embedded bash core). It mirrors the
//! `$XDG_STATE_HOME/remote-code-harness` directory the old `launch.sh` used,
//! renamed to `introdus`.

use std::path::PathBuf;

use anyhow::{Context, Result};

/// Name of the host-side state directory under `$XDG_STATE_HOME`.
pub const STATE_DIR_NAME: &str = "introdus";

/// The host-side state directory: `$XDG_STATE_HOME/introdus`
/// (falls back to `~/.local/state/introdus`). Created if missing.
pub fn state_dir() -> Result<PathBuf> {
    let base = dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/state")))
        .context("cannot determine a state directory (no $XDG_STATE_HOME or $HOME)")?;
    let dir = base.join(STATE_DIR_NAME);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating state dir {}", dir.display()))?;
    Ok(dir)
}

/// The host-side config directory: `$XDG_CONFIG_HOME/introdus`
/// (falls back to `~/.config/introdus`). Created if missing. Distinct from the
/// state dir: this holds hand-editable user config, not generated artifacts.
pub fn config_dir() -> Result<PathBuf> {
    let base = dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .context("cannot determine a config directory (no $XDG_CONFIG_HOME or $HOME)")?;
    let dir = base.join(STATE_DIR_NAME);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating config dir {}", dir.display()))?;
    Ok(dir)
}

/// The dev-machine notification-listener config file
/// (`$XDG_CONFIG_HOME/introdus/notify-listen.env`). Stores the SSH alias and
/// port the `notify-listen` wizard collected, in the same `.env` format as the
/// per-project config, so a bare `introdus notify-listen` skips the wizard.
pub fn notify_listen_config() -> Result<PathBuf> {
    Ok(config_dir()?.join("notify-listen.env"))
}

/// Per-session log for the detached `notify-host` service (which has no tmux
/// window of its own). Viewable from the control menu on demand.
pub fn notify_log(session_name: &str) -> Result<PathBuf> {
    Ok(state_dir()?.join(format!("notify-{session_name}.log")))
}

/// Per-session PID file the detached `notify-host` service writes on startup,
/// so the control menu can find and restart it (to pick up a changed
/// `RC_FORWARD_ADDR` / ntfy setting) without bouncing the tmux session.
pub fn notify_pid(session_name: &str) -> Result<PathBuf> {
    Ok(state_dir()?.join(format!("notify-{session_name}.pid")))
}

/// Per-container proxy allowlist file, bind-mounted read-only at
/// `/etc/tinyproxy/egress-allowlist.txt`. Regenerated on every launch.
pub fn allowlist_file(container_name: &str) -> Result<PathBuf> {
    Ok(state_dir()?.join(format!("allowlist-{container_name}.txt")))
}

/// Per-container "launch in progress" marker. Written when a launch begins and
/// cleared once the container is observed running (or on launch failure) so the
/// control menu can show "starting container…" during the bring-up window —
/// which the launch process itself can't report, since it execs into podman.
pub fn launch_marker(container_name: &str) -> Result<PathBuf> {
    Ok(state_dir()?.join(format!("launching-{container_name}")))
}

/// Directory holding the materialized copies of the embedded bash core for a
/// given container, bind-mounted into it at launch.
pub fn assets_dir(container_name: &str) -> Result<PathBuf> {
    let dir = state_dir()?.join(format!("assets-{container_name}"));
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating assets dir {}", dir.display()))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta20_allowlist_path_is_under_state_dir() {
        // Uses the real state dir; just assert the shape, not that it's writable
        // in every CI sandbox.
        let p = allowlist_file("introdus-demo-ab12").unwrap();
        assert!(p.ends_with("allowlist-introdus-demo-ab12.txt"));
        assert!(p.to_string_lossy().contains(STATE_DIR_NAME));
    }

    #[test]
    fn ta20_launch_marker_path_is_per_container() {
        let p = launch_marker("introdus-demo-ab12").unwrap();
        assert!(p.ends_with("launching-introdus-demo-ab12"));
        assert!(p.to_string_lossy().contains(STATE_DIR_NAME));
    }

    #[test]
    fn ta20_notify_log_path_is_per_session() {
        let p = notify_log("swift-otter").unwrap();
        assert!(p.ends_with("notify-swift-otter.log"));
        assert!(p.to_string_lossy().contains(STATE_DIR_NAME));
    }

    #[test]
    fn ta139_notify_pid_path_is_per_session() {
        let p = notify_pid("swift-otter").unwrap();
        assert!(p.ends_with("notify-swift-otter.pid"));
        assert!(p.to_string_lossy().contains(STATE_DIR_NAME));
    }

    #[test]
    fn ta131_notify_listen_config_under_config_dir() {
        let p = notify_listen_config().unwrap();
        assert!(p.ends_with("notify-listen.env"));
        assert!(p.to_string_lossy().contains(STATE_DIR_NAME));
    }
}
