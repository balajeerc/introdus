//! Pure egress-allowlist logic, kept out of the orchestration so it can be
//! tested against the exact behaviour of the old shell.
//!
//! The container's tinyproxy filter is a list of anchored, case-insensitive
//! extended-regex patterns generated from `WHITELIST_HOSTS` (plus the git host
//! and, when the webapp tunnel is on, `api.trycloudflare.com`). The escaping and
//! anchoring here reproduce `launch_dev_container.sh` byte-for-meaning:
//!
//! ```text
//! esc=$(printf '%s' "$h" | sed 's/[.[\*^$()+?{|]/\\&/g')
//! printf '(^|\\.)%s$\n' "$esc"
//! ```

/// The tunnel API hostname added to the proxy allowlist when `EXPOSE_WEBAPP`.
pub const TUNNEL_API_HOST: &str = "api.trycloudflare.com";

/// Cloudflare argotunnel edge IPs, allowed directly by IP on 7844 in the nft
/// filter (cloudflared's edge protocol can't go through the HTTP proxy). Pinned
/// because the restricted in-container DNS can't do cloudflared's SRV discovery.
pub const TUNNEL_EDGE_IPS: &[&str] = &[
    "198.41.192.167",
    "198.41.192.227",
    "198.41.200.13",
    "198.41.200.193",
];

/// Extract the bare host from a repo URL (`git@github.com:o/r.git` ->
/// `github.com`), matching `sed 's#^(git@|ssh://git@|https://)##; s#[:/].*$##'`.
pub fn git_host(repo_url: &str) -> String {
    let s = repo_url
        .strip_prefix("git@")
        .or_else(|| repo_url.strip_prefix("ssh://git@"))
        .or_else(|| repo_url.strip_prefix("https://"))
        .unwrap_or(repo_url);
    s.split([':', '/']).next().unwrap_or(s).to_owned()
}

/// Escape a hostname for the extended-regex allowlist, matching the shell's
/// `sed 's/[.[\*^$()+?{|]/\\&/g'` — note `]`, `}`, `-` are intentionally not
/// escaped.
fn escape_host(host: &str) -> String {
    const SPECIAL: &[char] = &['.', '[', '\\', '*', '^', '$', '(', ')', '+', '?', '{', '|'];
    let mut out = String::with_capacity(host.len() + 4);
    for c in host.chars() {
        if SPECIAL.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The anchored allowlist pattern for one host: `(^|\.)<escaped>$`, so
/// `github.com` matches `github.com` and `api.github.com` but not
/// `notgithub.com`.
pub fn allowlist_pattern(host: &str) -> String {
    format!("(^|\\.){}$", escape_host(host))
}

/// The full ordered host list the container enforces: the git host, then
/// `WHITELIST_HOSTS`, then any tunnel hosts — matching the shell's
/// `CONTAINER_WHITELIST_HOSTS="$GIT_HOST $WHITELIST_HOSTS $TUNNEL_HOSTS"`.
pub fn container_whitelist(
    repo_url: &str,
    whitelist: &[String],
    expose_webapp: bool,
) -> Vec<String> {
    let mut hosts = vec![git_host(repo_url)];
    hosts.extend(whitelist.iter().cloned());
    if expose_webapp {
        hosts.push(TUNNEL_API_HOST.to_owned());
    }
    hosts
}

/// Render the tinyproxy allowlist file body (one pattern per line) from a host
/// list.
pub fn render_allowlist(hosts: &[String]) -> String {
    let mut out = String::new();
    for h in hosts {
        out.push_str(&allowlist_pattern(h));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_host_forms() {
        assert_eq!(git_host("git@github.com:org/repo.git"), "github.com");
        assert_eq!(
            git_host("ssh://git@gitlab.example.com:22/o/r.git"),
            "gitlab.example.com"
        );
        assert_eq!(git_host("https://github.com/o/r.git"), "github.com");
        assert_eq!(git_host("github.com/o/r"), "github.com");
    }

    #[test]
    fn pattern_matches_shell_escaping() {
        assert_eq!(allowlist_pattern("github.com"), "(^|\\.)github\\.com$");
        // '-' and ']' are not escaped; '.' is.
        assert_eq!(allowlist_pattern("a-b.co"), "(^|\\.)a-b\\.co$");
    }

    #[test]
    fn container_whitelist_order_and_tunnel() {
        let wl = vec!["registry.npmjs.org".to_owned()];
        let hosts = container_whitelist("git@github.com:o/r.git", &wl, false);
        assert_eq!(hosts, vec!["github.com", "registry.npmjs.org"]);

        let hosts = container_whitelist("git@github.com:o/r.git", &wl, true);
        assert_eq!(hosts.last().unwrap(), TUNNEL_API_HOST);
    }

    #[test]
    fn render_is_one_pattern_per_line() {
        let hosts = vec!["github.com".to_owned(), "pypi.org".to_owned()];
        assert_eq!(
            render_allowlist(&hosts),
            "(^|\\.)github\\.com$\n(^|\\.)pypi\\.org$\n"
        );
    }
}
