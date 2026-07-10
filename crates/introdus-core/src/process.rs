//! A thin, logged wrapper over `std::process::Command`.
//!
//! Every external tool the control plane drives — `podman`, `tmux`, `git` —
//! goes through [`Cmd`]. It centralizes argument building, non-zero-exit ->
//! error mapping, and stdout capture, so the `podman`/`tmux`/`git` modules stay
//! declarative.

use std::ffi::OsStr;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

/// A builder around one external command invocation.
pub struct Cmd {
    inner: Command,
    /// Human-readable form for error messages (`podman run …`).
    label: String,
}

impl Cmd {
    /// Start building an invocation of `program`.
    pub fn new(program: &str) -> Self {
        Self {
            inner: Command::new(program),
            label: program.to_owned(),
        }
    }

    /// Append one argument.
    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.push_label(arg.as_ref());
        self.inner.arg(arg);
        self
    }

    /// Append several arguments.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        for a in args {
            self.push_label(a.as_ref());
            self.inner.arg(a);
        }
        self
    }

    /// Set an environment variable for the child.
    pub fn env(mut self, key: impl AsRef<OsStr>, val: impl AsRef<OsStr>) -> Self {
        self.inner.env(key, val);
        self
    }

    /// Set the child's working directory.
    pub fn current_dir(mut self, dir: impl AsRef<std::path::Path>) -> Self {
        self.inner.current_dir(dir);
        self
    }

    /// The human-readable command line, for logs and errors.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Run to completion inheriting stdio; error on a non-zero exit.
    pub fn run(mut self) -> Result<()> {
        let status = self
            .inner
            .status()
            .with_context(|| format!("failed to spawn `{}`", self.label))?;
        if !status.success() {
            bail!("`{}` exited with {}", self.label, status);
        }
        Ok(())
    }

    /// Replace the current process with this command (never returns on success).
    /// Falls back to a normal spawn+wait on platforms without `exec`.
    #[cfg(unix)]
    pub fn exec(mut self) -> Result<std::convert::Infallible> {
        use std::os::unix::process::CommandExt;
        // `exec` only returns on failure.
        let err = self.inner.exec();
        Err(err).with_context(|| format!("failed to exec `{}`", self.label))
    }

    /// Run capturing stdout; error on a non-zero exit. Returns trimmed stdout.
    pub fn stdout(mut self) -> Result<String> {
        let out = self
            .inner
            .stderr(Stdio::inherit())
            .output()
            .with_context(|| format!("failed to spawn `{}`", self.label))?;
        if !out.status.success() {
            bail!("`{}` exited with {}", self.label, out.status);
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    /// Run silently and report whether it exited zero — for existence probes
    /// like `podman image exists …` where a non-zero exit is a normal "no".
    pub fn ok(mut self) -> bool {
        self.inner
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn push_label(&mut self, arg: &OsStr) {
        self.label.push(' ');
        self.label.push_str(&arg.to_string_lossy());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_accumulates_args() {
        let c = Cmd::new("podman").arg("run").args(["--rm", "img"]);
        assert_eq!(c.label(), "podman run --rm img");
    }

    #[test]
    fn run_ok_and_failure() {
        Cmd::new("true").run().unwrap();
        let err = Cmd::new("false").run().unwrap_err();
        assert!(err.to_string().contains("`false` exited"));
    }

    #[test]
    fn stdout_is_captured_and_trimmed() {
        let out = Cmd::new("printf").arg("  hi  ").stdout().unwrap();
        assert_eq!(out, "hi");
    }

    #[test]
    fn ok_probe() {
        assert!(Cmd::new("true").ok());
        assert!(!Cmd::new("false").ok());
        assert!(!Cmd::new("introdus-no-such-binary-xyz").ok());
    }
}
