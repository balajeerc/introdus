//! The `introdus` binary — control plane for the network-hardened dev-container
//! harness. Runs on the container host; drives podman + tmux + a full-screen
//! control TUI. See PLAN.md for the milestone roadmap.

mod context;
mod image;
mod install;
mod launch;
mod lifecycle;
mod menu;
mod menu_actions;
mod notify;
mod notify_listen;
mod panel;
mod preflight;
mod run;
#[cfg(test)]
mod screenshot;
mod send_files;
mod session;
mod ui;
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

impl From<NotifyListenArgs> for notify_listen::Options {
    fn from(a: NotifyListenArgs) -> Self {
        Self {
            via: a.via,
            port: a.port,
            install_service: a.install_service,
            no_tunnel: a.no_tunnel,
            dry_run: a.dry_run,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Ensure the tmux session, control TUI, and container are up (default).
    Launch(LaunchArgs),
    /// Run the setup wizard standalone: (re)write this project's .env without
    /// launching a container. No podman required.
    Init,
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
    NotifyListen(NotifyListenArgs),
    /// Dual-pane file browser to send files/folders into a running introdus
    /// container — on this machine or an ssh-reachable host.
    SendFiles,
    /// Put the binary on PATH and set up host services.
    Install,
}

/// Flags for `introdus notify-listen` (the dev-machine side). With no `--via`
/// and no `--port` (and nothing saved from a prior run), an interactive wizard
/// collects them.
#[derive(Debug, Clone, Args)]
struct NotifyListenArgs {
    /// SSH alias/host to open the reverse tunnel to (the container host, as named
    /// in your `~/.ssh/config`). Omit to be prompted / use the saved value.
    #[arg(long)]
    via: Option<String>,
    /// Loopback port used on both ends of the tunnel and by the listener
    /// (must match `RC_FORWARD_ADDR` on the host). Defaults to 8765.
    #[arg(long)]
    port: Option<u16>,
    /// Install and enable a `systemd --user` unit that runs this on each login,
    /// instead of running in the foreground now.
    #[arg(long)]
    install_service: bool,
    /// Only run the listener; don't manage the ssh reverse tunnel (bring your own).
    #[arg(long)]
    no_tunnel: bool,
    /// Resolve settings (running the wizard if needed) and print the plan without
    /// binding the port, opening the tunnel, or touching systemd.
    #[arg(long)]
    dry_run: bool,
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
        Command::Init => run_init(),
        Command::Up(a) => launch::run_launch(Lifecycle::Keep, a.into()),
        Command::Recreate(a) => launch::run_launch(Lifecycle::Recreate, a.into()),
        Command::Reset(a) => launch::run_launch(Lifecycle::Reset, a.into()),
        Command::Verify => launch::run_verify(),
        Command::Update => launch::run_update(),
        Command::RebuildBase => launch::run_rebuild_base(),
        Command::Menu => menu::run(),
        Command::NotifyHost => notify::run_host(),
        Command::NotifyListen(a) => notify_listen::run(a.into()),
        Command::SendFiles => send_files::run(),
        Command::Install => install::run(),
    }
}

/// Run the setup wizard for the current directory, standalone. If a config
/// already exists, confirm before overwriting it.
fn run_init() -> Result<()> {
    let dir = std::env::current_dir()?;
    // Offer to relocate a legacy `./.env` before we decide what to reconfigure.
    context::migrate_legacy_config(&dir)?;
    let env = context::env_path(&dir);
    if env.exists()
        && !ui::confirm(
            &format!("{} exists — reconfigure it?", env.display()),
            false,
        )?
    {
        println!("  left config unchanged.");
        return Ok(());
    }
    wizard::run(&dir)?;
    Ok(())
}
