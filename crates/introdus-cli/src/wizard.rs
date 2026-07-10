//! The setup wizard — the interactive replacement for `create-dev-container.sh`.
//! Walks the user through the required `.env` fields, the agent checklist, and
//! deploy-key setup, then writes the project's `.env`.

use std::path::Path;

use anyhow::{Context, Result};
use inquire::{Confirm, CustomType, MultiSelect, Text};
use introdus_core::agents::{self, Agent};
use introdus_core::process::Cmd;
use introdus_core::Config;

use crate::util::expand_tilde;

/// Run the wizard for a project rooted at `project_dir`, writing `.env` and
/// returning the resulting config.
pub fn run(project_dir: &Path) -> Result<Config> {
    println!("\n=== introdus setup ===");
    println!(
        "Configuring a network-hardened dev container for {}\n",
        project_dir.display()
    );

    let default_name = project_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "my-project".to_owned());
    let project_name = ask_default("Project name", &default_name)?;
    let repo_url = ask_nonempty("Git repo URL (git@github.com:org/repo.git)")?;
    let deploy_key_path = prompt_deploy_key(&repo_url)?;
    let webapp_port = CustomType::<u16>::new("Webapp port (bound in the container):")
        .with_default(3000)
        .prompt()?;

    let install_agents = prompt_agents()?;
    let expose_webapp = Confirm::new("Expose the webapp to the internet via a Cloudflare tunnel?")
        .with_default(false)
        .prompt()?;
    let (enable_notify_sh_alerts, ntfy_sh_topic) = prompt_ntfy()?;

    let mut config = Config::new(project_name, repo_url, deploy_key_path, webapp_port);
    apply_agents(&mut config, install_agents);
    config.expose_webapp = expose_webapp;
    config.enable_notify_sh_alerts = enable_notify_sh_alerts;
    config.ntfy_sh_topic = ntfy_sh_topic;

    let env = project_dir.join(".env");
    config
        .save(&env)
        .with_context(|| format!("writing {}", env.display()))?;
    println!("\n==> wrote {}", env.display());
    Ok(config)
}

/// Set the selected agents and extend the whitelist with their egress hosts.
fn apply_agents(config: &mut Config, selected: Vec<String>) {
    config.install_agents = selected;
    for id in &config.install_agents {
        if let Some(agent) = agents::find(id) {
            for host in agent.host_list() {
                let host = host.to_owned();
                if !config.whitelist_hosts.contains(&host) {
                    config.whitelist_hosts.push(host);
                }
            }
        }
    }
}

/// A selectable agent row (shows its label, carries its id).
struct AgentChoice {
    id: &'static str,
    label: &'static str,
    method_note: &'static str,
}

impl std::fmt::Display for AgentChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.label, self.method_note)
    }
}

fn prompt_agents() -> Result<Vec<String>> {
    let options: Vec<AgentChoice> = agents::AGENTS.iter().map(choice).collect();
    let claude_idx = agents::AGENTS
        .iter()
        .position(|a| a.id == "claude")
        .unwrap_or(0);
    let picked = MultiSelect::new(
        "Coding agents to install (space toggles, enter confirms):",
        options,
    )
    .with_default(&[claude_idx])
    .prompt()?;
    let mut ids: Vec<String> = picked.into_iter().map(|c| c.id.to_owned()).collect();
    // Claude is baked into the image; keep it selected regardless.
    if !ids.iter().any(|id| id == "claude") {
        ids.insert(0, "claude".to_owned());
    }
    Ok(ids)
}

fn choice(a: &'static Agent) -> AgentChoice {
    let method_note = match a.method {
        agents::InstallMethod::Script => "  [vendor installer — runs remote code]",
        agents::InstallMethod::Pnpm => "",
    };
    AgentChoice {
        id: a.id,
        label: a.label,
        method_note,
    }
}

fn prompt_ntfy() -> Result<(bool, Option<String>)> {
    let enable = Confirm::new("Enable mobile push notifications via ntfy.sh?")
        .with_default(false)
        .prompt()?;
    if !enable {
        return Ok((false, None));
    }
    let topic = ask_nonempty("ntfy.sh topic (treat like a password)")?;
    Ok((true, Some(topic)))
}

/// Prompt for the deploy key path, offering to generate one if it's missing.
fn prompt_deploy_key(repo_url: &str) -> Result<String> {
    loop {
        let raw = ask_nonempty("Path to the repo deploy key (private key)")?;
        let path = expand_tilde(&raw);
        if path.is_file() {
            return Ok(path.to_string_lossy().into_owned());
        }
        let generate = Confirm::new(&format!(
            "{} does not exist. Generate an ed25519 key there?",
            path.display()
        ))
        .with_default(true)
        .prompt()?;
        if generate {
            generate_deploy_key(&path, repo_url)?;
            return Ok(path.to_string_lossy().into_owned());
        }
    }
}

/// `ssh-keygen` a passphrase-less ed25519 key, print the public half, and wait
/// for the user to register it with the git host.
fn generate_deploy_key(path: &Path, repo_url: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    Cmd::new("ssh-keygen")
        .args(["-t", "ed25519", "-N", "", "-C", "introdus-deploy-key", "-f"])
        .arg(path)
        .run()
        .context("ssh-keygen failed")?;
    let pubkey = std::fs::read_to_string(path.with_extension("pub"))
        .context("reading generated public key")?;
    println!("\n  Add this PUBLIC deploy key to {repo_url} (with write access):\n");
    println!("    {}", pubkey.trim());
    Confirm::new("Press enter once the deploy key is registered with the repo")
        .with_default(true)
        .prompt()?;
    Ok(())
}

fn ask_nonempty(prompt: &str) -> Result<String> {
    loop {
        let value = Text::new(prompt).prompt()?;
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_owned());
        }
        println!("  (required)");
    }
}

fn ask_default(prompt: &str, default: &str) -> Result<String> {
    let value = Text::new(prompt).with_default(default).prompt()?;
    Ok(value.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_agents_extends_whitelist() {
        let mut c = Config::new(
            "p".to_owned(),
            "git@github.com:o/r.git".to_owned(),
            "/k".to_owned(),
            3000,
        );
        let before = c.whitelist_hosts.len();
        apply_agents(&mut c, vec!["claude".to_owned(), "codex".to_owned()]);
        assert_eq!(c.install_agents, vec!["claude", "codex"]);
        // codex's hosts (api.openai.com, ...) were appended.
        assert!(c.whitelist_hosts.contains(&"api.openai.com".to_owned()));
        assert!(c.whitelist_hosts.len() > before);
    }
}
