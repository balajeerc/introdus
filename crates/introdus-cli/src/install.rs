//! `introdus install` — put the single binary on `PATH`. Replaces
//! `host_install.sh`'s PATH-symlink step. The notification listener is no longer
//! a global systemd service — `introdus launch` starts `notify-host` as a
//! detached per-session service (see [`crate::session`]) — so there's nothing
//! else to install for the local case.

use std::path::PathBuf;

use anyhow::{Context, Result};

/// Copy the running binary into the user's local bin directory and print PATH
/// guidance.
pub fn run() -> Result<()> {
    let src = std::env::current_exe().context("cannot locate the running introdus binary")?;
    let bin_dir = install_dir()?;
    std::fs::create_dir_all(&bin_dir).with_context(|| format!("creating {}", bin_dir.display()))?;
    let dest = bin_dir.join("introdus");

    if same_file(&src, &dest) {
        println!("==> introdus is already installed at {}", dest.display());
    } else {
        std::fs::copy(&src, &dest)
            .with_context(|| format!("copying {} -> {}", src.display(), dest.display()))?;
        set_executable(&dest)?;
        println!("==> installed introdus to {}", dest.display());
    }

    print_path_guidance(&bin_dir);
    Ok(())
}

/// `~/.local/bin` (or the XDG executable dir).
fn install_dir() -> Result<PathBuf> {
    dirs::executable_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/bin")))
        .context("cannot determine an install directory (no $HOME)")
}

fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn print_path_guidance(bin_dir: &std::path::Path) {
    let on_path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d == bin_dir))
        .unwrap_or(false);
    println!();
    if on_path {
        println!(
            "{} is on your PATH — you're set. In any project directory run:",
            bin_dir.display()
        );
        println!("\n    introdus\n");
    } else {
        println!(
            "NOTE: {} is not on your PATH. Add to your shell profile:",
            bin_dir.display()
        );
        println!("\n    export PATH=\"{}:$PATH\"\n", bin_dir.display());
        println!("Then reload your shell and run `introdus` from a project directory.");
    }
}

#[cfg(unix)]
fn set_executable(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .with_context(|| format!("chmod +x {}", path.display()))
}

#[cfg(not(unix))]
fn set_executable(_path: &std::path::Path) -> Result<()> {
    Ok(())
}
