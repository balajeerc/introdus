//! Parsing the podman output the `send-files` flow depends on, kept pure so it
//! is unit-testable without a live podman: the running-container list
//! (`podman ps`) and a container directory listing (`podman exec … ls -1Ap`).

/// The `--format` template for [`parse_ps`]: tab-separated name / state / image.
pub const PS_FORMAT: &str = "{{.Names}}\t{{.State}}\t{{.Image}}";

/// The `ls` flags whose output [`parse_ls`] parses: one entry per line (`-1`),
/// include dotfiles but not `.`/`..` (`-A`), and append `/` to directories
/// (`-p`) so a listing distinguishes dirs from files without a second stat.
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
}

/// Parse `ls {LS_FLAGS} <dir>` output into entries. A trailing `/` (from `-p`)
/// marks a directory and is stripped from the stored name. Blank lines are
/// ignored; the list is returned in ls order (caller sorts if it wants).
pub fn parse_ls(output: &str) -> Vec<DirEntry> {
    output
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            if let Some(dir) = line.strip_suffix('/') {
                DirEntry {
                    name: dir.to_owned(),
                    is_dir: true,
                }
            } else {
                DirEntry {
                    name: line.to_owned(),
                    is_dir: false,
                }
            }
        })
        .collect()
}

/// Sort entries for display: directories first, then files, each group
/// case-insensitive alphabetical. Stable for entries that compare equal.
pub fn sort_entries(entries: &mut [DirEntry]) {
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
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
        // A name-only line (no state/image columns) is still a valid container;
        // a non-introdus or blank line is dropped.
        let out = "\nintrodus-solo\n\ngarbage\n";
        let cs = parse_ps(out);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].name, "introdus-solo");
        assert_eq!(cs[0].state, "");
    }

    #[test]
    fn ta144_parse_ls_marks_dirs_by_trailing_slash() {
        let out = "src/\nCargo.toml\n.hidden\nnode_modules/\n";
        let es = parse_ls(out);
        assert_eq!(
            es,
            vec![
                DirEntry {
                    name: "src".into(),
                    is_dir: true
                },
                DirEntry {
                    name: "Cargo.toml".into(),
                    is_dir: false
                },
                DirEntry {
                    name: ".hidden".into(),
                    is_dir: false
                },
                DirEntry {
                    name: "node_modules".into(),
                    is_dir: true
                },
            ]
        );
    }

    #[test]
    fn ta144_sort_entries_dirs_first_then_alpha_ci() {
        let mut es = parse_ls("Zoo.txt\nbeta/\nApple.md\nalpha/\n");
        sort_entries(&mut es);
        let names: Vec<&str> = es.iter().map(|e| e.name.as_str()).collect();
        // Dirs (alpha, beta) first, then files (Apple.md, Zoo.txt) — both
        // case-insensitive.
        assert_eq!(names, vec!["alpha", "beta", "Apple.md", "Zoo.txt"]);
    }
}
