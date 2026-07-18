//! The dual-pane file manager: the laptop filesystem on the left, the chosen
//! container's filesystem on the right. Navigate both, `Space`-pick a
//! file/folder on the left, and `s`-send it into the right pane's current
//! directory. Each pane can be re-sorted (`o` cycles name / modified / created)
//! and fuzzy-filtered on the current folder (`/`). The container side is listed
//! live via `podman exec … find` (falling back to `ls`), wrapped in ssh for a
//! remote host; the transfer itself is [`super::transfer::send`], run behind a
//! spinner so a large copy doesn't freeze the UI.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use introdus_core::containers::{
    fuzzy_match, parse_find, parse_ls, sort_entries, DirEntry, SortMode, FIND_PRINTF, LS_FLAGS,
};
use introdus_core::remote::Location;

use crate::ui::{ACCENT, DIM};

use super::{transfer, App};

/// Where a pane's entries come from — the local FS or a container's FS.
enum PaneSource {
    Local,
    Container { loc: Location, container: String },
}

/// One side of the browser. `all` is the full sorted listing (a synthetic `..`
/// leads it except at `/`); `view` holds the indices of `all` passing the fuzzy
/// `filter`, and `cursor` indexes `view`. `state` is the ratatui [`ListState`],
/// **persisted across frames** so the widget keeps its own scroll offset — the
/// highlight moves within the viewport and only scrolls at the edges.
struct Pane {
    title: &'static str,
    cwd: String,
    source: PaneSource,
    state: ListState,
    all: Vec<DirEntry>,
    view: Vec<usize>,
    cursor: usize,
    sort: SortMode,
    filter: String,
}

impl Pane {
    fn new(title: &'static str, cwd: String, source: PaneSource) -> Self {
        Self {
            title,
            cwd,
            source,
            state: ListState::default(),
            all: Vec::new(),
            view: Vec::new(),
            cursor: 0,
            sort: SortMode::Name,
            filter: String::new(),
        }
    }

    fn sync_selection(&mut self) {
        self.state
            .select((!self.view.is_empty()).then_some(self.cursor));
    }

    /// List the current directory from the backing source (no `..` synthesis).
    /// The container side prefers `find` (which yields mtime/btime for sorting)
    /// and falls back to `ls` on a container without findutils.
    fn list(&self) -> Result<Vec<DirEntry>> {
        match &self.source {
            PaneSource::Local => list_local(&self.cwd),
            PaneSource::Container { loc, container } => {
                let find = loc
                    .podman(&[
                        "exec",
                        "--user",
                        "dev",
                        container,
                        "find",
                        &self.cwd,
                        "-maxdepth",
                        "1",
                        "-mindepth",
                        "1",
                        "-printf",
                        FIND_PRINTF,
                    ])
                    .stdout_quiet();
                match find {
                    Ok(out) => Ok(parse_find(&out)),
                    Err(_) => {
                        let out = loc
                            .podman(&[
                                "exec", "--user", "dev", container, "ls", LS_FLAGS, "--", &self.cwd,
                            ])
                            .stdout_quiet()?;
                        Ok(parse_ls(&out))
                    }
                }
            }
        }
    }

    /// Re-list the current directory: sort the entries, prepend `..` (unless at
    /// `/`), rebuild the filtered view, and reset the scroll to the top.
    fn refresh(&mut self) -> Result<()> {
        let mut real = self.list()?;
        sort_entries(&mut real, self.sort);
        self.all.clear();
        if self.cwd != "/" {
            self.all.push(DirEntry::bare("..", true));
        }
        self.all.extend(real);
        self.cursor = 0;
        *self.state.offset_mut() = 0;
        self.rebuild_view();
        Ok(())
    }

    /// Recompute `view` from `all` under the current filter (`..` always shows,
    /// so you can always go up), clamp the cursor, and re-point the selection.
    fn rebuild_view(&mut self) {
        self.view = self
            .all
            .iter()
            .enumerate()
            .filter(|(_, e)| e.name == ".." || fuzzy_match(&e.name, &self.filter))
            .map(|(i, _)| i)
            .collect();
        if self.cursor >= self.view.len() {
            self.cursor = self.view.len().saturating_sub(1);
        }
        self.sync_selection();
    }

    /// Re-sort in place with the next sort mode, keeping `..` pinned at the top.
    fn cycle_sort(&mut self) {
        self.sort = self.sort.next();
        let start = usize::from(self.cwd != "/"); // keep a leading `..`
        let mut real = self.all.split_off(start);
        sort_entries(&mut real, self.sort);
        self.all.extend(real);
        self.cursor = 0;
        *self.state.offset_mut() = 0;
        self.rebuild_view();
    }

    /// Apply a new fuzzy filter to the current folder (top of the list).
    fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.cursor = 0;
        *self.state.offset_mut() = 0;
        self.rebuild_view();
    }

    fn selected(&self) -> Option<&DirEntry> {
        self.view.get(self.cursor).and_then(|&i| self.all.get(i))
    }

    /// The absolute path of the selected entry (`None` for the `..` row).
    fn selected_path(&self) -> Option<String> {
        let sel = self.selected()?;
        (sel.name != "..").then(|| join(&self.cwd, &sel.name))
    }

    /// Enter the selected directory (or ascend on `..`); a file is a no-op. The
    /// filter clears on a directory change. On a listing error the cwd is left
    /// unchanged and the error is returned.
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
        self.filter.clear();
        if let Err(e) = self.refresh() {
            self.cwd = prev; // roll back to a directory we can still list
            self.refresh().ok();
            return Err(e);
        }
        Ok(())
    }

    fn move_cursor(&mut self, delta: isize) {
        let len = self.view.len();
        if len == 0 {
            return;
        }
        let cur = self.cursor as isize + delta;
        self.cursor = cur.clamp(0, len as isize - 1) as usize;
        self.sync_selection();
    }
}

/// List a local directory into [`DirEntry`]s with timestamps (symlinks are not
/// followed for the type, matching container-side `find`/`ls -p`).
fn list_local(dir: &str) -> Result<Vec<DirEntry>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let meta = entry.metadata().ok();
        let modified = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(epoch);
        let created = meta.as_ref().and_then(|m| m.created().ok()).and_then(epoch);
        entries.push(DirEntry {
            name,
            is_dir,
            modified,
            created,
        });
    }
    Ok(entries)
}

/// A `SystemTime` as whole seconds since the Unix epoch (`None` before it).
fn epoch(t: SystemTime) -> Option<u64> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
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
    let mut filtering = false;

    loop {
        let header = format!("{container} → {}", loc.label());
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
                filtering,
            )
        })?;

        let Some((code, mods)) = crate::ui::next_key()? else {
            continue;
        };
        if crate::ui::is_ctrl_c(code, mods) {
            return Ok(());
        }
        if filtering {
            handle_filter_key(
                code,
                cur_pane(&mut left, &mut right, active_left),
                &mut filtering,
            );
            continue;
        }
        use ratatui::crossterm::event::KeyCode::*;
        match code {
            Esc | Char('q') => return Ok(()),
            Tab | BackTab => active_left = !active_left,
            Left => active_left = true,
            Right => active_left = false,
            Up => cur_pane(&mut left, &mut right, active_left).move_cursor(-1),
            Down => cur_pane(&mut left, &mut right, active_left).move_cursor(1),
            Char('/') => filtering = true,
            Char('o') => cur_pane(&mut left, &mut right, active_left).cycle_sort(),
            Enter => {
                if let Err(e) = cur_pane(&mut left, &mut right, active_left).enter() {
                    status = format!("can't open: {e}");
                }
            }
            Char(' ') => status = pick(&left, active_left, &mut source),
            Char('s') | Char('S') => {
                status = send(app, loc, container, source.as_deref(), &mut right)
            }
            _ => {}
        }
    }
}

/// Feed a keystroke to the active pane's live filter editor.
fn handle_filter_key(
    code: ratatui::crossterm::event::KeyCode,
    pane: &mut Pane,
    filtering: &mut bool,
) {
    use ratatui::crossterm::event::KeyCode::*;
    match code {
        Esc => {
            pane.set_filter(String::new());
            *filtering = false;
        }
        Enter => *filtering = false,
        Backspace => {
            let mut f = pane.filter.clone();
            f.pop();
            pane.set_filter(f);
        }
        Char(c) => {
            let mut f = pane.filter.clone();
            f.push(c);
            pane.set_filter(f);
        }
        Up => pane.move_cursor(-1),
        Down => pane.move_cursor(1),
        _ => {}
    }
}

/// Mark the left pane's selection as the send source; returns the status line.
fn pick(left: &Pane, active_left: bool, source: &mut Option<String>) -> String {
    if !active_left {
        return "pick the source on the LEFT (this machine)".to_owned();
    }
    match left.selected_path() {
        Some(p) => {
            let msg = format!("picked {}", basename(&p));
            *source = Some(p);
            msg
        }
        None => "pick a file or folder, not `..`".to_owned(),
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

// ---- rendering --------------------------------------------------------------

/// Draw the two-pane browser into `f`. Kept here (not in `mod.rs`) so all
/// browser layout lives beside its state.
#[allow(clippy::too_many_arguments)]
fn render(
    f: &mut Frame,
    header: &str,
    left: &mut Pane,
    right: &mut Pane,
    active_left: bool,
    source: Option<&str>,
    status: &str,
    filtering: bool,
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
    // Snapshot the active pane's filter for the editor line *before* the mutable
    // per-pane borrows below (render drives each pane's persisted `ListState`).
    let (atitle, afilter) = {
        let a = if active_left { &*left } else { &*right };
        (a.title, a.filter.clone())
    };
    f.render_widget(
        Paragraph::new(status_line(source, atitle, &afilter, filtering)),
        rows[1],
    );

    let cols =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[2]);
    render_pane(f, cols[0], left, active_left);
    render_pane(f, cols[1], right, !active_left);

    f.render_widget(footer(status), rows[3]);
}

/// The second header row: the live filter editor while filtering, else the
/// current send source.
fn status_line<'a>(
    source: Option<&str>,
    active_title: &str,
    active_filter: &str,
    filtering: bool,
) -> Line<'a> {
    if filtering {
        return Line::from(vec![
            Span::styled(
                format!("filter [{active_title}]: "),
                Style::default().fg(DIM),
            ),
            Span::styled(
                format!("{active_filter}▌"),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
        ]);
    }
    match source {
        Some(p) => Line::from(vec![
            Span::styled("source: ", Style::default().fg(DIM)),
            Span::styled(p.to_owned(), Style::default().fg(ACCENT)),
        ]),
        None => Line::from(Span::styled(
            "source: (none — Space to pick on the left)",
            Style::default().fg(DIM),
        )),
    }
}

/// Draw one pane's bordered list, driving the pane's **persisted** `ListState`
/// (so scrolling is stable across frames).
fn render_pane(f: &mut Frame, area: Rect, pane: &mut Pane, active: bool) {
    let color = if active { ACCENT } else { DIM };
    let mut title = format!(" {}  {}  ·{}", pane.title, pane.cwd, pane.sort.label());
    if !pane.filter.is_empty() {
        title.push_str(&format!("  /{}", pane.filter));
    }
    title.push(' ');
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color))
        .title(Span::styled(title, Style::default().fg(color)));

    let items: Vec<ListItem> = pane
        .view
        .iter()
        .map(|&i| {
            let e = &pane.all[i];
            let (label, style) = if e.is_dir {
                (format!("{}/", e.name), Style::default().fg(ACCENT))
            } else {
                (e.name.clone(), Style::default())
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
    let hint =
        "Tab switch · ↑↓ move · Enter open · Space pick · s send · / filter · o sort · Esc back";
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
