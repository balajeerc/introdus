//! Low-level `.env` I/O. Reading goes through `dotenvy` (which handles the
//! bash-style quoting and multi-line double-quoted values the harness uses, so
//! it matches what `source .env` produced for the old `launch.sh`). Writing is
//! done by [`crate::config::Config::render`]; the helpers here quote values so
//! a round-trip is faithful.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};

/// Read a `.env` file into a key→value map. Values are fully unquoted and
/// multi-line values are joined with `\n`, exactly as bash would expand them.
pub fn read_map(path: &Path) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    for item in dotenvy::from_path_iter(path)
        .with_context(|| format!("reading env file {}", path.display()))?
    {
        let (k, v) = item.with_context(|| format!("parsing env file {}", path.display()))?;
        map.insert(k, v);
    }
    Ok(map)
}

/// Split a whitespace/newline-separated list value (e.g. `WHITELIST_HOSTS`,
/// `INSTALL_AGENTS`, `INTERNAL_ALLOW_CIDRS`) into its entries, dropping blanks.
pub fn split_list(value: &str) -> Vec<String> {
    value.split_whitespace().map(str::to_owned).collect()
}

/// Quote a scalar value for the generated `.env`. Bare (unquoted) when it is a
/// simple token; double-quoted (with `"`, `\`, `$`, and backtick escaped)
/// otherwise, so bash sourcing and `dotenvy` both read back the exact string.
pub fn quote_scalar(value: &str) -> String {
    let simple = !value.is_empty()
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '@' | ',')
        });
    if simple {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        if matches!(c, '"' | '\\' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta09_split_list_drops_blanks_and_newlines() {
        assert_eq!(split_list("\n a \n b\nc \n"), vec!["a", "b", "c"]);
        assert_eq!(split_list(""), Vec::<String>::new());
    }

    #[test]
    fn ta10_quote_scalar_bare_vs_quoted() {
        assert_eq!(
            quote_scalar("git@github.com:o/r.git"),
            "git@github.com:o/r.git"
        );
        assert_eq!(quote_scalar("8g"), "8g");
        assert_eq!(quote_scalar("has space"), "\"has space\"");
        assert_eq!(quote_scalar("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote_scalar("$X`y`"), "\"\\$X\\`y\\`\"");
    }
}
