//! The setup wizard — the interactive replacement for `create-dev-container.sh`.
//! Walks the user through the required `.env` fields, the agent checklist, and
//! deploy-key setup, then writes the project's `.env`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use inquire::{Confirm, CustomType, MultiSelect, Select, Text};
use introdus_core::agents::{self, Agent};
use introdus_core::process::Cmd;
use introdus_core::{names, Config};

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
    let deploy_key_path = prompt_deploy_key(&project_name, &repo_url)?;
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

/// Deploy-key setup: ask up-front whether to generate a fresh key or point at
/// an existing one, branch to resolve the private-key path, then — in BOTH
/// cases — print the public half and wait for the user to register it as a
/// write-access deploy key before the clone step relies on it.
fn prompt_deploy_key(project_name: &str, repo_url: &str) -> Result<String> {
    let generate = Confirm::new("Generate a new per-project deploy key now?")
        .with_default(true)
        .with_help_message("No = point introdus at a deploy key you already have")
        .prompt()?;
    let path = if generate {
        generate_new_deploy_key(project_name)?
    } else {
        prompt_existing_deploy_key(project_name)?
    };
    announce_deploy_key(Path::new(&path), repo_url)?;
    Ok(path)
}

/// Create a fresh ed25519 key at a project-derived default location (which the
/// user can override), refusing to overwrite an existing file. Returns the
/// private-key path; registration is handled by the caller.
fn generate_new_deploy_key(project_name: &str) -> Result<String> {
    let slug = names::image_slug(project_name);
    let filename = format!("{slug}-deploy-key");
    // Group introdus-created keys under their own subdir so they don't clutter
    // ~/.ssh alongside personal keys.
    let default = dirs::home_dir()
        .map(|h| h.join(".ssh/introdus-deploy-keys").join(&filename))
        .unwrap_or_else(|| PathBuf::from(&filename));
    loop {
        let raw = Text::new("Where should the new deploy key be created?")
            .with_default(&default.to_string_lossy())
            .with_help_message(
                "A private key is written here; its .pub is printed next to register",
            )
            .prompt()?;
        let path = expand_tilde(raw.trim());
        if path.exists() {
            println!(
                "  a file already exists at {} — pick another path to avoid overwriting it.",
                path.display()
            );
            continue;
        }
        create_key_file(&path)?;
        return Ok(path.to_string_lossy().into_owned());
    }
}

/// Point introdus at an already-existing private deploy key. If any keys in the
/// user's ssh dirs resemble this project, offer to reuse one (a plain yes/no,
/// then a picker only when several match); otherwise, or on decline, prompt for
/// a path.
fn prompt_existing_deploy_key(project_name: &str) -> Result<String> {
    if let Some(path) = offer_candidate_keys(&find_candidate_keys(project_name))? {
        return Ok(path);
    }
    prompt_key_path()
}

/// Two-step reuse flow for project-matching keys: confirm intent first, then —
/// only if several matched — pick which. `Ok(None)` means "none / not these",
/// so the caller falls through to a manual path prompt.
fn offer_candidate_keys(matches: &[PathBuf]) -> Result<Option<String>> {
    let (first, rest) = match matches.split_first() {
        Some(pair) => pair,
        None => return Ok(None),
    };
    let question = if rest.is_empty() {
        format!("Reuse the existing key at {}?", first.display())
    } else {
        format!(
            "Reuse one of the {} existing keys matching this project?",
            matches.len()
        )
    };
    let reuse = Confirm::new(&question).with_default(true).prompt()?;
    if !reuse {
        return Ok(None);
    }
    if rest.is_empty() {
        return Ok(Some(first.to_string_lossy().into_owned()));
    }
    let options: Vec<String> = matches
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let choice = Select::new("Which key?", options).prompt()?;
    Ok(Some(choice))
}

/// Prompt for the path to a private deploy key, re-asking until it points at a
/// real file.
fn prompt_key_path() -> Result<String> {
    loop {
        let raw = ask_nonempty("Path to your existing deploy key (the private key file)")?;
        let path = expand_tilde(&raw);
        if path.is_file() {
            return Ok(path.to_string_lossy().into_owned());
        }
        println!(
            "  no file at {} — enter the path to your existing private key.",
            path.display()
        );
    }
}

/// Scan the user's ssh dirs for private keys whose filename resembles the
/// project, best-match-first. A file is treated as a private key when it has a
/// sibling `.pub` — which skips `config`, `known_hosts`, and public keys.
fn find_candidate_keys(project_name: &str) -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let tokens = key_match_tokens(project_name);
    if tokens.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(usize, PathBuf)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in [home.join(".ssh"), home.join(".ssh/introdus-deploy-keys")] {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_private_key(&path) {
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let score = tokens.iter().filter(|t| name.contains(*t)).count();
            if score > 0 && seen.insert(path.clone()) {
                scored.push((score, path));
            }
        }
    }
    // Highest score first, then alphabetical for a stable, predictable order.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, p)| p).collect()
}

/// The lowercase tokens (each ≥3 chars) a key filename must contain to be
/// considered a match — derived from the project's image slug.
fn key_match_tokens(project_name: &str) -> Vec<String> {
    let slug = names::image_slug(project_name);
    let mut tokens: Vec<String> = slug
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| t.len() >= 3)
        .map(str::to_owned)
        .collect();
    if slug.len() >= 3 {
        tokens.push(slug);
    }
    tokens.sort();
    tokens.dedup();
    tokens
}

/// True when `path` is a private key file: a regular file with no `.pub`
/// extension that has a sibling `<name>.pub`.
fn is_private_key(path: &Path) -> bool {
    if !path.is_file() || path.extension().is_some_and(|e| e == "pub") {
        return false;
    }
    pub_sibling(path).is_file()
}

/// The `<name>.pub` path beside a private key (appends, never replaces — so
/// `my.key` maps to `my.key.pub`, matching ssh-keygen).
fn pub_sibling(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".pub");
    path.with_file_name(name)
}

/// `ssh-keygen` a fresh passphrase-less ed25519 key at `path`.
fn create_key_file(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
        // Lock down our own key directory (holds private keys). Guarded by name
        // so a user-chosen custom path's parent is never re-permissioned.
        if parent
            .file_name()
            .is_some_and(|n| n == "introdus-deploy-keys")
        {
            restrict_dir(parent);
        }
    }
    // Capture (and discard) ssh-keygen's stdout — the "key pair generated",
    // fingerprint, and randomart art are noise here; stderr stays visible for
    // real errors. Mirrors the old wizard's `ssh-keygen … >/dev/null`.
    Cmd::new("ssh-keygen")
        .args(["-t", "ed25519", "-N", "", "-C", "introdus-deploy-key", "-f"])
        .arg(path)
        .stdout()
        .context("ssh-keygen failed")?;
    Ok(())
}

/// Print the public half of the deploy key and wait for the user to register it
/// with the git host. Run for both freshly-generated and reused keys, so the
/// clone step never proceeds against an unregistered key.
fn announce_deploy_key(path: &Path, repo_url: &str) -> Result<()> {
    let pubkey = read_public_key(path)?;
    println!("\n  Deploy key: {}", path.display());
    println!("  Add this PUBLIC key to {repo_url} as a deploy key WITH WRITE ACCESS:\n");
    println!("    {}", pubkey.trim());
    Confirm::new("Press enter once the deploy key is registered with the repo")
        .with_default(true)
        .prompt()?;
    Ok(())
}

/// The public key for a private key `path`: read the sibling `.pub` if present,
/// else derive it from the private key with `ssh-keygen -y`.
fn read_public_key(path: &Path) -> Result<String> {
    let pubfile = pub_sibling(path);
    if pubfile.is_file() {
        return std::fs::read_to_string(&pubfile)
            .with_context(|| format!("reading {}", pubfile.display()));
    }
    Cmd::new("ssh-keygen")
        .args(["-y", "-f"])
        .arg(path)
        .stdout()
        .context("deriving public key with `ssh-keygen -y`")
}

/// Best-effort `chmod 700` on our key directory (private keys live there).
#[cfg(unix)]
fn restrict_dir(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn restrict_dir(_dir: &Path) {}

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

    #[test]
    fn key_match_tokens_splits_slug() {
        let tokens = key_match_tokens("Ship TBC");
        // "ship tbc" -> slug "ship-tbc": tokens "ship", "tbc", plus the slug.
        assert!(tokens.contains(&"ship".to_owned()));
        assert!(tokens.contains(&"tbc".to_owned()));
        assert!(tokens.contains(&"ship-tbc".to_owned()));
        // Too-short to yield any ≥3-char token or slug.
        assert!(key_match_tokens("x").is_empty());
    }

    #[test]
    fn pub_sibling_appends_not_replaces() {
        assert_eq!(
            pub_sibling(Path::new("/k/id_ed25519")),
            Path::new("/k/id_ed25519.pub")
        );
        assert_eq!(
            pub_sibling(Path::new("/k/my.key")),
            Path::new("/k/my.key.pub")
        );
    }
}
