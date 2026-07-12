//! The control TUI (`introdus menu`) — the persistent two-pane ratatui panel
//! that runs in the `main-control` tmux window. The panel itself (left status +
//! menu, right output pane) lives in [`crate::panel`]; this module owns the menu
//! layout and dispatches selections to the utilities in [`crate::menu_actions`].
//! Host-side, so it can read/write `.env`, drive podman, open root/dev
//! terminals, and spawn agent windows — the things an in-container TUI could
//! never do.

use anyhow::Result;
use introdus_core::podman::{self, ContainerState};

use crate::context::{env_path, LaunchContext};
use crate::menu_actions as act;
use crate::panel::{Selection, Ui};
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
    NotifyLog,
    Restart,
    Stop,
    Recreate,
    Reset,
    Destroy,
    Refresh,
    Quit,
    QuitStop,
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
            Action::NotifyLog => "Show the notification log",
            Action::Restart => "Restart the container",
            Action::Stop => "Stop the container",
            Action::Recreate => "Recreate the container (apply .env changes)",
            Action::Reset => "Reset the container (wipe the volume)",
            Action::Destroy => "Destroy the container (remove volume + key)",
            Action::Refresh => "Refresh status",
            Action::Quit => "Quit this menu (leave the container running)",
            Action::QuitStop => "Quit introdus (stop the container)",
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
    Row::Item(Action::NotifyLog),
    Row::Header("Container lifecycle"),
    Row::Item(Action::Restart),
    Row::Item(Action::Stop),
    Row::Item(Action::Recreate),
    Row::Item(Action::Reset),
    Row::Item(Action::Destroy),
    Row::Header("Menu"),
    Row::Item(Action::Refresh),
    Row::Item(Action::Quit),
    Row::Item(Action::QuitStop),
];

/// Run the control menu for the current project until the user quits. The
/// [`Ui`] owns the alternate screen for the whole session; each turn re-snapshots
/// the status/menu, then an action's output streams into the right-hand pane.
pub fn run() -> Result<()> {
    let dir = std::env::current_dir()?;
    let env = env_path(&dir);
    // Set when the user picks "Quit introdus (stop the container)": after the Ui
    // is torn down we kill this tmux session, closing every window.
    let mut kill_session: Option<String> = None;
    {
        let mut ui = Ui::new()?;
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
            ui.set_menu(status, rows);

            let action = match ui.run_menu()? {
                Selection::Item(idx) => match MENU[idx] {
                    Row::Item(a) => a,
                    Row::Header(_) => continue,
                },
                // A poll tick: re-snapshot the status (loop top does it) + redraw.
                Selection::Tick => continue,
                Selection::Quit => break,
            };
            match action {
                Action::Quit => break,
                // Refresh just falls through to the next loop, which re-snapshots.
                Action::Refresh => continue,
                // Stop the container, then break out and (below, after the Ui is
                // dropped) kill the whole session — closing every window.
                Action::QuitStop => {
                    ui.begin(&action.to_string());
                    match act::stop_for_quit(&ctx, &mut ui) {
                        Ok(true) => {
                            kill_session = Some(act::session_of(&ctx));
                            break;
                        }
                        Ok(false) => ui.drain_input(),
                        Err(e) => {
                            ui.log(format!("  ! {e:#}"));
                            ui.drain_input();
                        }
                    }
                }
                _ => {
                    ui.begin(&action.to_string());
                    if let Err(e) = dispatch(action, &ctx, &mut ui) {
                        ui.log(format!("  ! {e:#}"));
                    }
                    // Discard keys mashed while the (possibly blocking) action ran,
                    // so they don't fire as a cascade of unintended selections.
                    ui.drain_input();
                }
            }
        }
    } // Ui dropped here: alternate screen exited + terminal restored.

    if let Some(session) = kill_session {
        // Closes every window (this TUI's included); the detached notify service
        // self-exits once the session is gone.
        let _ = introdus_core::tmux::kill_session(&session);
    }
    Ok(())
}

fn dispatch(action: Action, ctx: &LaunchContext, ui: &mut Ui) -> Result<()> {
    match action {
        Action::TunnelUrl => act::tunnel_url(ctx, ui),
        Action::ExposeWebapp => act::toggle_expose_webapp(ctx, ui),
        Action::EnableNtfy => act::enable_ntfy(ctx, ui),
        Action::CopyFile => act::copy_file(ctx, ui),
        Action::InstallAgent => act::install_agent(ctx, ui),
        Action::LaunchAgent => act::launch_agent(ctx, ui),
        Action::BlockedEgress => act::blocked_egress(ctx, ui),
        Action::AddAllowlist => act::add_allowlist(ctx, ui),
        Action::RootTerminal => act::open_terminal(ctx, ui, None),
        Action::DevTerminal => act::open_terminal(ctx, ui, Some("dev")),
        Action::TestNotify => act::test_notify(ctx, ui),
        Action::NotifyLog => act::notify_log(ctx, ui),
        Action::Restart => act::restart(ctx, ui),
        Action::Stop => act::stop(ctx, ui),
        Action::Recreate => act::recreate(ctx, ui),
        Action::Reset => act::reset(ctx, ui),
        Action::Destroy => act::destroy(ctx, ui),
        // Handled directly in `run()` (they end the loop), never dispatched.
        Action::Refresh | Action::Quit | Action::QuitStop => Ok(()),
    }
}

/// Snapshot the live status shown in the panel's header.
fn status_of(ctx: &LaunchContext) -> ui::Status {
    let launching = crate::launch::is_launching(ctx);
    let state = match podman::container_state(&ctx.container_name) {
        ContainerState::Running => {
            // The container is up — the launch (if any) is done, so drop the
            // marker; a later Stop must read as "stopped", not "starting".
            crate::launch::clear_launch_marker(ctx);
            "running"
        }
        // A launch is underway but the container isn't running yet (still being
        // created, or existing-but-not-started): report it as starting.
        ContainerState::Stopped | ContainerState::Absent if launching => "starting container…",
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
