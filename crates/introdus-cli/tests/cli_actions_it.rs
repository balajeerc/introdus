//! End-to-end tests for the headless control-panel subcommands whose real work
//! is a `.env` round-trip — no container needed. Each spawns the *real* compiled
//! `introdus` binary against a throwaway project (isolated `HOME`) and asserts
//! the on-disk config change + exit code, proving the CLI wiring end to end.
//!
//! The container-touching subcommands (`restart`, `stop` of a live container,
//! `dev-shell`/`root-shell`, `agent`, `tunnel-url`, `blocked-egress`,
//! in-container `install-agent`/`install-paseo`, `paseo-url`) need a running
//! dev container and are proven by the test-harness `cli` driver instead.

mod common;
use common::Fixture;

/// Run `introdus <sub> [args…]` in the fixture project; return (success, stdout,
/// stderr).
fn run(fx: &Fixture, sub: &str, args: &[&str]) -> (bool, String, String) {
    let out = fx.cmd(sub).args(args).output().expect("spawn introdus");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn ta156_allow_appends_hosts_to_config() {
    let fx = Fixture::new("allowproj");
    fx.write_env("allowproj");
    // No `--restart`, so this is pure config edit + allowlist regen (no podman).
    let (ok, _out, err) = run(&fx, "allow", &["api.example.test", "cdn.example.test"]);
    assert!(ok, "allow failed: {err}");
    let cfg = std::fs::read_to_string(fx.env_file()).unwrap();
    assert!(
        cfg.contains("api.example.test") && cfg.contains("cdn.example.test"),
        "WHITELIST_HOSTS missing the added hosts:\n{cfg}"
    );
}

#[test]
fn ta156_ntfy_enables_push_in_config() {
    let fx = Fixture::new("ntfyproj");
    fx.write_env("ntfyproj");
    let (ok, _out, err) = run(&fx, "ntfy", &["--topic", "secret-topic"]);
    assert!(ok, "ntfy failed: {err}");
    let cfg = std::fs::read_to_string(fx.env_file()).unwrap();
    assert!(
        cfg.contains("ENABLE_NOTIFY_SH_ALERTS=true"),
        "ntfy didn't enable alerts:\n{cfg}"
    );
    assert!(
        cfg.contains("NTFY_SH_TOPIC=secret-topic"),
        "ntfy didn't record the topic:\n{cfg}"
    );
}

#[test]
fn ta156_expose_webapp_flips_the_flag() {
    let fx = Fixture::new("exposeproj");
    fx.write_env("exposeproj"); // writes EXPOSE_WEBAPP=false
    let (ok, _out, err) = run(&fx, "expose-webapp", &[]);
    assert!(ok, "expose-webapp failed: {err}");
    let cfg = std::fs::read_to_string(fx.env_file()).unwrap();
    assert!(
        cfg.contains("EXPOSE_WEBAPP=true"),
        "expose-webapp didn't flip the flag:\n{cfg}"
    );
}

#[test]
fn ta156_install_agent_rejects_unknown_id() {
    let fx = Fixture::new("unkproj");
    fx.write_env("unkproj");
    // Unknown ids fail fast (before any container check) and don't touch config.
    let (ok, _out, err) = run(&fx, "install-agent", &["definitely-not-an-agent"]);
    assert!(!ok, "unknown agent should exit non-zero");
    assert!(err.contains("unknown agent"), "unexpected error: {err}");
}

#[test]
fn ta156_stop_requires_yes() {
    let fx = Fixture::new("stopproj");
    fx.write_env("stopproj");
    let (ok, _out, err) = run(&fx, "stop", &[]);
    assert!(!ok, "stop without --yes should exit non-zero");
    assert!(err.contains("--yes"), "unexpected error: {err}");
}

#[test]
fn ta156_missing_required_args_are_rejected() {
    let fx = Fixture::new("argsproj");
    fx.write_env("argsproj");
    // clap gates required args before any of our code runs.
    assert!(!run(&fx, "allow", &[]).0, "allow needs a host");
    assert!(!run(&fx, "ntfy", &[]).0, "ntfy needs --topic");
    assert!(
        !run(&fx, "install-agent", &[]).0,
        "install-agent needs an id"
    );
}
