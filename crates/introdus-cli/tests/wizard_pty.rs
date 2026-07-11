//! End-to-end pty tests for the setup wizard, driven through a real pseudo-
//! terminal so the `inquire` prompts run exactly as a user sees them. Reached
//! via the standalone `introdus init` (no podman required).

mod common;

use common::{keygen, Fixture, TIMEOUT_MS};
use rexpect::session::{spawn_command, PtySession};

/// Answer the tail of the wizard shared by every branch (webapp port → ntfy),
/// through to the `.env` being written.
fn finish_wizard(p: &mut PtySession) {
    p.exp_string("Webapp port").unwrap();
    p.send_line("").unwrap(); // default 3000
    p.exp_string("Coding agents to install").unwrap();
    p.send_line("").unwrap(); // accept the Claude default
    p.exp_string("Expose the webapp").unwrap();
    p.send_line("").unwrap(); // default No
    p.exp_string("mobile push notifications").unwrap();
    p.send_line("").unwrap(); // default No
    p.exp_string("wrote").unwrap();
    p.exp_eof().unwrap();
}

#[test]
fn ta74_wizard_generates_a_new_key() {
    let fx = Fixture::new("ship-tbc");
    let mut p = spawn_command(fx.cmd("init"), TIMEOUT_MS).unwrap();

    p.exp_string("Project name").unwrap();
    p.send_line("").unwrap(); // default = dir name "ship-tbc"
    p.exp_string("Git repo URL").unwrap();
    p.send_line("git@github.com:o/ship-tbc.git").unwrap();
    p.exp_string("Generate a new per-project deploy key now")
        .unwrap();
    p.send_line("").unwrap(); // default Yes -> generate
    p.exp_string("Where should the new deploy key be created")
        .unwrap();
    p.send_line("").unwrap(); // accept the default path
                              // Registration step is shown for a freshly generated key.
    p.exp_string("Add this PUBLIC key").unwrap();
    p.exp_string("Press enter once the deploy key is registered")
        .unwrap();
    p.send_line("").unwrap();
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
    p.send_line("").unwrap();
    p.exp_string("Git repo URL").unwrap();
    p.send_line("git@github.com:o/ship-tbc.git").unwrap();
    p.exp_string("Generate a new per-project deploy key now")
        .unwrap();
    p.send_line("n").unwrap(); // No -> reuse-existing branch
    p.exp_string("Reuse the existing key at").unwrap();
    p.send_line("").unwrap(); // default Yes -> reuse it
                              // Registration is shown for a REUSED key too (the whole point of the fix).
    p.exp_string("Add this PUBLIC key").unwrap();
    p.exp_string("Press enter once the deploy key is registered")
        .unwrap();
    p.send_line("").unwrap();
    finish_wizard(&mut p);

    let env = std::fs::read_to_string(fx.env_file()).unwrap();
    assert!(
        env.contains(&format!("DEPLOY_KEY_PATH={}", key.display())),
        "reused key not recorded:\n{env}"
    );
}
