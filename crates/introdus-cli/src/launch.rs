//! Top-level launch orchestration, tying preflight, image, lifecycle, and run
//! together — the flow the old `launch_dev_container.sh` ran end to end.

use std::path::PathBuf;

use anyhow::{bail, Result};
use introdus_core::podman;
use introdus_core::Config;

use crate::context::{env_path, LaunchContext};
use crate::lifecycle::Lifecycle;
use crate::{image, lifecycle, preflight, run};

/// Options shared by the launch-style subcommands.
#[derive(Debug, Clone, Copy, Default)]
pub struct LaunchOpts {
    pub pull: bool,
    pub disable_network_block: bool,
}

/// Load the current project's `.env` from the working directory and resolve its
/// launch context.
pub fn load_context() -> Result<LaunchContext> {
    let dir = std::env::current_dir()?;
    load_context_in(dir)
}

fn load_context_in(dir: PathBuf) -> Result<LaunchContext> {
    let env = env_path(&dir);
    if !env.exists() {
        bail!(
            "no .env in {} — run `introdus` to set the project up (wizard)",
            dir.display()
        );
    }
    let config = Config::load(&env)?;
    LaunchContext::resolve(config, dir)
}

/// Create or start the container for the current project, applying `lifecycle`
/// first. On success this hands the terminal to podman and does not return.
pub fn run_launch(lifecycle: Lifecycle, opts: LaunchOpts) -> Result<()> {
    preflight::check_launch()?;
    let ctx = load_context()?;
    run::validate_inputs(&ctx)?;

    if lifecycle == Lifecycle::Reset {
        lifecycle::confirm_reset(&ctx)?;
    }
    image::ensure(&ctx, false)?;
    lifecycle::apply(&ctx, lifecycle)?;
    lifecycle::ensure_volume(&ctx)?;
    if opts.pull {
        lifecycle::schedule_pull(&ctx)?;
    }
    ctx.write_allowlist()?;

    println!(
        "\n==> launching container {} (linux rootless)",
        ctx.container_name
    );
    println!("    repo:   {}", ctx.config.repo_url);
    println!("    webapp: port {}", ctx.config.webapp_port);
    if opts.disable_network_block {
        println!("    WARNING: --disable-network-block — egress filtering OFF");
    }

    if podman::container_exists(&ctx.container_name) {
        run::start_and_exec(&ctx)?;
    } else {
        run::create_and_exec(&ctx, opts.disable_network_block)?;
    }
    Ok(()) // unreachable: the calls above exec into podman.
}

/// `introdus verify`.
pub fn run_verify() -> Result<()> {
    preflight::check_launch()?;
    let ctx = load_context()?;
    image::ensure(&ctx, false)?;
    run::verify(&ctx)
}

/// `introdus update`.
pub fn run_update() -> Result<()> {
    preflight::check_launch()?;
    run::update(&load_context()?)
}

/// `introdus rebuild-base`.
pub fn run_rebuild_base() -> Result<()> {
    preflight::check_launch()?;
    image::rebuild(&load_context()?)
}
