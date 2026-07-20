//! The output surface shared by the interactive control panel and the headless
//! CLI, so the reusable menu-action cores can drive either without caring which.
//!
//! Only the *output* side is abstracted here — progress lines ([`Frontend::log`])
//! and long streaming subcommands ([`Frontend::run_task`]). Interactive prompts
//! (confirm / text / pick) stay on the panel's [`crate::panel::Ui`]: the CLI
//! resolves those decisions from flags instead of asking, so the shared cores are
//! written to take their inputs as parameters and never prompt through here.
//!
//! Two implementors: [`crate::panel::Ui`] (streams into the panel's output pane,
//! under the alternate-screen capture guard) and [`StdioFrontend`] (plain stdout,
//! for one-shot CLI subcommands — the child inherits the terminal directly).

use anyhow::{Context, Result};
use introdus_core::process::Cmd;

/// The output surface a reusable action writes its progress to.
pub trait Frontend {
    /// Append one progress line.
    fn log(&mut self, line: impl Into<String>);
    /// Run `cmd` as a foreground task, surfacing its output live; error on a
    /// non-zero exit. `label` names it for the progress indicator / error.
    fn run_task(&mut self, label: &str, cmd: Cmd) -> Result<()>;
}

/// A headless [`Frontend`] for one-shot CLI subcommands: progress goes to stdout
/// and, because no [`introdus_core::process::capture_stdio`] guard is active off
/// the TUI, each subcommand inherits the terminal and streams its output live.
pub struct StdioFrontend;

impl Frontend for StdioFrontend {
    fn log(&mut self, line: impl Into<String>) {
        println!("{}", line.into());
    }

    fn run_task(&mut self, label: &str, cmd: Cmd) -> Result<()> {
        // No capture guard on the CLI, so `run()` inherits stdio: the child's
        // output streams straight to the terminal and non-zero exits error.
        cmd.run().with_context(|| format!("`{label}` failed"))
    }
}
