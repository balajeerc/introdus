//! Small shared helpers used across the CLI (path and shell-quoting).

use std::path::PathBuf;

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
}
