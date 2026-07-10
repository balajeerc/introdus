//! Host pre-flight checks. Linux rootless podman only; egress filtering lives
//! inside the container, so the host needs just podman + pasta (and, for the
//! session model, tmux).

use anyhow::{bail, Result};
use introdus_core::process::Cmd;

/// True if `cmd` is on `PATH`.
fn have(cmd: &str) -> bool {
    Cmd::new("sh")
        .args(["-c", &format!("command -v {cmd} >/dev/null 2>&1")])
        .ok()
}

/// True if running as uid 0.
fn is_root() -> bool {
    Cmd::new("id")
        .arg("-u")
        .stdout()
        .map(|s| s.trim() == "0")
        .unwrap_or(false)
}

/// True if podman reports rootless operation.
fn podman_rootless() -> bool {
    Cmd::new("podman")
        .args(["info", "--format", "{{.Host.Security.Rootless}}"])
        .stdout()
        .map(|s| s.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Verify the container-launch prerequisites; error with an install hint on the
/// first missing one. Mirrors the shell's pre-flight block.
pub fn check_launch() -> Result<()> {
    if std::env::consts::OS != "linux" {
        bail!("this harness supports Linux only (rootless podman).");
    }
    if is_root() {
        bail!("run as a regular user — rootless podman only (no sudo).");
    }
    if !have("podman") {
        bail!("podman not installed.");
    }
    if !have("pasta") {
        bail!("pasta not installed. try `apt install passt`.");
    }
    if !podman_rootless() {
        bail!("podman is not configured for rootless operation.");
    }
    Ok(())
}

/// Additionally require tmux, for the session-driven control plane.
/// Used by the tmux session model (M4).
#[allow(dead_code)]
pub fn check_session() -> Result<()> {
    check_launch()?;
    if !have("tmux") {
        bail!("tmux not installed. try `apt install tmux`.");
    }
    Ok(())
}
