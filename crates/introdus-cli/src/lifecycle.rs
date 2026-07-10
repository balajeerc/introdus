//! Container/volume lifecycle: legacy cleanup, `--recreate` (drop the
//! container, keep the volume), and `--reset` (also wipe the volume, guarded by
//! a best-effort dirty-git scan and a mandatory typed confirmation).

use std::io::Write;

use anyhow::{bail, Result};
use introdus_core::names::BASE_IMAGE;
use introdus_core::podman::{self, image_exists, podman};

use crate::context::LaunchContext;

/// What to do to any existing container/volume before launching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// Reuse whatever exists.
    Keep,
    /// Remove the container, keep the `/home/dev` volume.
    Recreate,
    /// Remove the container AND wipe the volume.
    Reset,
}

/// For `--reset`: always require a typed confirmation (even when the dirty scan
/// finds nothing — it's best-effort and must never be the sole guard).
pub fn confirm_reset(ctx: &LaunchContext) -> Result<()> {
    if !podman::volume_exists(&ctx.volume_name) {
        return Ok(());
    }
    let dirty = scan_dirty_git(ctx);
    println!();
    println!(
        "  !!  reset will PERMANENTLY WIPE volume {} — everything under",
        ctx.volume_name
    );
    println!("      /home/dev: the repo, uncommitted changes, branches, installed packages.");
    match &dirty {
        Some(report) if !report.trim().is_empty() => {
            println!("\n  Uncommitted / unpushed git state that would be LOST:");
            for line in report.lines() {
                println!("    {line}");
            }
        }
        _ => println!("      (no uncommitted git state detected — but double-check anyway.)"),
    }
    print!("\n  Type 'yes' to permanently wipe it (anything else aborts): ");
    std::io::stdout().flush().ok();
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    if input.trim() != "yes" {
        bail!("aborted.");
    }
    Ok(())
}

/// Remove the legacy pre-suffix container, then apply the lifecycle removal.
pub fn apply(ctx: &LaunchContext, lifecycle: Lifecycle) -> Result<()> {
    if ctx.legacy_container_name != ctx.container_name
        && podman::container_exists(&ctx.legacy_container_name)
    {
        println!(
            "==> removing legacy container {} (volume preserved)",
            ctx.legacy_container_name
        );
        podman::remove_container(&ctx.legacy_container_name)?;
    }
    match lifecycle {
        Lifecycle::Reset => {
            println!(
                "==> reset: removing container {} and wiping volume {}",
                ctx.container_name, ctx.volume_name
            );
            podman::remove_container(&ctx.container_name)?;
            podman::remove_volume(&ctx.volume_name)?;
        }
        Lifecycle::Recreate => {
            if podman::container_exists(&ctx.container_name) {
                println!(
                    "==> recreate: removing container {} (volume {} preserved)",
                    ctx.container_name, ctx.volume_name
                );
                podman::remove_container(&ctx.container_name)?;
            }
        }
        Lifecycle::Keep => {}
    }
    Ok(())
}

/// Create the persistent volume if absent.
pub fn ensure_volume(ctx: &LaunchContext) -> Result<()> {
    if podman::volume_exists(&ctx.volume_name) {
        println!("==> reusing volume {}", ctx.volume_name);
    } else {
        podman()
            .args(["volume", "create", &ctx.volume_name])
            .run()?;
        println!("==> created volume {} (first launch)", ctx.volume_name);
    }
    Ok(())
}

/// Drop a one-shot sentinel into the volume so `setup.sh` fast-forwards the repo
/// on the next start.
pub fn schedule_pull(ctx: &LaunchContext) -> Result<()> {
    println!("==> pull: scheduling git pull --ff-only on next container start");
    podman()
        .args(["run", "--rm", "--network=none", "--volume"])
        .arg(format!("{}:/home/dev", ctx.volume_name))
        .args([&ctx.image_name, "touch", "/home/dev/.pull-on-next-start"])
        .run()
}

/// Best-effort scan of `/home/dev/work` for uncommitted/unpushed git state, run
/// in a read-only throwaway container. Returns `None` if the base image is
/// missing (nothing to scan with).
fn scan_dirty_git(ctx: &LaunchContext) -> Option<String> {
    if !image_exists(BASE_IMAGE) {
        return None;
    }
    println!("==> reset: scanning /home/dev/work for uncommitted/unpushed git state");
    podman()
        .args(["run", "--rm", "--network=none", "--volume"])
        .arg(format!("{}:/home/dev:ro", ctx.volume_name))
        .args([BASE_IMAGE, "bash", "-c", DIRTY_SCAN])
        .stdout()
        .ok()
}

const DIRTY_SCAN: &str = r#"set +e
export GIT_CONFIG_GLOBAL=/tmp/scan-gitconfig
git config --global --add safe.directory "*" 2>/dev/null
while IFS= read -r gitpath; do
    repo=${gitpath%/.git}
    cd "$repo" 2>/dev/null || continue
    status=$(git status --porcelain 2>/dev/null)
    stashes=$(git stash list 2>/dev/null)
    if [ -f "$gitpath" ]; then unpushed=0; else
        unpushed=$(git rev-list --count --all --not --remotes 2>/dev/null || echo 0)
    fi
    if [ -n "$status" ] || [ "${unpushed:-0}" -gt 0 ] || [ -n "$stashes" ]; then
        echo "--- $repo ---"
        [ -n "$status" ] && { echo "  working tree:"; echo "$status" | sed "s/^/    /"; }
        [ "${unpushed:-0}" -gt 0 ] && echo "  unpushed commits: $unpushed (not reachable from any remote)"
        [ -n "$stashes" ] && { echo "  stashes:"; echo "$stashes" | sed "s/^/    /"; }
        echo
    fi
done < <(find /home/dev/work -maxdepth 5 -name .git \( -type d -o -type f \) 2>/dev/null)
"#;
