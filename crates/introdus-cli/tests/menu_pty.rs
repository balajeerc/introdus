//! End-to-end pty test for the control menu (`introdus menu`). Runs against a
//! project whose container was never created — the exact case that used to leak
//! `Error: no such container` onto the screen. This is the regression guard for
//! that fix plus a smoke test that the full-screen ratatui menu starts and quits
//! cleanly.
//!
//! The menu is now a full-screen ratatui app, so its rendered content is a
//! cursor-addressed frame we don't scrape byte-for-byte here (the tmux harness
//! in `test-harness/driver-menu.sh` covers the on-screen layout). What we *can*
//! assert from the raw pty stream is that no `podman inspect` error leaks as
//! plain stderr, and that Esc quits the app with a clean EOF.

mod common;

use common::{Fixture, TIMEOUT_MS};
use rexpect::session::spawn_command;

#[test]
fn ta80_menu_reports_not_created_without_leaking_podman_error() {
    let fx = Fixture::new("ship-tbc");
    fx.write_env("ship-tbc");

    let mut p = spawn_command(fx.cmd("menu"), TIMEOUT_MS).unwrap();

    // Give the alternate-screen app a moment to render, then quit with Esc.
    std::thread::sleep(std::time::Duration::from_millis(500));
    p.send("\x1b").unwrap();
    p.flush().unwrap();

    // exp_eof returns everything still buffered. A leaked `podman inspect` on the
    // absent container would surface here as a plain-text stderr line; the fix
    // (gating on container_exists) means it must not.
    let rest = p.exp_eof().unwrap();
    assert!(
        !rest.contains("no such container"),
        "podman inspect error leaked from the menu:\n{rest}"
    );
}
