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

/// Per-container proxy allowlist file, bind-mounted read-only at
/// `/etc/tinyproxy/egress-allowlist.txt`. Regenerated on every launch.
pub fn allowlist_file(container_name: &str) -> Result<PathBuf> {
    Ok(state_dir()?.join(format!("allowlist-{container_name}.txt")))
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
}
