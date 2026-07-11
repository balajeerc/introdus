//! End-to-end pty tests for the setup wizard, driven through a real pseudo-
//! terminal. The wizard is now a sequence of `ratatui` inline modal prompts
//! (confirm / text / checklist) rather than `inquire`, so we drive it with
//! explicit keystrokes: `\r` is the byte a terminal sends for Enter in raw mode
//! (the same thing tmux emits for the control-menu harness). Each modal renders
//! its prompt text as a contiguous run, so `exp_string` still synchronizes on
//! the prompt wording. Reached via the standalone `introdus init` (no podman).

mod common;

use common::{keygen, Fixture, TIMEOUT_MS};
use rexpect::session::{spawn_command, PtySession};

/// Send a burst of raw bytes and flush — no trailing newline is added, so
/// callers spell out `\r` (Enter) themselves.
fn send(p: &mut PtySession, bytes: &str) {
    p.send(bytes).unwrap();
    p.flush().unwrap();
}

/// Accept a modal's current value (Enter).
fn enter(p: &mut PtySession) {
    send(p, "\r");
}

/// Answer the tail of the wizard shared by every branch (webapp port → ntfy),
/// through to the `.env` being written. Every step here takes its default, so
/// it's a bare Enter each time.
fn finish_wizard(p: &mut PtySession) {
    p.exp_string("Webapp port").unwrap();
    enter(p); // default 3000
    p.exp_string("Coding agents to install").unwrap();
    enter(p); // accept the Claude default (pre-checked)
    p.exp_string("Expose the webapp").unwrap();
    enter(p); // default No
    p.exp_string("mobile push notifications").unwrap();
    enter(p); // default No
    p.exp_string("wrote").unwrap();
    p.exp_eof().unwrap();
}

#[test]
fn ta74_wizard_generates_a_new_key() {
    let fx = Fixture::new("ship-tbc");
    let mut p = spawn_command(fx.cmd("init"), TIMEOUT_MS).unwrap();

    p.exp_string("Project name").unwrap();
    enter(&mut p); // default = dir name "ship-tbc"
    p.exp_string("Git repo URL").unwrap();
    send(&mut p, "git@github.com:o/ship-tbc.git\r");
    p.exp_string("Generate a new per-project deploy key now")
        .unwrap();
    enter(&mut p); // default Yes -> generate
    p.exp_string("Where should the new deploy key be created")
        .unwrap();
    enter(&mut p); // accept the default path
                   // Registration step is shown for a freshly generated key.
    p.exp_string("Add this PUBLIC key").unwrap();
    p.exp_string("Press enter once the deploy key is registered")
        .unwrap();
    enter(&mut p);
    finish_wizard(&mut p);

    let key = fx.deploy_keys_dir().join("ship-tbc-deploy-key");
    assert!(key.exists(), "generated private key is missing");
    assert!(
        key.with_extension("pub").exists(),
        "generated .pub is missing"
    );
    let env = std::fs::read_to_string(fx.env_file()).unwrap();
    assert!(env.contains("PROJECT_NAME=ship-tbc"), "{env}");
    assert!(
        env.contains(&format!("DEPLOY_KEY_PATH={}", key.display())),
        "{env}"
    );
}

#[test]
fn ta75_wizard_reuses_a_matching_key_and_still_shows_registration() {
    let fx = Fixture::new("ship-tbc");
    // A pre-existing key whose name matches the project — the reuse flow should
    // find it and offer it via a plain yes/no.
    let key = fx.deploy_keys_dir().join("ship-tbc-deploy-key");
    keygen(&key);

    let mut p = spawn_command(fx.cmd("init"), TIMEOUT_MS).unwrap();
    p.exp_string("Project name").unwrap();
    enter(&mut p);
    p.exp_string("Git repo URL").unwrap();
    send(&mut p, "git@github.com:o/ship-tbc.git\r");
    p.exp_string("Generate a new per-project deploy key now")
        .unwrap();
    send(&mut p, "n\r"); // No -> reuse-existing branch
    p.exp_string("Reuse the existing key at").unwrap();
    enter(&mut p); // default Yes -> reuse it
                   // Registration is shown for a REUSED key too (the whole point of the fix).
    p.exp_string("Add this PUBLIC key").unwrap();
    p.exp_string("Press enter once the deploy key is registered")
        .unwrap();
    enter(&mut p);
    finish_wizard(&mut p);

    let env = std::fs::read_to_string(fx.env_file()).unwrap();
    assert!(
        env.contains(&format!("DEPLOY_KEY_PATH={}", key.display())),
        "reused key not recorded:\n{env}"
    );
}
