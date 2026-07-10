//! The `introdus` binary — control plane for the network-hardened dev-container
//! harness. Runs on the container host; drives podman + tmux + a full-screen
//! control TUI. See PLAN.md for the milestone roadmap.

use anyhow::Result;
use clap::{Parser, Subcommand};

/// introdus — launch and drive network-hardened dev containers for AI agents.
#[derive(Debug, Parser)]
#[command(name = "introdus", version = introdus_core::VERSION, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Ensure the tmux session, control TUI, and container are up (default).
    Launch,
    /// (internal) Run/start the podman container, streaming its logs.
    Up,
    /// (internal) Render the control TUI in the main-control pane.
    Menu,
    /// Run the egress smoke test without starting the workload.
    Verify,
    /// Remove and recreate the container (keeps the /home/dev volume).
    Recreate,
    /// Remove the container and wipe the /home/dev volume.
    Reset,
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
    match cli.command.unwrap_or(Command::Launch) {
        Command::Launch => not_yet("launch"),
        Command::Up => not_yet("up"),
        Command::Menu => not_yet("menu"),
        Command::Verify => not_yet("verify"),
        Command::Recreate => not_yet("recreate"),
        Command::Reset => not_yet("reset"),
        Command::Update => not_yet("update"),
        Command::RebuildBase => not_yet("rebuild-base"),
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
