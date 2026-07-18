//! Parsing the podman output the `send-files` flow depends on, kept pure so it
//! is unit-testable without a live podman: the running-container list
//! (`podman ps`) and a container directory listing. Directory entries carry
//! modified/created timestamps so the browser can sort by them, and a fuzzy
//! matcher backs the current-folder filter.

use std::cmp::Ordering;

/// The `--format` template for [`parse_ps`]: tab-separated name / state / image.
pub const PS_FORMAT: &str = "{{.Names}}\t{{.State}}\t{{.Image}}";

/// The `find` `-printf` template [`parse_find`] parses: type, mtime epoch, birth
/// epoch, basename — tab-separated, one entry per line. `find` gives us the
/// timestamps a bare `ls` can't, so the browser can sort by modified/created.
pub const FIND_PRINTF: &str = "%y\\t%T@\\t%B@\\t%f\\n";

/// The `ls` flags [`parse_ls`] parses — the timestamp-less fallback when a
/// container has no `find`: one entry per line (`-1`), dotfiles but not `.`/`..`
/// (`-A`), `/` appended to directories (`-p`).
pub const LS_FLAGS: &str = "-1Ap";

/// A running introdus container, as surfaced by [`parse_ps`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    pub name: String,
    pub state: String,
    pub image: String,
}

/// The introdus container-name prefix; only these are ours to list.
const PREFIX: &str = "introdus-";

/// Parse `podman ps --format {PS_FORMAT}` output, keeping only introdus-managed
/// containers (name starts with `introdus-`). Malformed lines are skipped.
pub fn parse_ps(output: &str) -> Vec<Container> {
    output
        .lines()
        .filter_map(|line| {
            let mut cols = line.split('\t');
            let name = cols.next()?.trim();
            if !name.starts_with(PREFIX) {
                return None;
            }
            Some(Container {
                name: name.to_owned(),
                state: cols.next().unwrap_or("").trim().to_owned(),
                image: cols.next().unwrap_or("").trim().to_owned(),
            })
        })
        .collect()
}

/// One entry in a directory listing (either pane of the file browser).
/// `modified`/`created` are epoch seconds when known (a bare `ls`, or a
/// filesystem without birth time, leaves them `None`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub modified: Option<u64>,
    pub created: Option<u64>,
}

impl DirEntry {
    /// A plain entry with no timestamps (used by the `ls` fallback and tests).
    pub fn bare(name: impl Into<String>, is_dir: bool) -> Self {
        Self {
            name: name.into(),
            is_dir,
            modified: None,
            created: None,
        }
    }
}

/// Parse `ls {LS_FLAGS} <dir>` output into entries (no timestamps). A trailing
/// `/` (from `-p`) marks a directory and is stripped from the name.
pub fn parse_ls(output: &str) -> Vec<DirEntry> {
    output
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| match line.strip_suffix('/') {
            Some(dir) => DirEntry::bare(dir, true),
            None => DirEntry::bare(line, false),
        })
        .collect()
}

/// Parse `find <dir> -maxdepth 1 -mindepth 1 -printf {FIND_PRINTF}` output:
/// `type\tmtime\tbtime\tname`. Type `d` is a directory; a non-numeric or `0`
/// timestamp (a filesystem with no birth time) becomes `None`.
pub fn parse_find(output: &str) -> Vec<DirEntry> {
    output
        .lines()
        .filter_map(|line| {
            let mut cols = line.splitn(4, '\t');
            let kind = cols.next()?;
            let modified = parse_epoch(cols.next());
            let created = parse_epoch(cols.next());
            let name = cols.next()?;
            if name.is_empty() {
                return None;
            }
            Some(DirEntry {
                name: name.to_owned(),
                is_dir: kind == "d",
                modified,
                created,
            })
        })
        .collect()
}

/// A `find` `%T@`/`%B@` field (a float epoch, `-`, or `0`) as whole seconds;
/// `None` when unknown or non-positive.
fn parse_epoch(field: Option<&str>) -> Option<u64> {
    let secs: f64 = field?.trim().parse().ok()?;
    (secs > 0.0).then_some(secs as u64)
}

/// How a pane's entries are ordered. Directories always sort before files; this
/// picks the order *within* each group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    Name,
    ModifiedNewest,
    ModifiedOldest,
    CreatedNewest,
    CreatedOldest,
}

impl SortMode {
    /// The cycle order for the browser's sort key.
    pub const ALL: [SortMode; 5] = [
        SortMode::Name,
        SortMode::ModifiedNewest,
        SortMode::ModifiedOldest,
        SortMode::CreatedNewest,
        SortMode::CreatedOldest,
    ];

    /// A compact label for the pane header.
    pub fn label(self) -> &'static str {
        match self {
            SortMode::Name => "name",
            SortMode::ModifiedNewest => "modified↓",
            SortMode::ModifiedOldest => "modified↑",
            SortMode::CreatedNewest => "created↓",
            SortMode::CreatedOldest => "created↑",
        }
    }

    /// The next mode in the cycle (wraps).
    pub fn next(self) -> SortMode {
        let i = Self::ALL.iter().position(|&m| m == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

/// Sort entries for display: directories first, then files; within each group
/// by `mode` (case-insensitive name as the tie-breaker). A `None` timestamp
/// sorts as the oldest.
pub fn sort_entries(entries: &mut [DirEntry], mode: SortMode) {
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| cmp_within(a, b, mode)));
}

/// Order two same-group entries by the chosen key, breaking ties on name.
fn cmp_within(a: &DirEntry, b: &DirEntry, mode: SortMode) -> Ordering {
    let by_name = || a.name.to_lowercase().cmp(&b.name.to_lowercase());
    match mode {
        SortMode::Name => by_name(),
        SortMode::ModifiedNewest => b.modified.cmp(&a.modified).then_with(by_name),
        SortMode::ModifiedOldest => a.modified.cmp(&b.modified).then_with(by_name),
        SortMode::CreatedNewest => b.created.cmp(&a.created).then_with(by_name),
        SortMode::CreatedOldest => a.created.cmp(&b.created).then_with(by_name),
    }
}

/// Whether a directory entry should be shown given the pane's visibility toggle
/// and fuzzy filter. Dotfiles are hidden unless `show_hidden`; `..` is handled
/// by the caller (always shown so you can go up). Combines the hidden rule with
/// [`fuzzy_match`].
pub fn entry_visible(name: &str, show_hidden: bool, query: &str) -> bool {
    (show_hidden || !name.starts_with('.')) && fuzzy_match(name, query)
}

/// Fuzzy match: are all of `query`'s characters present in `name`, in order
/// (not necessarily adjacent), case-insensitively? An empty query matches
/// everything. Backs the browser's current-folder filter.
pub fn fuzzy_match(name: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let mut needles = query.chars().flat_map(char::to_lowercase).peekable();
    for hay in name.chars().flat_map(char::to_lowercase) {
        match needles.peek() {
            Some(&n) if n == hay => {
                needles.next();
            }
            _ => {}
        }
        if needles.peek().is_none() {
            return true;
        }
    }
    needles.peek().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta144_parse_ps_keeps_only_introdus_containers() {
        let out = "introdus-web-ab12\trunning\tintrodus-web-app-ab12:latest\n\
                   some-other-container\trunning\tnginx:latest\n\
                   introdus-api-cd34\trunning\tintrodus-api-cd34:latest\n";
        let cs = parse_ps(out);
        assert_eq!(cs.len(), 2);
        assert_eq!(cs[0].name, "introdus-web-ab12");
        assert_eq!(cs[0].state, "running");
        assert_eq!(cs[0].image, "introdus-web-app-ab12:latest");
        assert_eq!(cs[1].name, "introdus-api-cd34");
    }

    #[test]
    fn ta144_parse_ps_tolerates_short_and_blank_lines() {
        let out = "\nintrodus-solo\n\ngarbage\n";
        let cs = parse_ps(out);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].name, "introdus-solo");
        assert_eq!(cs[0].state, "");
    }

    #[test]
    fn ta144_parse_ls_marks_dirs_by_trailing_slash() {
        let es = parse_ls("src/\nCargo.toml\n.hidden\nnode_modules/\n");
        assert_eq!(
            es,
            vec![
                DirEntry::bare("src", true),
                DirEntry::bare("Cargo.toml", false),
                DirEntry::bare(".hidden", false),
                DirEntry::bare("node_modules", true),
            ]
        );
    }

    #[test]
    fn ta144_sort_by_name_dirs_first_ci() {
        let mut es = parse_ls("Zoo.txt\nbeta/\nApple.md\nalpha/\n");
        sort_entries(&mut es, SortMode::Name);
        assert_eq!(names(&es), vec!["alpha", "beta", "Apple.md", "Zoo.txt"]);
    }

    #[test]
    fn ta151_parse_find_reads_type_and_times() {
        // type \t mtime \t btime \t name
        let out = "d\t1700.9\t1000.0\tsrc\n\
                   f\t1800.5\t-\tmain.rs\n\
                   f\t1600.0\t0\told.txt\n";
        let es = parse_find(out);
        assert_eq!(
            es,
            vec![
                DirEntry {
                    name: "src".into(),
                    is_dir: true,
                    modified: Some(1700),
                    created: Some(1000)
                },
                // birth `-` (unknown) → None
                DirEntry {
                    name: "main.rs".into(),
                    is_dir: false,
                    modified: Some(1800),
                    created: None
                },
                // birth `0` (unsupported) → None
                DirEntry {
                    name: "old.txt".into(),
                    is_dir: false,
                    modified: Some(1600),
                    created: None
                },
            ]
        );
    }

    #[test]
    fn ta151_sort_by_modified_and_created() {
        let mk = |n: &str, m: u64, c: u64| DirEntry {
            name: n.into(),
            is_dir: false,
            modified: Some(m),
            created: Some(c),
        };
        let base = vec![mk("a", 10, 100), mk("b", 30, 200), mk("c", 20, 300)];

        let mut newest = base.clone();
        sort_entries(&mut newest, SortMode::ModifiedNewest);
        assert_eq!(names(&newest), vec!["b", "c", "a"]); // 30,20,10

        let mut oldest = base.clone();
        sort_entries(&mut oldest, SortMode::ModifiedOldest);
        assert_eq!(names(&oldest), vec!["a", "c", "b"]); // 10,20,30

        let mut cnew = base.clone();
        sort_entries(&mut cnew, SortMode::CreatedNewest);
        assert_eq!(names(&cnew), vec!["c", "b", "a"]); // 300,200,100
    }

    #[test]
    fn ta151_sort_keeps_dirs_first_and_none_is_oldest() {
        let mut es = vec![
            DirEntry {
                name: "file-new".into(),
                is_dir: false,
                modified: Some(99),
                created: None,
            },
            DirEntry {
                name: "dir".into(),
                is_dir: true,
                modified: Some(1),
                created: None,
            },
            DirEntry {
                name: "file-unknown".into(),
                is_dir: false,
                modified: None,
                created: None,
            },
        ];
        sort_entries(&mut es, SortMode::ModifiedNewest);
        // Dir first regardless of time; among files, known-newest then unknown.
        assert_eq!(names(&es), vec!["dir", "file-new", "file-unknown"]);
    }

    #[test]
    fn ta151_sort_mode_cycles() {
        assert_eq!(SortMode::Name.next(), SortMode::ModifiedNewest);
        assert_eq!(SortMode::CreatedOldest.next(), SortMode::Name);
        assert_eq!(SortMode::Name.label(), "name");
    }

    #[test]
    fn ta153_entry_visible_hides_dotfiles_unless_toggled() {
        // Hidden by default…
        assert!(!entry_visible(".bashrc", false, ""));
        assert!(entry_visible("Cargo.toml", false, ""));
        // …shown when the toggle is on.
        assert!(entry_visible(".bashrc", true, ""));
        // Combines with the fuzzy filter (hidden rule applies first).
        assert!(!entry_visible(".config", false, "cfg"));
        assert!(entry_visible(".config", true, "cfg"));
        assert!(!entry_visible("readme", true, "xyz"));
    }

    #[test]
    fn ta152_fuzzy_match_subsequence_ci() {
        assert!(fuzzy_match("Cargo.toml", "")); // empty matches all
        assert!(fuzzy_match("Cargo.toml", "cto")); // subsequence, case-insensitive
        assert!(fuzzy_match("src/main.rs", "mn")); // gap allowed
        assert!(!fuzzy_match("Cargo.toml", "xyz"));
        assert!(!fuzzy_match("abc", "acb")); // order matters
        assert!(fuzzy_match(".bashrc", "bash"));
    }

    fn names(es: &[DirEntry]) -> Vec<&str> {
        es.iter().map(|e| e.name.as_str()).collect()
    }
}
