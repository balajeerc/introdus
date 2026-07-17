//! The dev-machine (laptop) side of notifications: `introdus notify-listen`.
//!
//! It resolves where to listen and where to tunnel from — flags, then env, then
//! a saved config, then an interactive wizard — and then either runs in the
//! foreground (opening an ssh reverse tunnel to the container host and rendering
//! events) or installs a `systemd --user` unit that does the same on each login.
//!
//! The actual accept-loop + rendering lives in [`crate::notify`]; this module
//! owns everything *around* it: settings, the wizard, tunnel supervision, and
//! the systemd unit.
//!
//! Deliberately **no linger.** The unit is `WantedBy=default.target` and starts
//! with the user's graphical session, so `notify-send`/`paplay` inherit the
//! session's D-Bus and display. A boot-time lingering service would fire
//! notifications into a session that isn't there.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use anyhow::{Context, Result};
use introdus_core::env_file::{quote_scalar, read_map};
use introdus_core::paths;

use crate::util::have;

/// Default loopback port for the tunnel + listener (must match the host's
/// `RC_FORWARD_ADDR`).
const DEFAULT_PORT: u16 = 8765;

/// The `systemd --user` unit name.
const UNIT_NAME: &str = "introdus-notify-listen.service";

/// Parsed `introdus notify-listen` flags (see `main.rs`).
pub struct Options {
    pub via: Option<String>,
    pub port: Option<u16>,
    pub install_service: bool,
    pub no_tunnel: bool,
    pub dry_run: bool,
}

/// The fully-resolved plan a run acts on.
struct Resolved {
    /// The ssh alias/host to reverse-tunnel from; `None` = listener only.
    via: Option<String>,
    port: u16,
    install_service: bool,
}

/// `introdus notify-listen`.
pub fn run(opts: Options) -> Result<()> {
    let resolved = resolve(&opts)?;
    if opts.dry_run {
        return print_plan(&resolved);
    }
    // Remember a usable tunnel target so the next bare run skips the wizard.
    if resolved.via.is_some() {
        save_settings(resolved.via.as_deref(), resolved.port)?;
    }
    if resolved.install_service {
        return install_service(&resolved);
    }
    run_foreground(&resolved)
}

// ---- resolution -------------------------------------------------------------

/// Resolve `via`/`port`/service from flags → env → saved config → wizard. Only
/// runs the wizard when nothing at all was supplied and a tunnel is wanted.
fn resolve(opts: &Options) -> Result<Resolved> {
    let saved = load_settings()?;
    let env_port = std::env::var("RC_LISTEN_TCP")
        .ok()
        .and_then(|v| parse_port(&v));
    let flags_present = opts.via.is_some() || opts.port.is_some() || env_port.is_some();

    let mut resolved = if !flags_present && saved.is_none() && !opts.no_tunnel {
        let w = wizard()?;
        Resolved {
            via: Some(w.via),
            port: w.port,
            install_service: opts.install_service || w.install_service,
        }
    } else {
        Resolved {
            via: opts
                .via
                .clone()
                .or_else(|| saved.as_ref().and_then(|s| s.via.clone())),
            port: opts
                .port
                .or(env_port)
                .or_else(|| saved.as_ref().map(|s| s.port))
                .unwrap_or(DEFAULT_PORT),
            install_service: opts.install_service,
        }
    };
    if opts.no_tunnel {
        resolved.via = None;
    }
    Ok(resolved)
}

/// Accept `PORT` or `host:PORT` (how `RC_LISTEN_TCP` is documented), taking the
/// trailing port field.
fn parse_port(v: &str) -> Option<u16> {
    v.trim().rsplit(':').next().and_then(|t| t.parse().ok())
}

// ---- saved settings ---------------------------------------------------------

/// Persisted dev-machine settings, in the same `.env` format as project config.
struct Settings {
    via: Option<String>,
    port: u16,
}

fn load_settings() -> Result<Option<Settings>> {
    let path = paths::notify_listen_config()?;
    if !path.exists() {
        return Ok(None);
    }
    let m = read_map(&path)?;
    let via = m
        .get("RC_VIA_ALIAS")
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let port = m
        .get("RC_LISTEN_TCP")
        .and_then(|v| parse_port(v))
        .unwrap_or(DEFAULT_PORT);
    Ok(Some(Settings { via, port }))
}

fn save_settings(via: Option<&str>, port: u16) -> Result<()> {
    let path = paths::notify_listen_config()?;
    let mut body = String::new();
    body.push_str("# introdus notify-listen (dev machine). Generated/edited by `introdus`.\n");
    if let Some(via) = via {
        let _ = writeln!(body, "RC_VIA_ALIAS={}", quote_scalar(via));
    }
    let _ = writeln!(body, "RC_LISTEN_TCP={port}");
    std::fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

// ---- wizard -----------------------------------------------------------------

struct Wizard {
    via: String,
    port: u16,
    install_service: bool,
}

fn wizard() -> Result<Wizard> {
    println!("\n=== introdus notify-listen setup ===");
    println!("Runs on your dev machine: opens an ssh reverse tunnel to the container");
    println!("host and renders task-done / awaiting-input notifications locally.\n");
    let via =
        crate::wizard::ask_nonempty("SSH alias/host of the container host (from ~/.ssh/config)")?;
    let port = crate::wizard::ask_port(
        "Loopback port (must match RC_FORWARD_ADDR on the host)",
        DEFAULT_PORT,
    )?;
    let install_service = crate::ui::confirm(
        "Install a systemd --user service so this starts on each login?",
        true,
    )?;
    Ok(Wizard {
        via,
        port,
        install_service,
    })
}

// ---- foreground -------------------------------------------------------------

fn run_foreground(r: &Resolved) -> Result<()> {
    let addr = format!("127.0.0.1:{}", r.port);
    // Bind first so a port clash fails fast, before we open any tunnel.
    let listener = crate::notify::bind_listener(&addr)?;
    let _tunnel = match &r.via {
        Some(via) => {
            let t = Tunnel::spawn(via, r.port)?;
            println!(
                "rc-notify: reverse tunnel up via {via} ({p}:127.0.0.1:{p})",
                p = r.port
            );
            Some(t)
        }
        None => None,
    };
    println!("rc-notify: listening on tcp://{addr}");
    crate::notify::serve_listener(listener)
}

/// A supervised ssh reverse tunnel, killed when this guard drops. Interactive
/// Ctrl-C already signals the whole foreground process group (so the child dies
/// too); the guard covers the non-signal exits (a bind error after spawn, etc.).
struct Tunnel(Child);

impl Tunnel {
    fn spawn(via: &str, port: u16) -> Result<Self> {
        let (prog, args) = tunnel_argv(have("autossh"), via, port);
        let child = Command::new(&prog)
            .args(&args)
            .spawn()
            .with_context(|| format!("spawning `{prog}` reverse tunnel to {via}"))?;
        Ok(Self(child))
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Build the reverse-tunnel command. `autossh -M 0` delegates liveness to ssh
/// keepalives, so those `-o` options are load-bearing (without them a silently
/// dropped tunnel never reconnects). `ExitOnForwardFailure` makes a port clash on
/// the host fail loudly instead of leaving a live-but-useless tunnel.
fn tunnel_argv(use_autossh: bool, via: &str, port: u16) -> (String, Vec<String>) {
    let mut args: Vec<String> = Vec::new();
    let prog = if use_autossh {
        // -M 0 disables autossh's own monitoring port; keepalives do the work.
        args.push("-M".into());
        args.push("0".into());
        "autossh"
    } else {
        "ssh"
    };
    args.push("-N".into());
    for opt in [
        "ExitOnForwardFailure=yes",
        "ServerAliveInterval=30",
        "ServerAliveCountMax=3",
    ] {
        args.push("-o".into());
        args.push(opt.into());
    }
    args.push("-R".into());
    args.push(format!("{port}:127.0.0.1:{port}"));
    args.push(via.into());
    (prog.into(), args)
}

// ---- systemd --user service -------------------------------------------------

fn install_service(r: &Resolved) -> Result<()> {
    let via = r
        .via
        .clone()
        .context("--install-service needs a tunnel target; pass --via <ssh-alias>")?;
    let bin = std::env::current_exe().context("cannot determine the introdus binary path")?;
    let unit_path = systemd_user_dir()?.join(UNIT_NAME);
    let desired = render_unit(&bin, &via, r.port);

    // Idempotency: if the unit is already active with the exact same definition,
    // there is nothing to do — don't churn a healthy tunnel.
    let active = systemctl(&["is-active", "--quiet", UNIT_NAME]);
    if active && std::fs::read_to_string(&unit_path).ok().as_deref() == Some(desired.as_str()) {
        println!("  {UNIT_NAME} is already running with these settings — nothing to do.");
        return Ok(());
    }

    if let Some(parent) = unit_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&unit_path, &desired)
        .with_context(|| format!("writing {}", unit_path.display()))?;
    println!("  wrote {}", unit_path.display());
    require_systemctl(&["daemon-reload"])?;
    require_systemctl(&["enable", UNIT_NAME])?;
    require_systemctl(&["restart", UNIT_NAME])?;
    println!("  enabled + started {UNIT_NAME} — it now runs on each login.");
    println!("  (no linger: it starts with your graphical session so desktop popups work.");
    println!("   status: systemctl --user status {UNIT_NAME})");
    Ok(())
}

/// The `~/.config/systemd/user` directory (where user units live).
fn systemd_user_dir() -> Result<PathBuf> {
    let base = dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .context("cannot determine ~/.config for the systemd user unit")?;
    Ok(base.join("systemd/user"))
}

/// Render the unit file. The `ExecStart` re-invokes this binary with explicit
/// flags, so the running service is self-describing and skips the wizard.
fn render_unit(bin: &Path, via: &str, port: u16) -> String {
    format!(
        "[Unit]\n\
         Description=introdus notification listener (dev machine)\n\
         After=default.target\n\
         \n\
         [Service]\n\
         ExecStart={bin} notify-listen --via {via} --port {port}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        bin = bin.display(),
    )
}

/// Run `systemctl --user <args>`, returning whether it exited zero (for probes).
fn systemctl(args: &[&str]) -> bool {
    introdus_core::process::Cmd::new("systemctl")
        .arg("--user")
        .args(args)
        .ok()
}

/// Run `systemctl --user <args>`, erroring on a non-zero exit.
fn require_systemctl(args: &[&str]) -> Result<()> {
    introdus_core::process::Cmd::new("systemctl")
        .arg("--user")
        .args(args)
        .run()
        .with_context(|| format!("systemctl --user {}", args.join(" ")))
}

// ---- dry run ----------------------------------------------------------------

fn print_plan(r: &Resolved) -> Result<()> {
    println!("notify-listen plan (dry run):");
    println!("  listener:  127.0.0.1:{}", r.port);
    match &r.via {
        Some(via) => {
            let (prog, args) = tunnel_argv(have("autossh"), via, r.port);
            println!("  tunnel:    {prog} {}", args.join(" "));
        }
        None => println!("  tunnel:    (none — listener only)"),
    }
    println!(
        "  service:   {}",
        if r.install_service {
            "install systemd --user unit"
        } else {
            "run in foreground"
        }
    );
    if let Ok(p) = paths::notify_listen_config() {
        println!("  config:    {}", p.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta133_parse_port_accepts_bare_and_hostport() {
        assert_eq!(parse_port("8765"), Some(8765));
        assert_eq!(parse_port(" 8765 "), Some(8765));
        assert_eq!(parse_port("127.0.0.1:8765"), Some(8765));
        assert_eq!(parse_port("nope"), None);
        assert_eq!(parse_port(""), None);
    }

    #[test]
    fn ta134_tunnel_argv_autossh_vs_ssh() {
        let (prog, args) = tunnel_argv(true, "myhost", 8765);
        assert_eq!(prog, "autossh");
        // autossh needs -M 0 and the keepalive options, or -M 0 never reconnects.
        assert_eq!(args[0], "-M");
        assert_eq!(args[1], "0");
        assert!(args.contains(&"ServerAliveInterval=30".to_owned()));
        assert!(args.contains(&"ExitOnForwardFailure=yes".to_owned()));
        assert!(args.contains(&"8765:127.0.0.1:8765".to_owned()));
        assert_eq!(args.last().unwrap(), "myhost");

        let (prog, args) = tunnel_argv(false, "myhost", 9000);
        assert_eq!(prog, "ssh");
        // Plain ssh has no -M flag but keeps the keepalives + forward spec.
        assert!(!args.contains(&"-M".to_owned()));
        assert!(args.contains(&"ServerAliveInterval=30".to_owned()));
        assert!(args.contains(&"9000:127.0.0.1:9000".to_owned()));
        assert_eq!(args.last().unwrap(), "myhost");
    }

    #[test]
    fn ta135_render_unit_is_no_linger_default_target() {
        let unit = render_unit(Path::new("/home/me/.local/bin/introdus"), "devhost", 8765);
        assert!(unit.contains("WantedBy=default.target"));
        assert!(unit.contains(
            "ExecStart=/home/me/.local/bin/introdus notify-listen --via devhost --port 8765"
        ));
        assert!(unit.contains("Restart=on-failure"));
        // Never a linger/boot target — notifications need the graphical session.
        assert!(!unit.to_lowercase().contains("linger"));
    }
}
