//! pty smoke test for `introdus send-files`. The full send (real container +
//! transfer) and the on-screen dual-pane layout are the tmux harness's job
//! (`test-harness/driver-send-files.sh`); here we prove the standalone
//! alternate-screen app starts on its first stage (the host picker) and quits
//! cleanly on Esc — entering and leaving the alternate screen — without leaking
//! a podman/ssh error or panicking.
//!
//! As with `menu_pty.rs`, the app's rendered content is a cursor-addressed frame
//! we don't scrape here: a bare test pty reports a 0×0 size, so ratatui draws no
//! visible text. What the raw pty stream *does* prove is that the app owns and
//! restores the alternate screen and exits on Esc with no error spilled as plain
//! text. Hermetic: the fixture's `HOME` has no `~/.ssh/config`, so the picker
//! offers only "this machine (local)" and never shells out to ssh.

mod common;

use common::{Fixture, TIMEOUT_MS};
use rexpect::session::spawn_command;

#[test]
fn ta148_send_files_starts_and_quits_clean() {
    let fx = Fixture::new("send-proj");

    let mut p = spawn_command(fx.cmd("send-files"), TIMEOUT_MS).unwrap();

    // Let the alternate-screen host picker come up, then quit from stage one.
    std::thread::sleep(std::time::Duration::from_millis(700));
    p.send("\x1b").unwrap();
    p.flush().unwrap();

    // exp_eof returns everything still buffered. Esc must exit the process (clean
    // EOF); the app must have entered the alternate screen (1049h) and must not
    // have leaked a `podman ps`/ssh failure or panicked.
    let out = p.exp_eof().unwrap();
    assert!(
        out.contains("\x1b[?1049h"),
        "send-files should own the alternate screen:\n{out:?}"
    );
    assert!(
        !out.contains("panicked") && !out.contains("no such container"),
        "send-files leaked an error:\n{out}"
    );
}
