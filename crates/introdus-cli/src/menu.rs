//! The control TUI (`introdus menu`) — the persistent full-screen ratatui menu
//! that runs in the `main-control` tmux window. The chooser lives in
//! [`crate::ui`]; this module owns the menu layout and dispatches selections to
//! the utilities in [`crate::menu_actions`]. Host-side, so it can read/write
//! `.env`, drive podman, open root/dev terminals, and spawn agent windows — the
//! things an in-container TUI could never do.

use anyhow::Result;
use introdus_core::podman::{self, ContainerState};

use crate::context::{env_path, LaunchContext};
use crate::menu_actions as act;
use crate::ui;
use introdus_core::Config;

/// A menu entry: either a selectable action or an inert section header (headers
/// give the flat list some visual grouping; selecting one just redraws).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Header(&'static str),
    Item(Action),
}

/// The selectable actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    RootTerminal,
    DevTerminal,
    LaunchAgent,
    InstallAgent,
    CopyFile,
    BlockedEgress,
    AddAllowlist,
    TunnelUrl,
    ExposeWebapp,
    EnableNtfy,
    TestNotify,
    Restart,
    Stop,
    Recreate,
    Reset,
    Destroy,
    Refresh,
    Quit,
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Action::RootTerminal => "Open a root terminal (tmux window)",
            Action::DevTerminal => "Open a dev terminal (tmux window)",
            Action::LaunchAgent => "Launch an installed agent (tmux window)",
            Action::InstallAgent => "Install a coding agent",
            Action::CopyFile => "Copy a host file/folder into the container",
            Action::BlockedEgress => "List recently blocked egress URLs",
            Action::AddAllowlist => "Add hostnames to the egress allowlist",
            Action::TunnelUrl => "Show tunnel URL",
            Action::ExposeWebapp => "Expose webapp via Cloudflare tunnel",
            Action::EnableNtfy => "Enable ntfy.sh mobile notifications",
            Action::TestNotify => "Send a test host notification",
            Action::Restart => "Restart the container",
            Action::Stop => "Stop the container",
            Action::Recreate => "Recreate the container (apply .env changes)",
            Action::Reset => "Reset the container (wipe the volume)",
            Action::Destroy => "Destroy the container (remove volume + key)",
            Action::Refresh => "Refresh status",
            Action::Quit => "Quit this menu",
        };
        f.write_str(s)
    }
}

const MENU: &[Row] = &[
    Row::Header("Terminals & agents"),
    Row::Item(Action::RootTerminal),
    Row::Item(Action::DevTerminal),
    Row::Item(Action::LaunchAgent),
    Row::Item(Action::InstallAgent),
    Row::Header("Files & egress"),
    Row::Item(Action::CopyFile),
    Row::Item(Action::BlockedEgress),
    Row::Item(Action::AddAllowlist),
    Row::Header("Webapp & notifications"),
    Row::Item(Action::TunnelUrl),
    Row::Item(Action::ExposeWebapp),
    Row::Item(Action::EnableNtfy),
    Row::Item(Action::TestNotify),
    Row::Header("Container lifecycle"),
    Row::Item(Action::Restart),
    Row::Item(Action::Stop),
    Row::Item(Action::Recreate),
    Row::Item(Action::Reset),
    Row::Item(Action::Destroy),
    Row::Header("Menu"),
    Row::Item(Action::Refresh),
    Row::Item(Action::Quit),
];

/// Run the control menu for the current project until the user quits.
pub fn run() -> Result<()> {
    let dir = std::env::current_dir()?;
    let env = env_path(&dir);
    loop {
        // Reload each iteration so actions that edited .env are reflected, and
        // re-snapshot the container state for the status panel.
        let config = Config::load(&env)?;
        let ctx = LaunchContext::resolve(config, dir.clone())?;
        let status = status_of(&ctx);
        let rows: Vec<ui::Row> = MENU
            .iter()
            .map(|r| match r {
                Row::Header(h) => ui::Row::Header((*h).to_owned()),
                Row::Item(a) => ui::Row::Item(a.to_string()),
            })
            .collect();

        // The chooser owns the alternate screen; it returns the index into MENU
        // of the picked item, or None when the user quit (Esc / Ctrl-C).
        let action = match ui::menu_select(&status, &rows)? {
            Some(idx) => match MENU[idx] {
                Row::Item(a) => a,
                Row::Header(_) => continue,
            },
            None => break,
        };
        match action {
            Action::Quit => break,
            // Refresh just falls through to the next loop, which re-snapshots.
            Action::Refresh => continue,
            _ => {
                if let Err(e) = dispatch(action, &ctx) {
                    eprintln!("  ! {e:#}");
                }
                ui::pause();
            }
        }
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
        Action::Restart => act::restart(ctx),
        Action::Stop => act::stop(ctx),
        Action::Recreate => act::recreate(ctx),
        Action::Reset => act::reset(ctx),
        Action::Destroy => act::destroy(ctx),
        Action::Refresh | Action::Quit => Ok(()),
    }
}

/// Snapshot the live status shown in the chooser's header panel.
fn status_of(ctx: &LaunchContext) -> ui::Status {
    let state = match podman::container_state(&ctx.container_name) {
        ContainerState::Running => "running",
        ContainerState::Stopped => "stopped",
        ContainerState::Absent => "not created",
    };
    ui::Status {
        project: ctx.config.project_name.clone(),
        container: ctx.container_name.clone(),
        state,
        webapp_port: ctx.config.webapp_port,
        agents: ctx.config.install_agents.join(", "),
    }
}
