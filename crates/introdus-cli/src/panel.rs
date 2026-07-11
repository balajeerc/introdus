//! The persistent two-pane control panel. Unlike the wizard's inline modals,
//! this owns the alternate screen for the whole `introdus menu` session: the
//! left column shows the live status + the grouped, filterable menu; the right
//! column is an output pane where each action's output streams in (instead of
//! clearing the screen and pausing). Prompts appear as centered popups.
//!
//! Action output reaches the pane two ways: the action's own [`Ui::log`] calls,
//! and — transparently — every external command it runs, because [`Ui::new`]
//! installs a [`process::capture_stdio`] guard that redirects `Cmd` output into
//! the same buffer instead of the screen.

use std::cell::RefCell;
use std::io::{self, Stdout};
use std::rc::Rc;
use std::time::Duration;

use anyhow::{bail, Result};
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use introdus_core::process::{self, CaptureGuard, OutputBuffer};

use crate::ui::{
    self, confirm_line, confirm_step, next_key, question, text_render, text_step, visible_items,
    Picker, Row, Status, Step, ACCENT, DIM,
};

type Backend = CrosstermBackend<Stdout>;

/// How long the menu waits for a keypress before returning [`Selection::Tick`]
/// so the caller can re-snapshot the (possibly just-started) container status.
const STATUS_POLL: Duration = Duration::from_secs(2);

/// The result of one turn at the menu.
pub enum Selection {
    /// The user chose the item at this index into `rows`.
    Item(usize),
    /// The user quit the menu (Esc / Ctrl-C).
    Quit,
    /// No input within [`STATUS_POLL`] — re-snapshot the status and redraw.
    Tick,
}

/// A centered popup prompt drawn over the two-pane frame.
enum Popup<'a> {
    Confirm {
        prompt: &'a str,
        answer: bool,
    },
    Text {
        prompt: &'a str,
        buf: &'a str,
        hidden: bool,
    },
    Pick {
        prompt: &'a str,
        items: &'a [String],
        picker: &'a Picker,
    },
}

/// Braille spinner frames for the "action in progress" indicator.
const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Owns the alternate screen + the output pane for a control-menu session.
pub struct Ui {
    term: Terminal<Backend>,
    out: OutputBuffer,
    status: Status,
    rows: Vec<Row>,
    /// Selection index into the *visible* (filtered) item list.
    sel: usize,
    query: String,
    /// When set, a long action is running: the footer shows a spinner and
    /// keystrokes are ignored (the menu is disabled until it finishes).
    busy: Option<String>,
    spin: usize,
    _capture: CaptureGuard,
}

impl Ui {
    pub fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let mut term = Terminal::new(CrosstermBackend::new(stdout))?;
        term.hide_cursor()?;
        let out: OutputBuffer = Rc::new(RefCell::new(Vec::new()));
        let capture = process::capture_stdio(out.clone());
        Ok(Self {
            term,
            out,
            status: blank_status(),
            rows: Vec::new(),
            sel: 0,
            query: String::new(),
            busy: None,
            spin: 0,
            _capture: capture,
        })
    }

    /// Refresh the left-column status + menu rows for the next turn.
    pub fn set_menu(&mut self, status: Status, rows: Vec<Row>) {
        self.status = status;
        self.rows = rows;
    }

    /// Append a progress line to the output pane and redraw.
    pub fn log(&mut self, line: impl Into<String>) {
        self.out.borrow_mut().push(line.into());
        let _ = self.draw(None);
    }

    /// Announce the start of an action in the output pane (a visual separator).
    pub fn begin(&mut self, title: &str) {
        {
            let mut out = self.out.borrow_mut();
            if !out.is_empty() {
                out.push(String::new());
            }
            out.push(format!("▶ {title}"));
        }
        let _ = self.draw(None);
    }

    /// Run a long command as a foreground *task*: it streams its output into the
    /// pane line-by-line on a worker thread while the panel shows a spinner, and
    /// — crucially — all keystrokes are discarded until it finishes, so the menu
    /// is disabled and no other action can be started (or queued) meanwhile.
    pub fn run_task(&mut self, label: &str, cmd: introdus_core::process::Cmd) -> Result<()> {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let handle = std::thread::spawn(move || cmd.stream(tx));
        self.busy = Some(label.to_owned());
        let outcome = loop {
            let mut drained_any = false;
            while let Ok(line) = rx.try_recv() {
                self.out.borrow_mut().push(line);
                drained_any = true;
            }
            self.spin = self.spin.wrapping_add(1);
            let _ = self.draw(None);
            if handle.is_finished() && !drained_any {
                while let Ok(line) = rx.try_recv() {
                    self.out.borrow_mut().push(line);
                }
                break handle.join();
            }
            // Wait briefly for input and throw it away — the task owns the UI.
            if ratatui::crossterm::event::poll(Duration::from_millis(120))? {
                let _ = ratatui::crossterm::event::read()?;
            }
        };
        self.busy = None;
        match outcome {
            Ok(Ok(true)) => Ok(()),
            Ok(Ok(false)) => bail!("`{label}` exited non-zero"),
            Ok(Err(e)) => Err(e),
            Err(_) => bail!("`{label}` task thread panicked"),
        }
    }

    /// Discard any keystrokes buffered while an action was running, so keys
    /// mashed during a blocking step don't fire as a cascade of menu selections.
    pub fn drain_input(&self) {
        while ratatui::crossterm::event::poll(Duration::from_millis(0)).unwrap_or(false) {
            let _ = ratatui::crossterm::event::read();
        }
    }

    /// Run one turn at the menu: filter/navigate until the user picks an item or
    /// quits.
    pub fn run_menu(&mut self) -> Result<Selection> {
        loop {
            let visible = visible_items(&self.rows, &self.query);
            if self.sel >= visible.len() {
                self.sel = visible.len().saturating_sub(1);
            }
            self.draw(None)?;
            // Poll rather than block, so the status panel keeps up with a
            // container that starts (or stops) while we're sitting at the menu.
            if !ratatui::crossterm::event::poll(STATUS_POLL)? {
                return Ok(Selection::Tick);
            }
            let Some((code, mods)) = next_key()? else {
                continue;
            };
            if ui::is_ctrl_c(code, mods) {
                return Ok(Selection::Quit);
            }
            use ratatui::crossterm::event::KeyCode::*;
            match code {
                Esc => return Ok(Selection::Quit),
                Enter => {
                    if let Some(&idx) = visible.get(self.sel) {
                        // Reset the filter so the next turn starts from the full
                        // menu (the query persists across turns otherwise).
                        self.query.clear();
                        self.sel = 0;
                        return Ok(Selection::Item(idx));
                    }
                }
                Up => self.sel = self.sel.saturating_sub(1),
                Down if self.sel + 1 < visible.len() => self.sel += 1,
                Backspace => {
                    self.query.pop();
                    self.sel = 0;
                }
                Char(c) => {
                    self.query.push(c);
                    self.sel = 0;
                }
                _ => {}
            }
        }
    }

    // ---- popup prompts (mirror the wizard's inline modals) -----------------

    pub fn confirm(&mut self, prompt: &str, default: bool) -> Result<bool> {
        let mut answer = default;
        loop {
            self.draw(Some(Popup::Confirm { prompt, answer }))?;
            let Some((code, mods)) = next_key()? else {
                continue;
            };
            match confirm_step(code, mods, &mut answer) {
                Step::Continue => {}
                Step::Accept => break,
                Step::Cancel => bail!("cancelled"),
            }
        }
        self.log(format!(
            "  ? {prompt} {}",
            if answer { "yes" } else { "no" }
        ));
        Ok(answer)
    }

    pub fn text(&mut self, prompt: &str, hidden: bool) -> Result<String> {
        let mut buf = String::new();
        loop {
            self.draw(Some(Popup::Text {
                prompt,
                buf: &buf,
                hidden,
            }))?;
            let Some((code, mods)) = next_key()? else {
                continue;
            };
            match text_step(code, mods, &mut buf) {
                Step::Continue => {}
                Step::Accept => break,
                Step::Cancel => bail!("cancelled"),
            }
        }
        let shown = if hidden {
            "••••••".to_owned()
        } else {
            buf.clone()
        };
        self.log(format!("  ? {prompt} {shown}"));
        Ok(buf)
    }

    pub fn select(&mut self, prompt: &str, items: Vec<String>) -> Result<String> {
        let picks = self.pick(prompt, &items, false, &[])?;
        picks
            .into_iter()
            .next()
            .and_then(|i| items.get(i).cloned())
            .ok_or_else(|| anyhow::anyhow!("cancelled"))
    }

    pub fn multiselect_indexed(
        &mut self,
        prompt: &str,
        items: &[String],
        default_checked: &[usize],
    ) -> Result<Vec<usize>> {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        self.pick(prompt, items, true, default_checked)
    }

    fn pick(
        &mut self,
        prompt: &str,
        items: &[String],
        multi: bool,
        initial_checked: &[usize],
    ) -> Result<Vec<usize>> {
        if items.is_empty() {
            bail!("nothing to choose from");
        }
        let mut picker = Picker::new(items.len(), multi, initial_checked);
        loop {
            self.draw(Some(Popup::Pick {
                prompt,
                items,
                picker: &picker,
            }))?;
            let Some((code, mods)) = next_key()? else {
                continue;
            };
            match picker.step(code, mods) {
                Step::Continue => {}
                Step::Accept => break,
                Step::Cancel => bail!("cancelled"),
            }
        }
        Ok(picker.confirmed())
    }

    // ---- rendering ---------------------------------------------------------

    fn draw(&mut self, popup: Option<Popup>) -> Result<()> {
        // Pull each field out so `term.draw` (mut) and the read-only panel state
        // are disjoint borrows of `self`.
        let lines = self.out.clone();
        let lines = lines.borrow();
        let status = &self.status;
        let rows = &self.rows;
        let query = &self.query;
        let sel = self.sel;
        let busy = self
            .busy
            .as_ref()
            .map(|label| (label.as_str(), SPINNER[self.spin % SPINNER.len()]));
        self.term.draw(|f| {
            draw_frame(f, status, rows, query, sel, &lines, popup.as_ref(), busy);
        })?;
        Ok(())
    }
}

impl Drop for Ui {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.term.backend_mut(), LeaveAlternateScreen);
        let _ = self.term.show_cursor();
    }
}

fn blank_status() -> Status {
    Status {
        project: String::new(),
        container: String::new(),
        state: "…",
        webapp_port: 0,
        agents: String::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_frame(
    f: &mut Frame,
    status: &Status,
    rows: &[Row],
    query: &str,
    sel: usize,
    out: &[String],
    popup: Option<&Popup>,
    busy: Option<(&str, char)>,
) {
    // A prompt (if any) is a full-width band at the bottom, above the footer —
    // it never covers the panes, and its full width keeps long prompt text on a
    // single line (so the output pane stays fully visible alongside it).
    let prompt_h = popup.map_or(0, prompt_height);
    let root = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(prompt_h),
        Constraint::Length(1),
    ])
    .split(f.area());
    let cols =
        Layout::horizontal([Constraint::Percentage(48), Constraint::Percentage(52)]).split(root[0]);
    let left = Layout::vertical([Constraint::Length(7), Constraint::Min(3)]).split(cols[0]);

    // While a task runs, the menu is disabled — dim it and drop the highlight.
    draw_status_panel(f, left[0], status);
    draw_menu_list(
        f,
        left[1],
        rows,
        query,
        sel,
        popup.is_none() && busy.is_none(),
    );
    draw_output_pane(f, cols[1], out);
    if let Some(p) = popup {
        draw_prompt(f, root[1], p);
    }
    draw_footer(f, root[2], query, busy);
}

fn prompt_height(popup: &Popup) -> u16 {
    // +1 for the top separator border line.
    1 + match popup {
        Popup::Pick { items, .. } => (items.len() as u16).min(12) + 1,
        _ => 1,
    }
}

fn draw_status_panel(f: &mut Frame, area: Rect, status: &Status) {
    let (color, glyph) = match status.state {
        "running" => (Color::Green, "●"),
        "stopped" => (Color::Yellow, "◐"),
        _ => (Color::Red, "○"),
    };
    let label = Style::default().fg(DIM);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    // The left column is narrow, so state gets its own line — otherwise a long
    // container name pushes the state word off the right edge.
    let body = vec![
        Line::from(vec![
            Span::styled(" project    ", label),
            Span::styled(&status.project, bold),
        ]),
        Line::from(vec![
            Span::styled(" container  ", label),
            Span::raw(&status.container),
        ]),
        Line::from(vec![
            Span::styled(" state      ", label),
            Span::styled(
                format!("{glyph} {}", status.state),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(" webapp     ", label),
            Span::raw(format!("port {}", status.webapp_port)),
        ]),
        Line::from(vec![
            Span::styled(" agents     ", label),
            Span::raw(if status.agents.is_empty() {
                "(none)".to_owned()
            } else {
                status.agents.clone()
            }),
        ]),
    ];
    let title = Line::from(vec![
        Span::styled(" introdus ", Style::default().fg(Color::Black).bg(ACCENT)),
        Span::styled(
            " control ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT))
        .title(title);
    f.render_widget(Paragraph::new(body).block(block), area);
}

fn draw_menu_list(f: &mut Frame, area: Rect, rows: &[Row], query: &str, sel: usize, focused: bool) {
    let visible = visible_items(rows, query);
    let selected_row = visible.get(sel).copied();
    let mut items: Vec<ListItem> = Vec::new();
    let mut selected_line = None;
    for (i, row) in rows.iter().enumerate() {
        if !query.is_empty() && !visible.contains(&i) {
            continue;
        }
        if Some(i) == selected_row {
            selected_line = Some(items.len());
        }
        items.push(match row {
            Row::Header(t) => ListItem::new(Line::from(Span::styled(
                format!("  {t}"),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ))),
            Row::Item(label) => ListItem::new(Line::from(format!("    {label}"))),
        });
    }
    let mut state = ListState::default();
    if focused {
        state.select(selected_line);
    }
    let hl = Style::default()
        .bg(ACCENT)
        .fg(Color::Black)
        .add_modifier(Modifier::BOLD);
    f.render_stateful_widget(List::new(items).highlight_style(hl), area, &mut state);
}

fn draw_output_pane(f: &mut Frame, area: Rect, out: &[String]) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(" output ", Style::default().fg(DIM)));
    let inner_w = area.width.saturating_sub(2);
    let inner_h = area.height.saturating_sub(2);
    let text: Vec<Line> = if out.is_empty() {
        vec![Line::from(Span::styled(
            "  (select an action — its output appears here)",
            Style::default().fg(DIM),
        ))]
    } else {
        out.iter().map(|l| Line::from(l.as_str())).collect()
    };
    // Pin to the bottom: scroll past the wrapped rows that overflow the pane.
    let total = wrapped_rows(out, inner_w);
    let scroll = total.saturating_sub(inner_h as usize) as u16;
    f.render_widget(
        Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        area,
    );
}

/// Approximate the number of display rows `lines` occupy when wrapped to
/// `width` columns (char-count approximation — fine for the ASCII output here).
fn wrapped_rows(lines: &[String], width: u16) -> usize {
    let w = (width.max(1)) as usize;
    lines
        .iter()
        .map(|l| {
            let n = l.chars().count();
            if n == 0 {
                1
            } else {
                n.div_ceil(w)
            }
        })
        .sum()
}

fn draw_footer(f: &mut Frame, area: Rect, query: &str, busy: Option<(&str, char)>) {
    let hint = "↑/↓ move · type to filter · Enter select · Esc quit · Ctrl-a ⟨n⟩ tmux windows";
    let line = if let Some((label, spin)) = busy {
        Line::from(vec![
            Span::styled(
                format!(" {spin} working: {label}… "),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  menu paused until it finishes", Style::default().fg(DIM)),
        ])
    } else if query.is_empty() {
        Line::from(Span::styled(hint, Style::default().fg(DIM)))
    } else {
        Line::from(vec![
            Span::styled("filter: ", Style::default().fg(DIM)),
            Span::styled(query, Style::default().fg(ACCENT)),
        ])
    };
    f.render_widget(Paragraph::new(line), area);
}

fn draw_prompt(f: &mut Frame, area: Rect, popup: &Popup) {
    // A top separator line sets the band off from the panes; the content spans
    // the full width below it (no side borders), so long prompts stay on one row.
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(" prompt ", Style::default().fg(ACCENT)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    match *popup {
        Popup::Confirm { prompt, answer } => {
            f.render_widget(Paragraph::new(confirm_line(prompt, answer)), inner);
        }
        Popup::Text {
            prompt,
            buf,
            hidden,
        } => {
            let pos = text_render(f, inner, prompt, buf, hidden);
            f.set_cursor_position(pos);
        }
        Popup::Pick {
            prompt,
            items,
            picker,
        } => {
            let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(inner);
            f.render_widget(Paragraph::new(question(prompt)), rows[0]);
            let (list, mut state) = picker.list(items);
            f.render_stateful_widget(list, rows[1], &mut state);
        }
    }
}
