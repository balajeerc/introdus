//! The `introdus` binary — control plane for the network-hardened dev-container
//! harness. Runs on the container host; drives podman + tmux + a full-screen
//! control TUI. See PLAN.md for the milestone roadmap.

mod context;
mod image;
mod launch;
mod lifecycle;
mod menu;
mod menu_actions;
mod preflight;
mod run;
mod session;
mod util;
mod wizard;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

use launch::LaunchOpts;
use lifecycle::Lifecycle;

/// introdus — launch and drive network-hardened dev containers for AI agents.
#[derive(Debug, Parser)]
#[command(name = "introdus", version = introdus_core::VERSION, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

/// Flags shared by the launch-style subcommands.
#[derive(Debug, Clone, Copy, Args)]
struct LaunchArgs {
    /// On next start, fast-forward the project repo (git fetch + pull --ff-only).
    #[arg(long)]
    pull: bool,
    /// Run with NO egress filtering (all outbound permitted).
    #[arg(long)]
    disable_network_block: bool,
}

impl From<LaunchArgs> for LaunchOpts {
    fn from(a: LaunchArgs) -> Self {
        Self {
            pull: a.pull,
            disable_network_block: a.disable_network_block,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Ensure the tmux session, control TUI, and container are up (default).
    Launch(LaunchArgs),
    /// (internal) Run/start the podman container, streaming its logs.
    Up(LaunchArgs),
    /// (internal) Render the control TUI in the main-control pane.
    Menu,
    /// Run the egress smoke test without starting the workload.
    Verify,
    /// Remove and recreate the container (keeps the /home/dev volume).
    Recreate(LaunchArgs),
    /// Remove the container and wipe the /home/dev volume.
    Reset(LaunchArgs),
    /// In-container refresh: apt, mise, agents, LazyVim.
    Update,
    /// Rebuild the shared base image.
    RebuildBase,
    /// Host notification service: render / forward / ntfy push.
    NotifyHost,
    /// Dev-machine listener + ssh reverse tunnel for forwarded notifications.
    NotifyListen,
    /// Put the binary on PATH and set up host services.
    Install,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Launch(LaunchArgs {
        pull: false,
        disable_network_block: false,
    })) {
        // Launch drives the tmux session model; Up is the container run that
        // executes inside the session's dev-container window.
        Command::Launch(a) => session::launch(a.into()),
        Command::Up(a) => launch::run_launch(Lifecycle::Keep, a.into()),
        Command::Recreate(a) => launch::run_launch(Lifecycle::Recreate, a.into()),
        Command::Reset(a) => launch::run_launch(Lifecycle::Reset, a.into()),
        Command::Verify => launch::run_verify(),
        Command::Update => launch::run_update(),
        Command::RebuildBase => launch::run_rebuild_base(),
        Command::Menu => menu::run(),
        Command::NotifyHost => not_yet("notify-host"),
        Command::NotifyListen => not_yet("notify-listen"),
        Command::Install => not_yet("install"),
    }
}

/// Placeholder until each subcommand lands in its milestone.
fn not_yet(name: &str) -> Result<()> {
    anyhow::bail!(
        "`{} {name}` is not implemented yet — see PLAN.md",
        introdus_core::BIN_NAME
    )
}
