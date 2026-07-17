//! pty test for the dev-machine `introdus notify-listen` wizard. With no flags,
//! no `RC_LISTEN_TCP`, and no saved config, a bare invocation prompts for the
//! SSH alias, the port, and whether to install a systemd unit. Driven under
//! `--dry-run` so it resolves the plan and prints it without binding the port,
//! opening a tunnel, or touching systemd.

mod common;

use common::{Fixture, TIMEOUT_MS};
use rexpect::session::spawn_command;

#[test]
fn ta137_notify_listen_wizard_then_dry_run_plan() {
    let fx = Fixture::new("laptop");
    let mut cmd = fx.cmd("notify-listen");
    cmd.arg("--dry-run");
    let mut p = spawn_command(cmd, TIMEOUT_MS).unwrap();

    // Wizard: SSH alias (required), port (default 8765), systemd confirm.
    p.exp_string("SSH alias/host of the container host")
        .unwrap();
    p.send("devhost\r").unwrap();
    p.flush().unwrap();
    p.exp_string("Loopback port").unwrap();
    p.send("\r").unwrap(); // accept default 8765
    p.flush().unwrap();
    p.exp_string("Install a systemd --user service").unwrap();
    p.send("n").unwrap(); // foreground, not a service
    p.flush().unwrap();

    // The dry-run plan echoes the resolved tunnel + listener.
    p.exp_string("notify-listen plan").unwrap();
    p.exp_string("127.0.0.1:8765").unwrap();
    p.exp_string("devhost").unwrap();
    p.exp_eof().unwrap();
}
