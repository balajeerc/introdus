//! Base-image lifecycle: build the shared `introdus-base:latest` when missing
//! or stale, tag a cheap per-project alias, and prune stale project tags.
//!
//! Staleness signal differs from the old shell: the Dockerfile and baked
//! `container/` files are embedded in this binary and materialized fresh each
//! launch (so their mtimes are always "now"). The meaningful trigger is instead
//! **the introdus binary being newer than the image** — a new build may carry a
//! changed Dockerfile/asset. The rebuild is cache-enabled, so an unchanged
//! content set is a near-instant cache hit.

use anyhow::{Context, Result};
use introdus_core::names::{image_slug, BASE_IMAGE};
use introdus_core::podman::{image_exists, podman};

use crate::context::LaunchContext;

/// Ensure the base image exists and is current, then tag the per-project alias
/// and prune stale project tags.
pub fn ensure(ctx: &LaunchContext, rebuild: bool) -> Result<()> {
    if rebuild {
        println!("==> rebuilding base image {BASE_IMAGE} (--no-cache)");
        build(ctx, true)?;
    } else if !image_exists(BASE_IMAGE) {
        println!("==> building base image {BASE_IMAGE}");
        build(ctx, false)?;
    } else if is_stale() {
        println!("==> introdus is newer than the base image — cached rebuild");
        println!("    (use `introdus rebuild-base` to force a full --no-cache rebuild)");
        build(ctx, false)?;
    } else {
        println!("==> using cached base image {BASE_IMAGE}");
    }
    tag_and_prune(ctx)
}

/// Force a base-image rebuild (`--no-cache`), for `introdus rebuild-base`.
pub fn rebuild(ctx: &LaunchContext) -> Result<()> {
    println!("==> rebuilding base image {BASE_IMAGE} (--no-cache)");
    build(ctx, true)
}

fn build(ctx: &LaunchContext, no_cache: bool) -> Result<()> {
    let mut c = podman().arg("build");
    if no_cache {
        c = c.arg("--no-cache");
    }
    c.args(["-t", BASE_IMAGE, "-f"])
        .arg(ctx.dockerfile())
        .arg(&ctx.assets_dir)
        .run()
}

/// True when the introdus binary's mtime is newer than the base image's
/// creation time (so embedded assets may have changed).
fn is_stale() -> bool {
    match (base_image_epoch(), binary_epoch()) {
        (Some(img), Some(bin)) => bin > img,
        // Can't tell -> don't force a rebuild (shell also errs toward not).
        _ => false,
    }
}

fn base_image_epoch() -> Option<i64> {
    podman()
        .args([
            "image",
            "inspect",
            "--format",
            "{{.Created.Unix}}",
            BASE_IMAGE,
        ])
        .stdout()
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

fn binary_epoch() -> Option<i64> {
    let modified = std::env::current_exe()
        .ok()?
        .metadata()
        .ok()?
        .modified()
        .ok()?;
    let secs = modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    i64::try_from(secs).ok()
}

/// Tag the base image as the per-project alias and untag stale project tags from
/// earlier launches (different suffix).
fn tag_and_prune(ctx: &LaunchContext) -> Result<()> {
    podman()
        .args(["image", "tag", BASE_IMAGE, &ctx.image_name])
        .run()
        .with_context(|| format!("tagging {BASE_IMAGE} as {}", ctx.image_name))?;
    println!("==> tagged base image as {}", ctx.image_name);

    let prefix = format!("introdus-{}-", image_slug(&ctx.config.project_name));
    let listing = podman()
        .args(["image", "ls", "--format", "{{.Repository}}:{{.Tag}}"])
        .stdout()
        .unwrap_or_default();
    for line in listing.lines() {
        let reference = line.trim().trim_start_matches("localhost/");
        if reference == ctx.image_name {
            continue;
        }
        if is_stale_project_tag(reference, &prefix) {
            println!("==> removing stale project image tag {reference}");
            let _ = podman().args(["untag", reference]).run();
        }
    }
    Ok(())
}

/// True for `introdus-<slug>-XXXX:latest` tags (4-char suffix) of this project.
fn is_stale_project_tag(reference: &str, prefix: &str) -> bool {
    match reference
        .strip_prefix(prefix)
        .and_then(|r| r.strip_suffix(":latest"))
    {
        Some(suffix) => suffix.len() == 4 && suffix.chars().all(|c| c.is_ascii_alphanumeric()),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta32_stale_tag_matching() {
        let prefix = "introdus-web-";
        assert!(is_stale_project_tag("introdus-web-ab12:latest", prefix));
        assert!(!is_stale_project_tag(
            "introdus-web-ab12:latest",
            "introdus-other-"
        ));
        assert!(!is_stale_project_tag("introdus-web-abc:latest", prefix)); // wrong len
        assert!(!is_stale_project_tag("introdus-web-ab12:v2", prefix)); // wrong tag
    }
}
