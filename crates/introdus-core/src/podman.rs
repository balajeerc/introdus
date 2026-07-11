//! Thin `podman` helpers built on [`crate::process::Cmd`].
//!
//! Constructors return a [`Cmd`] the caller extends (so the big `podman run`
//! flag list in launch orchestration stays in one place), plus boolean probes
//! for the common existence checks.

use crate::process::Cmd;
use anyhow::Result;

/// A fresh `podman` invocation.
pub fn podman() -> Cmd {
    Cmd::new("podman")
}

/// True if the named image exists locally.
pub fn image_exists(name: &str) -> bool {
    podman().args(["image", "exists", name]).ok()
}

/// True if a container with this name exists (running or stopped).
pub fn container_exists(name: &str) -> bool {
    podman().args(["container", "exists", name]).ok()
}

/// True if the named container exists and is currently running. The existence
/// check gates the `inspect` so a missing container never spills `Error: no such
/// container` onto the menu (inspect inherits stderr).
pub fn container_running(name: &str) -> bool {
    container_exists(name)
        && podman()
            .args(["container", "inspect", "-f", "{{.State.Running}}", name])
            .stdout()
            .map(|s| s.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false)
}

/// Coarse host-visible lifecycle state of a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerState {
    /// Created and running.
    Running,
    /// Created but not running.
    Stopped,
    /// Not created (no such container).
    Absent,
}

/// Classify a container as running, stopped, or absent in one probe pair.
pub fn container_state(name: &str) -> ContainerState {
    if !container_exists(name) {
        ContainerState::Absent
    } else if container_running(name) {
        ContainerState::Running
    } else {
        ContainerState::Stopped
    }
}

/// True if the named volume exists.
pub fn volume_exists(name: &str) -> bool {
    podman().args(["volume", "exists", name]).ok()
}

/// Force-remove a container if present (no error if it's already gone).
pub fn remove_container(name: &str) -> Result<()> {
    if container_exists(name) {
        podman().args(["rm", "-f", name]).run()?;
    }
    Ok(())
}

/// Remove a volume if present.
pub fn remove_volume(name: &str) -> Result<()> {
    if volume_exists(name) {
        podman().args(["volume", "rm", name]).run()?;
    }
    Ok(())
}

/// A `podman exec` invocation into `container`, optionally as a specific user.
/// The caller appends the command to run.
pub fn exec(container: &str, user: Option<&str>) -> Cmd {
    let mut c = podman().arg("exec");
    if let Some(u) = user {
        c = c.args(["--user", u]);
    }
    c.arg(container)
}

/// An interactive `podman exec -it` invocation (adds `--user` when given).
pub fn exec_interactive(container: &str, user: Option<&str>) -> Cmd {
    let mut c = podman().args(["exec", "-it"]);
    if let Some(u) = user {
        c = c.args(["--user", u]);
    }
    c.arg(container)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta26_exec_builds_user_flag() {
        assert_eq!(exec("cx", Some("dev")).label(), "podman exec --user dev cx");
        assert_eq!(exec("cx", None).label(), "podman exec cx");
    }

    #[test]
    fn ta26_interactive_exec_is_it() {
        assert_eq!(
            exec_interactive("cx", Some("dev")).arg("bash").label(),
            "podman exec -it --user dev cx bash"
        );
    }
}
