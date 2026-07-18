//! Minimal `~/.ssh/config` reader: the concrete `Host` aliases the user can
//! `ssh` to, for the `send-files` host picker.
//!
//! We list only literal host aliases — pattern entries (`Host *`, globs `?`/`*`,
//! negations `!foo`) aren't real destinations, so they're dropped. `Include`
//! directives and `Match` blocks are out of scope for v1 (a plain top-level
//! `Host` list covers the common case); documented so it isn't mistaken for a
//! bug.

/// Extract the literal `Host` aliases from ssh-config text, in first-seen order,
/// de-duplicated. Pattern/negated tokens are skipped.
pub fn host_aliases(config_text: &str) -> Vec<String> {
    let mut seen = Vec::new();
    for line in config_text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut tokens = line.split_whitespace();
        // The keyword is case-insensitive; we only care about `Host` lines.
        match tokens.next() {
            Some(kw) if kw.eq_ignore_ascii_case("host") => {}
            _ => continue,
        }
        for tok in tokens {
            if is_literal_alias(tok) && !seen.iter().any(|h| h == tok) {
                seen.push(tok.to_owned());
            }
        }
    }
    seen
}

/// A usable destination alias: no glob (`*`/`?`) and not a negation (`!…`).
fn is_literal_alias(tok: &str) -> bool {
    !tok.starts_with('!') && !tok.contains('*') && !tok.contains('?')
}

/// Read `~/.ssh/config` and return its literal `Host` aliases. A missing or
/// unreadable file yields an empty list (the picker just offers "this machine").
pub fn read_host_aliases() -> Vec<String> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    match std::fs::read_to_string(home.join(".ssh/config")) {
        Ok(text) => host_aliases(&text),
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
}
