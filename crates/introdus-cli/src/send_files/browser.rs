//! The dual-pane file manager: the laptop filesystem on the left, the chosen
//! container's filesystem on the right. You navigate both, `Space`-pick a
//! file/folder on the left, and `s`-send it into the right pane's current
//! directory. The container side is listed live via `podman exec … ls` (wrapped
//! in ssh for a remote host); the transfer itself is [`super::transfer::send`],
//! run behind a spinner so a large copy doesn't freeze the UI.

use std::path::Path;

use anyhow::Result;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use introdus_core::containers::{parse_ls, sort_entries, DirEntry, LS_FLAGS};
use introdus_core::remote::Location;

use crate::ui::{ACCENT, DIM};

use super::{transfer, App};

/// Where a pane's entries come from — the local FS or a container's FS.
enum PaneSource {
    Local,
    Container { loc: Location, container: String },
}

/// One side of the browser: a current directory, its (sorted) entries, and the
/// cursor. A synthetic `..` leads the list except at the filesystem root.
///
/// `state` is the ratatui [`ListState`] and is **persisted across frames** (not
/// rebuilt per draw): the widget maintains its own scroll offset from it, so the
/// selection moves within the viewport and only scrolls at the edges. Rebuilding
/// it each frame (offset 0) would re-derive the offset from scratch and pin the
/// selection to the bottom on the way down and back up.
struct Pane {
    title: &'static str,
    cwd: String,
    entries: Vec<DirEntry>,
    cursor: usize,
    source: PaneSource,
    state: ListState,
}

impl Pane {
    fn new(title: &'static str, cwd: String, source: PaneSource) -> Self {
        Self {
            title,
            cwd,
            entries: Vec::new(),
            cursor: 0,
            source,
            state: ListState::default(),
        }
    }

    /// Point the persisted `ListState` at the current cursor without disturbing
    /// the scroll offset — for cursor moves within the same listing.
    fn sync_selection(&mut self) {
        self.state
            .select((!self.entries.is_empty()).then_some(self.cursor));
    }

    /// List the current directory from the backing source (no `..` synthesis).
    fn list(&self) -> Result<Vec<DirEntry>> {
        match &self.source {
            PaneSource::Local => list_local(&self.cwd),
            PaneSource::Container { loc, container } => {
                let out = loc
                    .podman(&[
                        "exec", "--user", "dev", container, "ls", LS_FLAGS, "--", &self.cwd,
                    ])
                    .stdout_quiet()?;
                Ok(parse_ls(&out))
            }
        }
    }

    /// Re-list the current directory into `entries`, sorted, with a leading `..`
    /// unless we're at `/`. Clamps the cursor into range.
    fn refresh(&mut self) -> Result<()> {
        let mut entries = self.list()?;
        sort_entries(&mut entries);
        if self.cwd != "/" {
            entries.insert(
                0,
                DirEntry {
                    name: "..".to_owned(),
                    is_dir: true,
                },
            );
        }
        self.entries = entries;
        if self.cursor >= self.entries.len() {
            self.cursor = self.entries.len().saturating_sub(1);
        }
        // A fresh listing starts scrolled to the top; drop any offset carried
        // over from the previous directory, then re-point the selection.
        *self.state.offset_mut() = 0;
        self.sync_selection();
        Ok(())
    }

    fn selected(&self) -> Option<&DirEntry> {
        self.entries.get(self.cursor)
    }

    /// The absolute path of the selected entry (`None` for the `..` row).
    fn selected_path(&self) -> Option<String> {
        let sel = self.selected()?;
        if sel.name == ".." {
            None
        } else {
            Some(join(&self.cwd, &sel.name))
        }
    }

    /// Enter the selected directory (or ascend on `..`); a file is a no-op. On a
    /// listing error the cwd is left unchanged and the error is returned.
    fn enter(&mut self) -> Result<()> {
        let Some(sel) = self.selected() else {
            return Ok(());
        };
        let prev = self.cwd.clone();
        if sel.name == ".." {
            self.cwd = parent(&self.cwd);
        } else if sel.is_dir {
            self.cwd = join(&self.cwd, &sel.name);
        } else {
            return Ok(());
        }
        self.cursor = 0;
        if let Err(e) = self.refresh() {
            self.cwd = prev; // roll back to a directory we can still list
            self.refresh().ok();
            return Err(e);
        }
        Ok(())
    }

    fn move_cursor(&mut self, delta: isize) {
        let len = self.entries.len();
        if len == 0 {
            return;
        }
        let cur = self.cursor as isize + delta;
        self.cursor = cur.clamp(0, len as isize - 1) as usize;
        // Keep the persisted ListState in step; its offset is preserved, so
        // ratatui moves the highlight within the viewport and only scrolls once
        // the selection would leave it.
        self.sync_selection();
    }
}

/// List a local directory into unsorted [`DirEntry`]s (symlinks are not
/// followed, matching container-side `ls -p`).
fn list_local(dir: &str) -> Result<Vec<DirEntry>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        entries.push(DirEntry { name, is_dir });
    }
    Ok(entries)
}

/// Join `name` onto directory `dir` with exactly one separator (root-aware).
fn join(dir: &str, name: &str) -> String {
    if dir == "/" {
        format!("/{name}")
    } else {
        format!("{}/{}", dir.trim_end_matches('/'), name)
    }
}

/// The parent directory of `dir` (root's parent is root).
fn parent(dir: &str) -> String {
    let trimmed = dir.trim_end_matches('/');
    match trimmed.rfind('/') {
        None | Some(0) => "/".to_owned(),
        Some(i) => trimmed[..i].to_owned(),
    }
}

fn basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

/// Run the browser for one container. Returns when the user quits (Esc/q),
/// which the caller treats as "back to the container picker".
pub fn browse(app: &mut App, loc: &Location, container: &str) -> Result<()> {
    let start = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_owned))
        .unwrap_or_else(|| "/".to_owned());
    let mut left = Pane::new("LOCAL", start, PaneSource::Local);
    let mut right = Pane::new(
        "CONTAINER",
        "/home/dev".to_owned(),
        PaneSource::Container {
            loc: loc.clone(),
            container: container.to_owned(),
        },
    );

    let mut status = String::new();
    if let Err(e) = left.refresh() {
        status = format!("local: {e}");
    }
    if let Err(e) = right.refresh() {
        status = format!("container: {e}");
    }

    let mut active_left = true;
    let mut source: Option<String> = None;

    loop {
        let header = format!("{} → {}", container, loc.label());
        // `browser` is a child module of `send_files`, so it may reach the
        // parent `App`'s terminal directly — keeping all pane layout beside the
        // pane state (the private `Pane` type can't cross back up to `mod.rs`).
        app.term.draw(|f| {
            render(
                f,
                &header,
                &mut left,
                &mut right,
                active_left,
                source.as_deref(),
                &status,
            )
        })?;

        let Some((code, mods)) = crate::ui::next_key()? else {
            continue;
        };
        use ratatui::crossterm::event::KeyCode::*;
        if crate::ui::is_ctrl_c(code, mods) {
            return Ok(());
        }
        match code {
            Esc | Char('q') => return Ok(()),
            Tab | BackTab => active_left = !active_left,
            Left => active_left = true,
            Right => active_left = false,
            Up => cur_pane(&mut left, &mut right, active_left).move_cursor(-1),
            Down => cur_pane(&mut left, &mut right, active_left).move_cursor(1),
            Enter => {
                if let Err(e) = cur_pane(&mut left, &mut right, active_left).enter() {
                    status = format!("can't open: {e}");
                }
            }
            Char(' ') => {
                if active_left {
                    match left.selected_path() {
                        Some(p) => {
                            status = format!("picked {}", basename(&p));
                            source = Some(p);
                        }
                        None => status = "pick a file or folder, not `..`".to_owned(),
                    }
                } else {
                    status = "pick the source on the LEFT (this machine)".to_owned();
                }
            }
            Char('s') | Char('S') => {
                status = send(app, loc, container, source.as_deref(), &mut right);
            }
            _ => {}
        }
    }
}

/// The pane the cursor currently drives.
fn cur_pane<'a>(left: &'a mut Pane, right: &'a mut Pane, active_left: bool) -> &'a mut Pane {
    if active_left {
        left
    } else {
        right
    }
}

/// Perform a send of the picked source into the right pane's directory,
/// returning the status line to show. Refreshes the right pane on success so the
/// delivered entry appears.
fn send(
    app: &mut App,
    loc: &Location,
    container: &str,
    source: Option<&str>,
    right: &mut Pane,
) -> String {
    let Some(src) = source else {
        return "pick a source first: Space on the left pane".to_owned();
    };
    let name = basename(src);
    let dest = right.cwd.clone();
    let label = format!("sending {name} → {dest}…");
    let (loc_c, cont_c, dest_c, src_c) = (
        loc.clone(),
        container.to_owned(),
        dest.clone(),
        std::path::PathBuf::from(src),
    );
    let outcome = app.spinner(&label, move || {
        transfer::send(&loc_c, &cont_c, &src_c, &dest_c)
    });
    match outcome {
        Ok(Ok(())) => {
            right.refresh().ok();
            format!("✓ sent {name} → {dest}")
        }
        Ok(Err(e)) => format!("✗ {e}"),
        Err(e) => format!("✗ {e}"),
    }
}

// ---- rendering (called by App::draw_browser via the shared terminal) --------

/// Draw the two-pane browser into `f`. Kept here (not in `mod.rs`) so all
/// browser layout lives beside its state.
fn render(
    f: &mut Frame,
    header: &str,
    left: &mut Pane,
    right: &mut Pane,
    active_left: bool,
    source: Option<&str>,
    status: &str,
) {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(f.area());

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "send-files ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(header.to_owned()),
        ])),
        rows[0],
    );
    let src_line = match source {
        Some(p) => Line::from(vec![
            Span::styled("source: ", Style::default().fg(DIM)),
            Span::styled(p.to_owned(), Style::default().fg(ACCENT)),
        ]),
        None => Line::from(Span::styled(
            "source: (none — Space to pick on the left)",
            Style::default().fg(DIM),
        )),
    };
    f.render_widget(Paragraph::new(src_line), rows[1]);

    let cols =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[2]);
    render_pane(f, cols[0], left, active_left);
    render_pane(f, cols[1], right, !active_left);

    f.render_widget(footer(status), rows[3]);
}

/// Draw one pane's bordered list, driving the pane's **persisted** `ListState`
/// (so scrolling is stable across frames).
fn render_pane(f: &mut Frame, area: Rect, pane: &mut Pane, active: bool) {
    let border = if active { ACCENT } else { DIM };
    let title = format!(" {}  {} ", pane.title, pane.cwd);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .title(Span::styled(title, Style::default().fg(border)));

    let items: Vec<ListItem> = pane
        .entries
        .iter()
        .map(|e| {
            let label = if e.is_dir {
                format!("{}/", e.name)
            } else {
                e.name.clone()
            };
            let style = if e.is_dir {
                Style::default().fg(ACCENT)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(format!(" {label}"), style)))
        })
        .collect();

    let hl = if active {
        Style::default()
            .bg(ACCENT)
            .fg(ratatui::style::Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::REVERSED)
    };
    f.render_stateful_widget(
        List::new(items).block(block).highlight_style(hl),
        area,
        &mut pane.state,
    );
}

fn footer(status: &str) -> Paragraph<'static> {
    let hint = "Tab switch · ↑↓ move · Enter open · Space pick(left) · s send · Esc back";
    let line = if status.is_empty() {
        Line::from(Span::styled(hint, Style::default().fg(DIM)))
    } else {
        Line::from(vec![
            Span::styled(status.to_owned(), Style::default().fg(ACCENT)),
            Span::styled(format!("   {hint}"), Style::default().fg(DIM)),
        ])
    };
    Paragraph::new(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta147_join_is_root_aware() {
        assert_eq!(join("/home/dev", "src"), "/home/dev/src");
        assert_eq!(join("/home/dev/", "src"), "/home/dev/src");
        assert_eq!(join("/", "etc"), "/etc");
    }

    #[test]
    fn ta147_parent_walks_up_and_stops_at_root() {
        assert_eq!(parent("/home/dev/work"), "/home/dev");
        assert_eq!(parent("/home"), "/");
        assert_eq!(parent("/"), "/");
        assert_eq!(parent("/home/dev/"), "/home");
    }

    #[test]
    fn ta147_basename_of_path() {
        assert_eq!(basename("/a/b/c.txt"), "c.txt");
        assert_eq!(basename("/a/b/"), "b");
        assert_eq!(basename("solo"), "solo");
    }
}
