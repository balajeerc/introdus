//! The host notification service — the Rust fold-in of `host_listener.py` and
//! `host_notify.sh`.
//!
//! `introdus notify-host` serves the endpoint a container writes events to (a
//! FIFO on Linux, a unix socket on macOS), validates each event through the
//! [`Notification`] trust boundary, and renders it: an optional ntfy.sh push,
//! then either a forward over TCP (headless remote host) or a local desktop
//! popup + sound. `introdus notify-listen` is the laptop side — it accepts the
//! forwarded events over a loopback TCP port (fed by an ssh reverse tunnel) and
//! renders locally.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

use anyhow::{Context, Result};
use introdus_core::notify::{Notification, READ_LIMIT};
use introdus_core::paths;
use introdus_core::process::Cmd;

/// The embedded notification sound, materialized to the state dir on first use.
const SOUND_WAV: &[u8] = include_bytes!("../../../notification_sound.wav");

/// Rendering configuration, read from the environment the service was started
/// with (launch exports these from the project `.env`).
struct NotifyConfig {
    enable_ntfy: bool,
    ntfy_topic: Option<String>,
    forward_addr: Option<String>,
    no_forward: bool,
}

impl NotifyConfig {
    fn from_env() -> Self {
        let enable_ntfy = std::env::var("ENABLE_NOTIFY_SH_ALERTS").as_deref() == Ok("true");
        let ntfy_topic = non_empty_env("NTFY_SH_TOPIC");
        let forward_addr = non_empty_env("RC_FORWARD_ADDR");
        let no_forward = std::env::var("RC_NO_FORWARD").as_deref() == Ok("1");
        Self {
            enable_ntfy,
            ntfy_topic,
            forward_addr,
            no_forward,
        }
    }
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

/// The Linux FIFO endpoint a container writes to, bind-mounted at `/run/notify`.
pub fn fifo_path() -> Result<PathBuf> {
    let runtime = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", uid())));
    Ok(runtime.join("rc-notify.fifo"))
}

/// Ensure `path` exists as a FIFO (created 0600). Idempotent: an existing FIFO
/// is reused so a bind-mount into a running container isn't invalidated.
pub fn ensure_fifo(path: &std::path::Path) -> Result<()> {
    if is_fifo(path) {
        return Ok(());
    }
    let _ = std::fs::remove_file(path);
    Cmd::new("mkfifo")
        .args(["-m", "600"])
        .arg(path)
        .run()
        .context("mkfifo failed")
}

/// `introdus notify-host`: serve the local endpoint and render events.
pub fn run_host() -> Result<()> {
    let cfg = NotifyConfig::from_env();
    let path = fifo_path()?;
    ensure_fifo(&path)?;
    // Launched detached (no tmux window of its own), so bind its lifetime to the
    // owning tmux session — exit once that session is gone.
    spawn_session_watcher();
    println!("rc-notify: reading FIFO {}", path.display());
    loop {
        // open() blocks until a writer connects; the loop yields lines until all
        // writers close (EOF), then we reopen for the next event.
        let file = std::fs::File::open(&path)
            .with_context(|| format!("opening FIFO {}", path.display()))?;
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            handle(&line, &cfg);
        }
    }
}

/// `introdus notify-listen`: laptop side. Accept forwarded events over the
/// loopback TCP port and render locally (never re-forward).
pub fn run_listen() -> Result<()> {
    let listen = non_empty_env("RC_LISTEN_TCP")
        .context("RC_LISTEN_TCP must be set (host:port or port) for notify-listen")?;
    let addr = if listen.contains(':') {
        listen
    } else {
        format!("127.0.0.1:{listen}")
    };
    // Force local rendering regardless of any RC_FORWARD_ADDR in the env.
    std::env::set_var("RC_NO_FORWARD", "1");
    let cfg = NotifyConfig::from_env();
    let listener = TcpListener::bind(&addr).with_context(|| format!("binding {addr}"))?;
    println!("rc-notify: listening on tcp://{addr}");
    for mut stream in listener.incoming().flatten() {
        let mut buf = vec![0u8; READ_LIMIT];
        if let Ok(n) = stream.read(&mut buf) {
            handle(&String::from_utf8_lossy(&buf[..n]), &cfg);
        }
    }
    Ok(())
}

/// When `RC_SESSION` is set (the detached-service case), poll for that tmux
/// session and exit the process once it disappears, so the background service
/// never lingers past the session it belongs to. The watcher runs on its own
/// thread so it fires even while the main thread is blocked opening the FIFO.
fn spawn_session_watcher() {
    let Some(session) = non_empty_env("RC_SESSION") else {
        return;
    };
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(5));
        if !introdus_core::tmux::has_session(&session) {
            std::process::exit(0);
        }
    });
}

fn handle(raw: &str, cfg: &NotifyConfig) {
    let capped = &raw[..raw.len().min(READ_LIMIT)];
    match Notification::parse(capped) {
        Some(n) => render(&n, cfg),
        None => eprintln!(
            "rc-notify: rejecting unknown event {:?}",
            &capped.trim()[..capped.trim().len().min(32)]
        ),
    }
}

fn render(n: &Notification, cfg: &NotifyConfig) {
    send_ntfy(n, cfg);
    if let Some(addr) = &cfg.forward_addr {
        if !cfg.no_forward {
            if let Err(e) = forward(addr, n) {
                eprintln!("rc-notify: forward to {addr} failed ({e})");
            }
            return;
        }
    }
    if let Err(e) = desktop(n) {
        eprintln!("rc-notify: {}: {}", n.title(), e);
    }
}

/// Fire-and-forget ntfy.sh push (does not block local rendering).
fn send_ntfy(n: &Notification, cfg: &NotifyConfig) {
    if !cfg.enable_ntfy {
        return;
    }
    let Some(topic) = &cfg.ntfy_topic else {
        eprintln!("rc-notify: ENABLE_NOTIFY_SH_ALERTS=true but NTFY_SH_TOPIC unset");
        return;
    };
    let _ = Cmd::new("curl")
        .args(["-fsS", "--max-time", "5"])
        .args(["-H", &format!("Title: {}", n.title())])
        .args(["-H", "Tags: bell"])
        .args(["-d", n.event.body()])
        .arg(format!("https://ntfy.sh/{topic}"))
        .ok();
}

/// Forward the validated event over TCP, preserving the `event<TAB>label` wire
/// format so the laptop listener renders it identically.
fn forward(addr: &str, n: &Notification) -> Result<()> {
    let msg = if n.label.is_empty() {
        format!("{}\n", n.event.keyword())
    } else {
        format!("{}\t{}\n", n.event.keyword(), n.label)
    };
    let mut stream = TcpStream::connect(addr).with_context(|| format!("connect {addr}"))?;
    stream.write_all(msg.as_bytes())?;
    Ok(())
}

fn desktop(n: &Notification) -> Result<()> {
    if std::env::consts::OS == "macos" {
        desktop_macos(n)
    } else {
        desktop_linux(n)
    }
}

fn desktop_macos(n: &Notification) -> Result<()> {
    if let Ok(sound) = sound_file() {
        let _ = Cmd::new("afplay").arg(sound).ok();
    }
    Cmd::new("osascript")
        .args([
            "-e",
            &format!(
                "display notification \"{}\" with title \"{}\"",
                n.event.body(),
                n.title()
            ),
        ])
        .run()
}

fn desktop_linux(n: &Notification) -> Result<()> {
    play_sound_linux();
    show_notification_linux(n)
}

fn play_sound_linux() {
    let Ok(sound) = sound_file() else { return };
    for (player, args) in [
        ("paplay", &[][..]),
        ("pw-play", &[]),
        ("aplay", &["-q"]),
        ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet"]),
    ] {
        if have(player) {
            let _ = Cmd::new(player).args(args).arg(&sound).ok();
            return;
        }
    }
    eprintln!("rc-notify: no audio player found (install paplay/pw-play/aplay/ffplay)");
}

fn show_notification_linux(n: &Notification) -> Result<()> {
    if !have("notify-send") {
        anyhow::bail!("notify-send not installed (try `apt install libnotify-bin`)");
    }
    // Collapse a follow-up onto the previous bubble via --replace-id.
    let id_file = runtime_dir().join("claude-code-notify.id");
    let prev_id = std::fs::read_to_string(&id_file)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0);
    let new_id = Cmd::new("notify-send")
        .args(["--print-id", "--replace-id", &prev_id.to_string()])
        .args(["--urgency=critical", "--expire-time=0"])
        .args(["--app-name=claude-code", "--icon=dialog-information"])
        .args([n.title().as_str(), n.event.body()])
        .stdout()?;
    if let Ok(id) = new_id.trim().parse::<u32>() {
        let _ = std::fs::write(&id_file, id.to_string());
    }
    Ok(())
}

/// Materialize the embedded sound to the state dir (once) and return its path.
fn sound_file() -> Result<PathBuf> {
    let path = paths::state_dir()?.join("notification_sound.wav");
    if !path.is_file() {
        std::fs::write(&path, SOUND_WAV).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(path)
}

fn runtime_dir() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
}

fn have(cmd: &str) -> bool {
    Cmd::new("sh")
        .args(["-c", &format!("command -v {cmd} >/dev/null 2>&1")])
        .ok()
}

fn uid() -> String {
    Cmd::new("id")
        .arg("-u")
        .stdout()
        .unwrap_or_else(|_| "1000".to_owned())
}

#[cfg(unix)]
fn is_fifo(path: &std::path::Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::metadata(path)
        .map(|m| m.file_type().is_fifo())
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_fifo(_path: &std::path::Path) -> bool {
    false
}
