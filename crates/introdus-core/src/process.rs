//! A thin, logged wrapper over `std::process::Command`.
//!
//! Every external tool the control plane drives — `podman`, `tmux`, `git` —
//! goes through [`Cmd`]. It centralizes argument building, non-zero-exit ->
//! error mapping, and stdout capture, so the `podman`/`tmux`/`git` modules stay
//! declarative.

use std::cell::RefCell;
use std::ffi::OsStr;
use std::process::{Command, Stdio};
use std::rc::Rc;

use anyhow::{bail, Context, Result};

/// A shared, line-oriented output buffer (see [`capture_stdio`]).
pub type OutputBuffer = Rc<RefCell<Vec<String>>>;

thread_local! {
    /// When set, [`Cmd::run`]/[`Cmd::stdout`] pipe the child's output into this
    /// buffer instead of inheriting the terminal — so a full-screen TUI that
    /// owns the screen isn't corrupted by a subprocess writing to it.
    static CAPTURE: RefCell<Option<OutputBuffer>> = const { RefCell::new(None) };
}

/// Redirect the output of subsequent [`Cmd`] runs on this thread into `buf`
/// rather than the inherited terminal. The returned guard restores normal
/// inheritance when dropped, so capture is scoped to the guard's lifetime.
#[must_use = "capture ends when the guard is dropped"]
pub fn capture_stdio(buf: OutputBuffer) -> CaptureGuard {
    CAPTURE.with(|c| *c.borrow_mut() = Some(buf));
    CaptureGuard(())
}

/// Restores terminal-inheriting stdio on drop. See [`capture_stdio`].
pub struct CaptureGuard(());

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        CAPTURE.with(|c| *c.borrow_mut() = None);
    }
}

fn capture_target() -> Option<OutputBuffer> {
    CAPTURE.with(|c| c.borrow().clone())
}

/// Append `bytes` to `buf`, split into lines (a trailing partial line is kept).
fn push_lines(buf: &OutputBuffer, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let text = String::from_utf8_lossy(bytes);
    let mut sink = buf.borrow_mut();
    for line in text.trim_end_matches('\n').split('\n') {
        sink.push(line.to_owned());
    }
}

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

    /// Run to completion; error on a non-zero exit. Inherits stdio normally, or
    /// — while a [`capture_stdio`] guard is active — pipes the child's
    /// stdout+stderr into the capture buffer so it never touches the screen.
    pub fn run(mut self) -> Result<()> {
        if let Some(buf) = capture_target() {
            let out = self
                .inner
                .stdin(Stdio::null())
                .output()
                .with_context(|| format!("failed to spawn `{}`", self.label))?;
            push_lines(&buf, &out.stdout);
            push_lines(&buf, &out.stderr);
            if !out.status.success() {
                bail!("`{}` exited with {}", self.label, out.status);
            }
            return Ok(());
        }
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
    /// stderr is inherited normally, or piped into the capture buffer while a
    /// [`capture_stdio`] guard is active (so it can't corrupt a TUI screen).
    pub fn stdout(mut self) -> Result<String> {
        let capture = capture_target();
        let stderr = if capture.is_some() {
            Stdio::piped()
        } else {
            Stdio::inherit()
        };
        let out = self
            .inner
            .stderr(stderr)
            .output()
            .with_context(|| format!("failed to spawn `{}`", self.label))?;
        if let Some(buf) = &capture {
            push_lines(buf, &out.stderr);
        }
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
    fn ta25_label_accumulates_args() {
        let c = Cmd::new("podman").arg("run").args(["--rm", "img"]);
        assert_eq!(c.label(), "podman run --rm img");
    }

    #[test]
    fn ta25_run_ok_and_failure() {
        Cmd::new("true").run().unwrap();
        let err = Cmd::new("false").run().unwrap_err();
        assert!(err.to_string().contains("`false` exited"));
    }

    #[test]
    fn ta25_stdout_is_captured_and_trimmed() {
        let out = Cmd::new("printf").arg("  hi  ").stdout().unwrap();
        assert_eq!(out, "hi");
    }

    #[test]
    fn ta25_ok_probe() {
        assert!(Cmd::new("true").ok());
        assert!(!Cmd::new("false").ok());
        assert!(!Cmd::new("introdus-no-such-binary-xyz").ok());
    }

    #[test]
    fn ta25_capture_redirects_run_output_into_the_buffer() {
        let buf: OutputBuffer = Rc::new(RefCell::new(Vec::new()));
        {
            let _guard = capture_stdio(buf.clone());
            Cmd::new("printf").arg("one\ntwo\n").run().unwrap();
        }
        assert_eq!(*buf.borrow(), vec!["one".to_owned(), "two".to_owned()]);
        // Guard dropped: run() is back to inheriting, buffer untouched.
        Cmd::new("true").run().unwrap();
        assert_eq!(buf.borrow().len(), 2);
    }
}
