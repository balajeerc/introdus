//! Small shared helpers used across the CLI (path and shell-quoting).

use std::path::{Path, PathBuf};

/// Expand a leading `~/` to the user's home directory; otherwise return the
/// path unchanged.
pub fn expand_tilde(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(raw)
}

/// The `<name>.pub` path beside a private key — appends `.pub` (never replaces
/// an existing extension), matching ssh-keygen's naming.
pub fn pub_sibling(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".pub");
    path.with_file_name(name)
}

/// Single-quote a string for safe embedding in a `sh -c` command (as tmux runs
/// window commands).
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_escapes() {
        assert_eq!(shell_quote("/opt/introdus"), "'/opt/introdus'");
        assert_eq!(shell_quote("a'b"), r"'a'\''b'");
    }

    #[test]
    fn expand_tilde_resolves_home() {
        if let Some(home) = dirs::home_dir() {
            assert_eq!(expand_tilde("~/x/y"), home.join("x/y"));
        }
        assert_eq!(expand_tilde("/abs/p"), PathBuf::from("/abs/p"));
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
