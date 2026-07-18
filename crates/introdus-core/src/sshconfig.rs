//! Minimal `~/.ssh/config` reader: the concrete, container-capable `Host`
//! aliases the user can `ssh` to, for the `send-files` host picker.
//!
//! We list only literal host aliases — pattern entries (`Host *`, globs `?`/`*`,
//! negations `!foo`) aren't real destinations, so they're dropped. We also drop
//! **git-forge remotes** (a `Host` whose `User` is `git`, or whose `HostName`/
//! alias is a known forge like `github.com`): those are code-push endpoints, not
//! machines you can run a container on, so listing them in the picker is noise.
//!
//! `Include` directives and `Match` blocks are out of scope for v1 (a plain
//! top-level `Host` list covers the common case); documented so it isn't
//! mistaken for a bug.

/// A parsed `~/.ssh/config` host entry: its literal alias plus the `HostName`
/// and `User` that apply to it (best-effort — first value seen in its block).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshHost {
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
}

/// Known git-forge SSH endpoints — hosts you push code to, not run containers
/// on. Matched against a host's effective `HostName` (falling back to its alias),
/// exactly or as a subdomain.
const GIT_FORGES: &[&str] = &[
    "github.com",
    "gitlab.com",
    "bitbucket.org",
    "codeberg.org",
    "git.sr.ht",
    "gitea.com",
    "ssh.dev.azure.com",
    "vs-ssh.visualstudio.com",
];

/// Parse the literal `Host` entries from ssh-config text, in first-seen order,
/// de-duplicated by alias, each carrying the `HostName`/`User` from its block.
/// Pattern/negated host tokens are skipped; `Match` blocks and unknown keywords
/// don't attach to a host.
pub fn parse_hosts(config_text: &str) -> Vec<SshHost> {
    let mut hosts: Vec<SshHost> = Vec::new();
    // Indices into `hosts` for the aliases the current `Host` block configures.
    let mut current: Vec<usize> = Vec::new();
    for line in config_text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut tokens = line.split_whitespace();
        let Some(keyword) = tokens.next() else {
            continue;
        };
        if keyword.eq_ignore_ascii_case("host") {
            current.clear();
            for tok in tokens {
                if !is_literal_alias(tok) {
                    continue;
                }
                match hosts.iter().position(|h| h.alias == tok) {
                    Some(i) => current.push(i),
                    None => {
                        hosts.push(SshHost {
                            alias: tok.to_owned(),
                            hostname: None,
                            user: None,
                        });
                        current.push(hosts.len() - 1);
                    }
                }
            }
        } else if keyword.eq_ignore_ascii_case("match") {
            // Options under a `Match` block don't attach to a `Host` alias.
            current.clear();
        } else if keyword.eq_ignore_ascii_case("hostname") {
            set_field(&mut hosts, &current, tokens.next(), |h| &mut h.hostname);
        } else if keyword.eq_ignore_ascii_case("user") {
            set_field(&mut hosts, &current, tokens.next(), |h| &mut h.user);
        }
    }
    hosts
}

/// Set an as-yet-unset field on every host in the active block (ssh's
/// first-value-wins, applied per alias).
fn set_field(
    hosts: &mut [SshHost],
    current: &[usize],
    value: Option<&str>,
    field: impl Fn(&mut SshHost) -> &mut Option<String>,
) {
    let Some(value) = value else {
        return;
    };
    for &i in current {
        let slot = field(&mut hosts[i]);
        if slot.is_none() {
            *slot = Some(value.to_owned());
        }
    }
}

/// A usable destination alias: no glob (`*`/`?`) and not a negation (`!…`).
fn is_literal_alias(tok: &str) -> bool {
    !tok.starts_with('!') && !tok.contains('*') && !tok.contains('?')
}

/// Whether `host` is a git-forge remote (push code, no containers): `User git`,
/// or an effective host that is (a subdomain of) a known forge.
pub fn is_git_forge(host: &SshHost) -> bool {
    if host.user.as_deref() == Some("git") {
        return true;
    }
    let effective = host
        .hostname
        .as_deref()
        .unwrap_or(&host.alias)
        .to_lowercase();
    GIT_FORGES
        .iter()
        .any(|forge| effective == *forge || effective.ends_with(&format!(".{forge}")))
}

/// The literal `Host` aliases (ignoring `HostName`/`User`), in order. Used where
/// only the names matter; the picker uses [`container_host_aliases`], which also
/// filters out git-forge remotes.
pub fn host_aliases(config_text: &str) -> Vec<String> {
    parse_hosts(config_text)
        .into_iter()
        .map(|h| h.alias)
        .collect()
}

/// The container-capable host aliases from ssh-config text: literal `Host`
/// entries minus git-forge remotes (`User git` / a forge `HostName`).
pub fn container_host_aliases(config_text: &str) -> Vec<String> {
    parse_hosts(config_text)
        .into_iter()
        .filter(|h| !is_git_forge(h))
        .map(|h| h.alias)
        .collect()
}

/// Read `~/.ssh/config` and return its container-capable `Host` aliases (git
/// forges excluded). A missing or unreadable file yields an empty list (the
/// picker just offers "this machine").
pub fn read_host_aliases() -> Vec<String> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    match std::fs::read_to_string(home.join(".ssh/config")) {
        Ok(text) => container_host_aliases(&text),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta145_lists_literal_hosts_in_order() {
        let cfg = "\
# my hosts
Host devbox
    HostName 10.0.0.5
    User me

Host laptop workstation
    HostName 10.0.0.6
";
        assert_eq!(host_aliases(cfg), vec!["devbox", "laptop", "workstation"]);
    }

    #[test]
    fn ta145_drops_wildcards_negations_and_dupes() {
        let cfg = "\
Host *
    ForwardAgent yes
Host prod !prod-bastion prod?
    User deploy
Host prod
    Port 2222
";
        // `*`, `!prod-bastion`, and `prod?` are dropped; `prod` appears once.
        assert_eq!(host_aliases(cfg), vec!["prod"]);
    }

    #[test]
    fn ta145_case_insensitive_keyword_and_ignores_other_directives() {
        let cfg = "\
Match host anything
    User x
HOST alpha
Include ~/.ssh/other
host beta
";
        assert_eq!(host_aliases(cfg), vec!["alpha", "beta"]);
    }

    #[test]
    fn ta145_empty_or_blank_config_is_empty() {
        assert!(host_aliases("").is_empty());
        assert!(host_aliases("\n  \n# just a comment\n").is_empty());
    }

    #[test]
    fn ta150_git_forge_hosts_are_excluded_from_the_picker() {
        // The exact shape the picker was over-listing: a forge by alias, a forge
        // aliased to a different name via HostName, and a `User git` box — all
        // dropped; only the real container hosts remain.
        let cfg = "\
Host github.com
    User git
Host github2
    HostName github.com
    User git
Host gl
    HostName gitlab.com
Host oracle
    HostName 10.0.0.9
    User ubuntu
Host eejalab
    HostName 192.168.1.20
";
        // Raw parse still sees every literal alias…
        assert_eq!(
            host_aliases(cfg),
            vec!["github.com", "github2", "gl", "oracle", "eejalab"]
        );
        // …but the picker list drops the three git-forge remotes.
        assert_eq!(container_host_aliases(cfg), vec!["oracle", "eejalab"]);
    }

    #[test]
    fn ta150_is_git_forge_signals() {
        let user_git = SshHost {
            alias: "gh".into(),
            hostname: None,
            user: Some("git".into()),
        };
        let forge_hostname = SshHost {
            alias: "work".into(),
            hostname: Some("GitHub.com".into()), // case-insensitive
            user: None,
        };
        let forge_subdomain = SshHost {
            alias: "az".into(),
            hostname: Some("ssh.dev.azure.com".into()),
            user: None,
        };
        let real = SshHost {
            alias: "box".into(),
            hostname: Some("10.0.0.1".into()),
            user: Some("me".into()),
        };
        assert!(is_git_forge(&user_git));
        assert!(is_git_forge(&forge_hostname));
        assert!(is_git_forge(&forge_subdomain));
        assert!(!is_git_forge(&real));
    }
}
