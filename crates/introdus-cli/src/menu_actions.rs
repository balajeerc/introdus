//! Implementations of the control-menu utilities. Each takes the resolved
//! [`LaunchContext`] and performs one action, printing its own progress. These
//! run on the host, so they can edit `.env`, drive podman, spawn tmux windows,
//! and open a root shell — the operations an in-container TUI can't do.

use std::io::Write;

use anyhow::{bail, Context, Result};
use inquire::{Confirm, MultiSelect, Select, Text};
use introdus_core::podman::{self, exec_interactive, podman};
use introdus_core::{agents, session as session_names, tmux, Config};

use crate::context::{env_path, LaunchContext};
use crate::util::{expand_tilde, shell_quote};

/// Wait for the user before redrawing the menu.
pub fn pause() {
    print!("\n  (press Enter to continue) ");
    let _ = std::io::stdout().flush();
    let mut buf = String::new();
    let _ = std::io::stdin().read_line(&mut buf);
}

// ---- read-only / runtime utilities -----------------------------------------

/// Show the cached cloudflared quick-tunnel URL from inside the container.
pub fn tunnel_url(ctx: &LaunchContext) -> Result<()> {
    require_running(ctx)?;
    let _ = exec(ctx, Some("dev")).arg("tunnel-url").run();
    Ok(())
}

/// Show the hostnames the egress proxy recently blocked.
pub fn blocked_egress(ctx: &LaunchContext) -> Result<()> {
    require_running(ctx)?;
    let _ = exec(ctx, Some("dev")).arg("egress-log").run();
    Ok(())
}

/// Fire a test notification event from inside the container.
pub fn test_notify(ctx: &LaunchContext) -> Result<()> {
    require_running(ctx)?;
    println!("  sending a test 'done' notification via rc-notify…");
    let _ = exec(ctx, Some("dev")).args(["rc-notify", "done"]).run();
    println!("  (delivery needs the host notify listener — see `introdus notify-host`, M7)");
    Ok(())
}

// ---- terminals & agent windows ---------------------------------------------

/// Open a shell in a new tmux window: `root-bash` (root) or `dev-bash` (dev).
pub fn open_terminal(ctx: &LaunchContext, user: Option<&str>) -> Result<()> {
    require_running(ctx)?;
    let window = if user.is_some() {
        "dev-bash"
    } else {
        "root-bash"
    };
    let cmd = exec_interactive(&ctx.container_name, user)
        .arg("bash")
        .label()
        .to_owned();
    spawn_window(ctx, window, &cmd)
}

/// Launch an installed agent in its own tmux window (Claude via `run-claude`,
/// with remote control already on; others via their own command).
pub fn launch_agent(ctx: &LaunchContext) -> Result<()> {
    require_running(ctx)?;
    let installed = ctx.config.install_agents.clone();
    if installed.is_empty() {
        bail!("no agents configured to launch");
    }
    let id = Select::new("Launch which agent?", installed).prompt()?;
    let run_cmd = if id == "claude" {
        "run-claude".to_owned()
    } else {
        agents::find(&id)
            .map(|a| a.cmd.to_owned())
            .unwrap_or(id.clone())
    };
    let cmd = exec_interactive(&ctx.container_name, Some("dev"))
        .arg(&run_cmd)
        .label()
        .to_owned();
    spawn_window(ctx, &format!("agent-{id}"), &cmd)
}

// ---- egress allowlist -------------------------------------------------------

/// Append hostnames to `WHITELIST_HOSTS`, regenerate the allowlist file, and
/// offer to restart the container to apply it.
pub fn add_allowlist(ctx: &LaunchContext) -> Result<()> {
    let raw = Text::new("Hostnames to allow (space/comma separated):").prompt()?;
    let mut config = ctx.config.clone();
    let mut added = Vec::new();
    for host in raw
        .split([',', ' ', '\n', '\t'])
        .map(str::trim)
        .filter(|h| !h.is_empty())
    {
        let host = host.to_owned();
        if !config.whitelist_hosts.contains(&host) {
            config.whitelist_hosts.push(host.clone());
            added.push(host);
        }
    }
    if added.is_empty() {
        println!("  nothing new to add.");
        return Ok(());
    }
    save_and_regen_allowlist(
        ctx,
        config,
        &format!("added {} host(s): {}", added.len(), added.join(", ")),
    )
}

// ---- config toggles (need a recreate to apply) ------------------------------

/// Turn on `EXPOSE_WEBAPP` and offer to recreate.
pub fn toggle_expose_webapp(ctx: &LaunchContext) -> Result<()> {
    if ctx.config.expose_webapp {
        println!("  webapp is already exposed (EXPOSE_WEBAPP=true).");
        return Ok(());
    }
    if !Confirm::new("Expose the webapp to the internet via a Cloudflare tunnel?")
        .with_default(false)
        .prompt()?
    {
        return Ok(());
    }
    let mut config = ctx.config.clone();
    config.expose_webapp = true;
    save_config(ctx, &config)?;
    offer_recreate(ctx, "EXPOSE_WEBAPP=true")
}

/// Turn on ntfy.sh push (prompting for the topic) and offer to recreate.
pub fn enable_ntfy(ctx: &LaunchContext) -> Result<()> {
    let topic = Text::new("ntfy.sh topic (treat like a password):").prompt()?;
    if topic.trim().is_empty() {
        bail!("a topic is required");
    }
    let mut config = ctx.config.clone();
    config.enable_notify_sh_alerts = true;
    config.ntfy_sh_topic = Some(topic.trim().to_owned());
    save_config(ctx, &config)?;
    offer_recreate(ctx, "ENABLE_NOTIFY_SH_ALERTS=true")
}

/// Install one or more not-yet-selected agents into the running container.
pub fn install_agent(ctx: &LaunchContext) -> Result<()> {
    require_running(ctx)?;
    let candidates: Vec<String> = agents::AGENTS
        .iter()
        .filter(|a| !a.prebaked && !ctx.config.install_agents.iter().any(|id| id == a.id))
        .map(|a| a.id.to_owned())
        .collect();
    if candidates.is_empty() {
        println!("  all supported agents are already selected.");
        return Ok(());
    }
    let picked = MultiSelect::new("Install which agents?", candidates).prompt()?;
    if picked.is_empty() {
        return Ok(());
    }
    let mut config = ctx.config.clone();
    for id in &picked {
        if !config.install_agents.contains(id) {
            config.install_agents.push(id.clone());
        }
        if let Some(agent) = agents::find(id) {
            for h in agent.host_list() {
                let h = h.to_owned();
                if !config.whitelist_hosts.contains(&h) {
                    config.whitelist_hosts.push(h);
                }
            }
        }
    }
    save_and_regen_allowlist(
        ctx,
        config.clone(),
        &format!("selected: {}", picked.join(", ")),
    )?;
    println!("  running install-agents in the container…");
    exec(ctx, Some("dev"))
        .env("INSTALL_AGENTS", config.install_agents.join(" "))
        .arg("install-agents")
        .run()?;
    println!(
        "  note: new egress hosts apply after a restart — use Recreate if an install was blocked."
    );
    Ok(())
}

// ---- copy a host file into the container ------------------------------------

/// Copy a host file/folder into the container's `/home/dev/uploads`.
pub fn copy_file(ctx: &LaunchContext) -> Result<()> {
    require_running(ctx)?;
    let raw = Text::new("Host path to copy (file or folder):").prompt()?;
    let src = expand_tilde(raw.trim());
    if !src.exists() {
        bail!("no such path: {}", src.display());
    }
    let dest = format!("{}:/home/dev/uploads/", ctx.container_name);
    exec(ctx, Some("dev"))
        .args(["mkdir", "-p", "/home/dev/uploads"])
        .run()?;
    podman().arg("cp").arg(&src).arg(&dest).run()?;
    exec(ctx, None)
        .args(["chown", "-R", "dev:dev", "/home/dev/uploads"])
        .run()?;
    println!("  copied {} -> /home/dev/uploads/", src.display());
    Ok(())
}

// ---- container lifecycle from the menu --------------------------------------

/// Recreate the container (drop it, keep the volume) to apply frozen `.env`
/// changes, respawning the dev-container window.
pub fn recreate(ctx: &LaunchContext) -> Result<()> {
    if !Confirm::new("Recreate the container now? (keeps your /home/dev volume)")
        .with_default(true)
        .prompt()?
    {
        return Ok(());
    }
    podman::remove_container(&ctx.container_name)?;
    respawn_dev_window(ctx)
}

/// Reset the container AND wipe the volume, respawning the dev-container window.
pub fn reset(ctx: &LaunchContext) -> Result<()> {
    println!("  reset WIPES /home/dev (repo, uncommitted work, installed packages).");
    let confirm = Text::new("Type 'yes' to wipe the volume:").prompt()?;
    if confirm.trim() != "yes" {
        println!("  aborted.");
        return Ok(());
    }
    podman::remove_container(&ctx.container_name)?;
    podman::remove_volume(&ctx.volume_name)?;
    respawn_dev_window(ctx)
}

// ---- helpers ----------------------------------------------------------------

fn require_running(ctx: &LaunchContext) -> Result<()> {
    if podman::container_running(&ctx.container_name) {
        Ok(())
    } else {
        bail!("container {} is not running", ctx.container_name)
    }
}

fn exec(ctx: &LaunchContext, user: Option<&str>) -> introdus_core::process::Cmd {
    podman::exec(&ctx.container_name, user)
}

fn session_of(ctx: &LaunchContext) -> String {
    ctx.config
        .session_name
        .clone()
        .unwrap_or_else(|| session_names::generate(&ctx.config.project_name))
}

/// Open (and focus) a new tmux window running `cmd`.
fn spawn_window(ctx: &LaunchContext, window: &str, cmd: &str) -> Result<()> {
    let session = session_of(ctx);
    tmux::new_window(&session, window, cmd, true, &ctx.project_dir)?;
    println!("  opened window '{window}' (Ctrl-a then its number to return here)");
    Ok(())
}

/// Kill and re-open the dev-container window running `introdus up`.
fn respawn_dev_window(ctx: &LaunchContext) -> Result<()> {
    let session = session_of(ctx);
    let bin = std::env::current_exe().context("locating introdus binary")?;
    let cmd = format!("exec {} up", shell_quote(&bin.to_string_lossy()));
    tmux::kill_window(&session, "dev-container")?;
    tmux::new_window(&session, "dev-container", &cmd, true, &ctx.project_dir)?;
    println!("  dev-container window restarted — it will (re)create the container.");
    Ok(())
}

fn save_config(ctx: &LaunchContext, config: &Config) -> Result<()> {
    config.save(&env_path(&ctx.project_dir))?;
    println!("  saved .env");
    Ok(())
}

/// Save the config, then regenerate the bind-mounted allowlist file and offer a
/// restart so the running proxy picks it up.
fn save_and_regen_allowlist(ctx: &LaunchContext, config: Config, summary: &str) -> Result<()> {
    save_config(ctx, &config)?;
    let regen = LaunchContext::resolve(config, ctx.project_dir.clone())?;
    regen.write_allowlist()?;
    println!("  {summary}");
    if podman::container_running(&ctx.container_name)
        && Confirm::new("Restart the container to apply the new allowlist?")
            .with_default(false)
            .prompt()?
    {
        podman().args(["restart", &ctx.container_name]).run()?;
    }
    Ok(())
}

fn offer_recreate(ctx: &LaunchContext, changed: &str) -> Result<()> {
    println!(
        "  {changed} saved — it applies only after a container recreate (env is frozen at create)."
    );
    if Confirm::new("Recreate the container now?")
        .with_default(false)
        .prompt()?
    {
        podman::remove_container(&ctx.container_name)?;
        return respawn_dev_window(ctx);
    }
    Ok(())
}
