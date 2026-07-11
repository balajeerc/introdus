//! End-to-end pty test for the control menu (`introdus menu`). Runs against a
//! project whose container was never created, which is the exact case that used
//! to leak `Error: no such container` onto the screen — this is the regression
//! guard for that fix, plus a check that the grouped menu renders and quits.

mod common;

use common::{Fixture, TIMEOUT_MS};
use rexpect::session::spawn_command;

#[test]
fn ta80_menu_reports_not_created_without_leaking_podman_error() {
    let fx = Fixture::new("ship-tbc");
    fx.write_env("ship-tbc");

    let mut p = spawn_command(fx.cmd("menu"), TIMEOUT_MS).unwrap();

    // Everything printed before the inquire prompt is the status block.
    let status = p.exp_string("control (ship-tbc)").unwrap();
    assert!(
        status.contains("not created"),
        "absent container should read as 'not created':\n{status}"
    );
    assert!(
        !status.contains("no such container"),
        "podman inspect error leaked onto the menu:\n{status}"
    );

    // The grouped layout renders section headers, not just a flat list.
    p.exp_string("Container lifecycle").unwrap();

    // Type-to-filter down to Quit and select it; the menu exits cleanly.
    p.send_line("Quit").unwrap();
    p.exp_eof().unwrap();
}
