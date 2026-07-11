//! Shared helpers for the pty integration tests. Not every helper is used by
//! every test binary, so silence dead-code here (each `tests/*.rs` file compiles
//! this module into its own crate).
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Default expect timeout (ms) — generous; the wizard is local + interactive.
pub const TIMEOUT_MS: Option<u64> = Some(20_000);

/// A throwaway `HOME` + project directory for one test, under the cargo target
/// tmpdir (auto-cleaned between `cargo` runs, isolated per test via a counter).
pub struct Fixture {
    pub home: PathBuf,
    pub proj: PathBuf,
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

impl Fixture {
    /// Create `<target-tmp>/it-<pid>-<n>/{home, <project>}`.
    pub fn new(project: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("it-{}-{n}", std::process::id()));
        let home = root.join("home");
        let proj = root.join(project);
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        Self { home, proj }
    }

    /// A `Command` for `introdus <sub>` in the project dir, with `HOME` pointed
    /// at the temp home so key scans, `.ssh`, and the state dir stay isolated.
    pub fn cmd(&self, sub: &str) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_introdus"));
        c.arg(sub)
            .current_dir(&self.proj)
            .env("HOME", &self.home)
            .env("TERM", "xterm-256color");
        c
    }

    pub fn env_file(&self) -> PathBuf {
        self.proj.join(".env")
    }

    pub fn deploy_keys_dir(&self) -> PathBuf {
        self.home.join(".ssh/introdus-deploy-keys")
    }

    /// Write a minimal but valid `.env` (the four required fields plus a couple
    /// of harmless flags) so `introdus menu` can load it.
    pub fn write_env(&self, project: &str) {
        let body = format!(
            "PROJECT_NAME={project}\n\
             REPO_URL=git@github.com:o/{project}.git\n\
             DEPLOY_KEY_PATH=/tmp/introdus-test-nonexistent-key\n\
             WEBAPP_PORT=3000\n\
             EXPOSE_WEBAPP=false\n"
        );
        std::fs::write(self.env_file(), body).unwrap();
    }
}

/// Generate a passphrase-less ed25519 keypair at `path` (creating parent dirs).
pub fn keygen(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let ok = Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-N", "", "-C", "fixture", "-f"])
        .arg(path)
        .status()
        .expect("spawn ssh-keygen")
        .success();
    assert!(ok, "ssh-keygen failed");
}
