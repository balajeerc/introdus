//! The container-side bash core, embedded into the binary.
//!
//! introdus is a single self-contained binary, but the container's security
//! core stays bash (see PLAN.md). We `include_str!` those files at build time
//! and [`materialize`] them into a per-container assets directory at launch.
//! That directory doubles as:
//!   * the **base-image build context** (`Dockerfile` at its root + the
//!     `container/` tree the Dockerfile `COPY`s), and
//!   * the source of the **runtime bind-mounts** (`setup.sh`,
//!     `firewall-entrypoint.sh`, `tinyproxy.conf`) that `launch` mounts into the
//!     container so edits apply without a rebuild — exactly as the old
//!     `launch_dev_container.sh` did.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// One embedded file: its repo-relative path, contents, and whether it should
/// be materialized executable.
struct Asset {
    /// Path relative to the assets/build-context root (e.g. `container/bin/rc-notify`).
    rel: &'static str,
    contents: &'static str,
    exec: bool,
}

/// Every file needed to build the base image and to bind-mount the security
/// core at runtime. Paths mirror the repo layout so the Dockerfile's `COPY`
/// directives resolve unchanged.
const ASSETS: &[Asset] = &[
    Asset {
        rel: "Dockerfile",
        contents: include_str!("../../../Dockerfile"),
        exec: false,
    },
    Asset {
        rel: "setup.sh",
        contents: include_str!("../../../setup.sh"),
        exec: true,
    },
    Asset {
        rel: "container/agents.sh",
        contents: include_str!("../../../container/agents.sh"),
        exec: false,
    },
    Asset {
        rel: "container/bin/egress-log",
        contents: include_str!("../../../container/bin/egress-log"),
        exec: true,
    },
    Asset {
        rel: "container/bin/install-agents",
        contents: include_str!("../../../container/bin/install-agents"),
        exec: true,
    },
    Asset {
        rel: "container/bin/rc-notify",
        contents: include_str!("../../../container/bin/rc-notify"),
        exec: true,
    },
    Asset {
        rel: "container/bin/run-claude",
        contents: include_str!("../../../container/bin/run-claude"),
        exec: true,
    },
    Asset {
        rel: "container/claude/settings.json",
        contents: include_str!("../../../container/claude/settings.json"),
        exec: false,
    },
    Asset {
        rel: "container/claude/test_notify.sh",
        contents: include_str!("../../../container/claude/test_notify.sh"),
        exec: true,
    },
    Asset {
        rel: "container/egress/firewall-entrypoint.sh",
        contents: include_str!("../../../container/egress/firewall-entrypoint.sh"),
        exec: true,
    },
    Asset {
        rel: "container/egress/tinyproxy.conf",
        contents: include_str!("../../../container/egress/tinyproxy.conf"),
        exec: false,
    },
];

/// Write every embedded asset under `dir`, preserving relative paths and file
/// modes. Overwrites existing files so a new binary version refreshes the core.
pub fn materialize(dir: &Path) -> Result<()> {
    for a in ASSETS {
        let target = dir.join(a.rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(&target, a.contents)
            .with_context(|| format!("writing asset {}", target.display()))?;
        set_mode(&target, a.exec)?;
    }
    Ok(())
}

/// The materialized `setup.sh` (bind-mounted at `/setup.sh`).
pub fn setup_script(dir: &Path) -> PathBuf {
    dir.join("setup.sh")
}

/// The materialized firewall entrypoint (bind-mounted at
/// `/usr/local/bin/firewall-entrypoint.sh`).
pub fn entrypoint(dir: &Path) -> PathBuf {
    dir.join("container/egress/firewall-entrypoint.sh")
}

/// The materialized tinyproxy config (bind-mounted at
/// `/etc/tinyproxy/tinyproxy.conf`).
pub fn tinyproxy_conf(dir: &Path) -> PathBuf {
    dir.join("container/egress/tinyproxy.conf")
}

/// The Dockerfile at the root of the build context (`dir` itself).
pub fn dockerfile(dir: &Path) -> PathBuf {
    dir.join("Dockerfile")
}

#[cfg(unix)]
fn set_mode(path: &Path, exec: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if exec { 0o755 } else { 0o644 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("chmod {mode:o} {}", path.display()))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _exec: bool) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta21_assets_embed_nonempty() {
        assert!(ASSETS.iter().all(|a| !a.contents.is_empty()));
        // Spot-check the security-critical core is really embedded.
        let entry = ASSETS
            .iter()
            .find(|a| a.rel.ends_with("firewall-entrypoint.sh"))
            .unwrap();
        assert!(
            entry.contents.contains("nft"),
            "entrypoint must install nft"
        );
    }

    #[test]
    fn ta22_materialize_writes_tree_with_modes() {
        let dir = std::env::temp_dir().join(format!("introdus-assets-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        materialize(&dir).unwrap();

        assert!(dockerfile(&dir).is_file());
        assert!(setup_script(&dir).is_file());
        assert!(tinyproxy_conf(&dir).is_file());
        let entry = entrypoint(&dir);
        assert!(entry.is_file());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&entry).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755, "entrypoint must be executable");
            let conf_mode = std::fs::metadata(tinyproxy_conf(&dir))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(conf_mode & 0o777, 0o644, "conf must not be executable");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
