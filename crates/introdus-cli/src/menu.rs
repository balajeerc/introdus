//! The control TUI (`introdus menu`) — the persistent menu that runs in the
//! `main-control` tmux window. An `inquire::Select` loop dispatches to the
//! utilities in [`crate::menu_actions`]. Host-side, so it can read/write `.env`,
//! drive podman, open root/dev terminals, and spawn agent windows — the things
//! an in-container TUI could never do.

use anyhow::Result;
use inquire::Select;
use introdus_core::podman;

use crate::context::{env_path, LaunchContext};
use crate::menu_actions as act;
use introdus_core::Config;

/// The menu entries, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    TunnelUrl,
    ExposeWebapp,
    EnableNtfy,
    CopyFile,
    InstallAgent,
    LaunchAgent,
    BlockedEgress,
    AddAllowlist,
    RootTerminal,
    DevTerminal,
    TestNotify,
    Recreate,
    Reset,
    Refresh,
    Quit,
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Action::TunnelUrl => "Show tunnel URL",
            Action::ExposeWebapp => "Expose webapp via Cloudflare tunnel",
            Action::EnableNtfy => "Enable ntfy.sh mobile notifications",
            Action::CopyFile => "Copy a host file/folder into the container",
            Action::InstallAgent => "Install a coding agent",
            Action::LaunchAgent => "Launch an installed agent (tmux window)",
            Action::BlockedEgress => "List recently blocked egress URLs",
            Action::AddAllowlist => "Add hostnames to the egress allowlist",
            Action::RootTerminal => "Open a root terminal (tmux window)",
            Action::DevTerminal => "Open a dev terminal (tmux window)",
            Action::TestNotify => "Send a test host notification",
            Action::Recreate => "Recreate the container (apply .env changes)",
            Action::Reset => "Reset the container (wipe the volume)",
            Action::Refresh => "Refresh status",
            Action::Quit => "Quit this menu",
        };
        f.write_str(s)
    }
}

const ACTIONS: &[Action] = &[
    Action::TunnelUrl,
    Action::ExposeWebapp,
    Action::EnableNtfy,
    Action::CopyFile,
    Action::InstallAgent,
    Action::LaunchAgent,
    Action::BlockedEgress,
    Action::AddAllowlist,
    Action::RootTerminal,
    Action::DevTerminal,
    Action::TestNotify,
    Action::Recreate,
    Action::Reset,
    Action::Refresh,
    Action::Quit,
];

/// Run the control menu for the current project until the user quits.
pub fn run() -> Result<()> {
    let dir = std::env::current_dir()?;
    let env = env_path(&dir);
    loop {
        // Reload each iteration so actions that edited .env are reflected.
        let config = Config::load(&env)?;
        let ctx = LaunchContext::resolve(config, dir.clone())?;
        print_status(&ctx);

        let action = match Select::new("introdus — control", ACTIONS.to_vec()).prompt() {
            Ok(a) => a,
            // Esc / Ctrl-C leaves the menu without treating it as an error.
            Err(_) => break,
        };
        if action == Action::Quit {
            break;
        }
        if let Err(e) = dispatch(action, &ctx) {
            eprintln!("  ! {e:#}");
        }
        act::pause();
    }
    Ok(())
}

fn dispatch(action: Action, ctx: &LaunchContext) -> Result<()> {
    match action {
        Action::TunnelUrl => act::tunnel_url(ctx),
        Action::ExposeWebapp => act::toggle_expose_webapp(ctx),
        Action::EnableNtfy => act::enable_ntfy(ctx),
        Action::CopyFile => act::copy_file(ctx),
        Action::InstallAgent => act::install_agent(ctx),
        Action::LaunchAgent => act::launch_agent(ctx),
        Action::BlockedEgress => act::blocked_egress(ctx),
        Action::AddAllowlist => act::add_allowlist(ctx),
        Action::RootTerminal => act::open_terminal(ctx, None),
        Action::DevTerminal => act::open_terminal(ctx, Some("dev")),
        Action::TestNotify => act::test_notify(ctx),
        Action::Recreate => act::recreate(ctx),
        Action::Reset => act::reset(ctx),
        Action::Refresh | Action::Quit => Ok(()),
    }
}

fn print_status(ctx: &LaunchContext) {
    let running = podman::container_running(&ctx.container_name);
    let state = if running { "running" } else { "stopped" };
    println!("\n────────────────────────────────────────");
    println!(" project:   {}", ctx.config.project_name);
    println!(" container: {} ({state})", ctx.container_name);
    println!(" webapp:    port {}", ctx.config.webapp_port);
    println!(" agents:    {}", ctx.config.install_agents.join(", "));
    println!("────────────────────────────────────────");
}
