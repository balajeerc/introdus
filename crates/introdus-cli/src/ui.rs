//! The whole interactive TUI, built on ratatui: a full-screen menu chooser
//! ([`menu_select`]) plus a small set of inline modal prompts ([`confirm`],
//! [`text`], [`select`], [`multiselect`]). Both the persistent control menu and
//! the one-shot setup wizard drive this module — it's the sole TUI layer (there
//! is no `inquire` anymore).
//!
//! The chooser owns the alternate screen; the modals render *inline* (a couple
//! of reserved lines at the cursor) so they interleave cleanly with the plain
//! `println!` output the callers emit around them. Inline anchoring needs the
//! terminal to answer a DSR cursor-position query; where it won't (a bare test
//! pty), the modals fall back to a fixed top-of-screen region.

use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{bail, Result};
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal, TerminalOptions, Viewport};

type Backend = CrosstermBackend<Stdout>;

// ---- palette ---------------------------------------------------------------

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;

// ---- terminal guards -------------------------------------------------------

/// Owns the alternate screen + raw mode for the full-screen chooser; restores
/// the terminal on drop no matter how we leave (return, `?`, panic-unwind).
struct FullScreen {
    terminal: Terminal<Backend>,
}

impl FullScreen {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let mut out = io::stdout();
        execute!(out, EnterAlternateScreen)?;
        let mut terminal = Terminal::new(CrosstermBackend::new(out))?;
        terminal.hide_cursor()?;
        terminal.clear()?;
        Ok(Self { terminal })
    }
}

impl Drop for FullScreen {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

/// Owns raw mode for an inline modal — a fixed-height viewport drawn at the
/// current cursor line. On drop it clears its region so the following
/// `println!` output flows from a clean line.
struct Inline {
    terminal: Terminal<Backend>,
}

/// Sticky once we learn the terminal won't report its cursor position, so only
/// the first modal pays the DSR timeout; the rest go straight to the fallback.
static INLINE_UNSUPPORTED: AtomicBool = AtomicBool::new(false);

impl Inline {
    fn enter(height: u16) -> Result<Self> {
        enable_raw_mode()?;
        Ok(Self {
            terminal: build_inline_terminal(height)?,
        })
    }
}

/// Build the modal's terminal. An inline viewport is anchored at the *current
/// cursor row*, which ratatui learns by asking the terminal (a DSR `ESC[6n`
/// query). Real terminals — xterm, tmux, an ssh pty — answer it; a bare test
/// pty doesn't, so the query times out. In that case we fall back to a fixed
/// region at the top of the screen (which needs only the ioctl size, never
/// DSR) and remember it, so later modals skip the doomed probe.
fn build_inline_terminal(height: u16) -> Result<Terminal<Backend>> {
    if !INLINE_UNSUPPORTED.load(Ordering::Relaxed) {
        match Terminal::with_options(
            CrosstermBackend::new(io::stdout()),
            TerminalOptions {
                viewport: Viewport::Inline(height),
            },
        ) {
            Ok(t) => return Ok(t),
            Err(_) => INLINE_UNSUPPORTED.store(true, Ordering::Relaxed),
        }
    }
    let (cols, rows) = crossterm_size();
    let area = Rect::new(0, 0, cols, height.min(rows));
    Ok(Terminal::with_options(
        CrosstermBackend::new(io::stdout()),
        TerminalOptions {
            viewport: Viewport::Fixed(area),
        },
    )?)
}

/// The terminal size via ioctl (never DSR), with an 80x24 fallback — including
/// when the pty reports a degenerate 0×0 (as bare test ptys do).
fn crossterm_size() -> (u16, u16) {
    match ratatui::crossterm::terminal::size() {
        Ok((c, r)) if c > 0 && r > 0 => (c, r),
        _ => (80, 24),
    }
}

impl Drop for Inline {
    fn drop(&mut self) {
        // Erase the viewport so the caller's summary line starts clean, then
        // hand the terminal back in cooked mode.
        let _ = self.terminal.clear();
        let _ = disable_raw_mode();
    }
}

/// Read the next key press, collapsing the key-repeat/release events some
/// backends emit into a single logical press. Returns `None` on a non-key event
/// (resize, focus) so the caller can just redraw.
fn next_key() -> Result<Option<(KeyCode, KeyModifiers)>> {
    match event::read()? {
        Event::Key(k) if k.kind == KeyEventKind::Press => Ok(Some((k.code, k.modifiers))),
        _ => Ok(None),
    }
}

fn is_ctrl_c(code: KeyCode, mods: KeyModifiers) -> bool {
    mods.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('c'))
}

// ============================================================================
// Full-screen menu chooser
// ============================================================================

/// The live status shown in the chooser's header panel.
pub struct Status {
    pub project: String,
    pub container: String,
    /// Human word for the container state: `running` / `stopped` / `not created`.
    pub state: &'static str,
    pub webapp_port: u16,
    pub agents: String,
}

/// A row in the menu: an inert section header or a selectable item.
pub enum Row {
    Header(String),
    Item(String),
}

impl Row {
    fn label(&self) -> &str {
        match self {
            Row::Header(s) | Row::Item(s) => s,
        }
    }
    fn is_item(&self) -> bool {
        matches!(self, Row::Item(_))
    }
}

/// Show the full-screen control menu and block until the user picks an item or
/// quits. Returns `Some(index)` — the index into `rows` of the chosen
/// [`Row::Item`] — or `None` if the user quit (Esc / Ctrl-C / the Quit item is
/// the caller's own row, handled by index).
///
/// Typing filters the item list by case-insensitive substring (so the test
/// harness can drive it by sending the label text); Up/Down move between items,
/// Enter selects, Backspace edits the filter, Esc quits.
pub fn menu_select(status: &Status, rows: &[Row]) -> Result<Option<usize>> {
    let mut screen = FullScreen::enter()?;
    let mut query = String::new();
    // `sel` indexes into the currently-visible item list, not `rows`.
    let mut sel: usize = 0;

    let outcome = loop {
        let visible = visible_items(rows, &query);
        if sel >= visible.len() {
            sel = visible.len().saturating_sub(1);
        }
        screen
            .terminal
            .draw(|f| draw_menu(f, status, rows, &query, &visible, sel))?;

        let Some((code, mods)) = next_key()? else {
            continue;
        };
        if is_ctrl_c(code, mods) {
            break None;
        }
        match code {
            KeyCode::Esc => break None,
            KeyCode::Enter => {
                if let Some(&idx) = visible.get(sel) {
                    break Some(idx);
                }
            }
            KeyCode::Up => sel = sel.saturating_sub(1),
            KeyCode::Down if sel + 1 < visible.len() => sel += 1,
            KeyCode::Backspace => {
                query.pop();
                sel = 0;
            }
            KeyCode::Char(c) => {
                query.push(c);
                sel = 0;
            }
            _ => {}
        }
    };
    Ok(outcome)
}

/// Indices into `rows` of the items visible under the current filter. With an
/// empty query every item shows; otherwise only items whose label contains the
/// query (case-insensitive).
fn visible_items(rows: &[Row], query: &str) -> Vec<usize> {
    let q = query.to_lowercase();
    rows.iter()
        .enumerate()
        .filter(|(_, r)| r.is_item() && (q.is_empty() || r.label().to_lowercase().contains(&q)))
        .map(|(i, _)| i)
        .collect()
}

fn draw_menu(
    f: &mut Frame,
    status: &Status,
    rows: &[Row],
    query: &str,
    visible: &[usize],
    sel: usize,
) {
    let chunks = Layout::vertical([
        Constraint::Length(7), // status panel
        Constraint::Min(3),    // menu list
        Constraint::Length(1), // footer
    ])
    .split(f.area());

    draw_status_panel(f, chunks[0], status);
    draw_menu_list(f, chunks[1], rows, query, visible, sel);
    draw_footer(f, chunks[2], query);
}

fn draw_status_panel(f: &mut Frame, area: Rect, status: &Status) {
    let (state_color, state_glyph) = match status.state {
        "running" => (Color::Green, "●"),
        "stopped" => (Color::Yellow, "◐"),
        _ => (Color::Red, "○"),
    };
    let label = Style::default().fg(DIM);
    let rows = vec![
        Line::from(vec![
            Span::styled(" project    ", label),
            Span::styled(
                &status.project,
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(" container  ", label),
            Span::raw(&status.container),
            Span::raw("  "),
            Span::styled(
                format!("{state_glyph} {}", status.state),
                Style::default()
                    .fg(state_color)
                    .add_modifier(Modifier::BOLD),
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
    f.render_widget(Paragraph::new(rows).block(block), area);
}

fn draw_menu_list(
    f: &mut Frame,
    area: Rect,
    rows: &[Row],
    query: &str,
    visible: &[usize],
    sel: usize,
) {
    let selected_row = visible.get(sel).copied();
    let mut items: Vec<ListItem> = Vec::new();
    let mut selected_line: Option<usize> = None;
    for (i, row) in rows.iter().enumerate() {
        // While filtering, drop headers and non-matching items entirely.
        if !query.is_empty() && !visible.contains(&i) {
            continue;
        }
        if Some(i) == selected_row {
            selected_line = Some(items.len());
        }
        items.push(match row {
            Row::Header(title) => ListItem::new(Line::from(Span::styled(
                format!("  {title}"),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ))),
            Row::Item(label) => ListItem::new(Line::from(format!("    {label}"))),
        });
    }

    let mut state = ListState::default();
    state.select(selected_line);
    let list = List::new(items).highlight_style(
        Style::default()
            .bg(ACCENT)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD),
    );
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_footer(f: &mut Frame, area: Rect, query: &str) {
    let hint = "↑/↓ move · type to filter · Enter select · Esc quit · Ctrl-a ⟨n⟩ tmux windows";
    let line = if query.is_empty() {
        Line::from(Span::styled(hint, Style::default().fg(DIM)))
    } else {
        Line::from(vec![
            Span::styled("filter: ", Style::default().fg(DIM)),
            Span::styled(query, Style::default().fg(ACCENT)),
        ])
    };
    f.render_widget(Paragraph::new(line), area);
}

// ============================================================================
// Inline modal prompts (used by the menu actions and the setup wizard)
// ============================================================================

/// A styled question glyph + text, used as the leading line of every modal and
/// the resolved summary printed afterwards.
fn question(prompt: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            "? ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            prompt.to_owned(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ])
}

/// Yes/no confirmation. `y`/`n` set the answer, Enter accepts the current one
/// (starting from `default`), Esc/Ctrl-C cancels.
pub fn confirm(prompt: &str, default: bool) -> Result<bool> {
    let mut answer = default;
    let mut cancelled = false;
    {
        let mut ui = Inline::enter(1)?;
        loop {
            ui.terminal.draw(|f| {
                let hint = if answer { "(Y/n)" } else { "(y/N)" };
                let mut spans = question(prompt).spans;
                spans.push(Span::raw(" "));
                spans.push(Span::styled(hint, Style::default().fg(DIM)));
                f.render_widget(Paragraph::new(Line::from(spans)), f.area());
            })?;
            let Some((code, mods)) = next_key()? else {
                continue;
            };
            if is_ctrl_c(code, mods) {
                cancelled = true;
                break;
            }
            match code {
                KeyCode::Char('y') | KeyCode::Char('Y') => answer = true,
                KeyCode::Char('n') | KeyCode::Char('N') => answer = false,
                KeyCode::Enter => break,
                KeyCode::Esc => {
                    cancelled = true;
                    break;
                }
                _ => {}
            }
        }
    }
    if cancelled {
        bail!("cancelled");
    }
    println!("  ? {prompt} {}", if answer { "yes" } else { "no" });
    Ok(answer)
}

/// Free-text entry. `default` pre-fills the buffer; `hidden` masks it (for
/// secrets). Enter accepts, Esc/Ctrl-C cancels.
pub fn text(prompt: &str, default: Option<&str>, hidden: bool) -> Result<String> {
    let mut buf = default.unwrap_or("").to_owned();
    let mut cancelled = false;
    {
        let mut ui = Inline::enter(1)?;
        loop {
            ui.terminal
                .draw(|f| draw_text_line(f, prompt, &buf, hidden))?;
            let Some((code, mods)) = next_key()? else {
                continue;
            };
            if is_ctrl_c(code, mods) {
                cancelled = true;
                break;
            }
            match code {
                KeyCode::Enter => break,
                KeyCode::Esc => {
                    cancelled = true;
                    break;
                }
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Char(c) => buf.push(c),
                _ => {}
            }
        }
    }
    if cancelled {
        bail!("cancelled");
    }
    let shown = if hidden {
        "••••••".to_owned()
    } else {
        buf.clone()
    };
    println!("  ? {prompt} {shown}");
    Ok(buf)
}

fn draw_text_line(f: &mut Frame, prompt: &str, buf: &str, hidden: bool) {
    let shown = if hidden {
        "•".repeat(buf.chars().count())
    } else {
        buf.to_owned()
    };
    let mut spans = question(prompt).spans;
    spans.push(Span::raw(" "));
    let prefix_cols: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    spans.push(Span::styled(shown.clone(), Style::default().fg(ACCENT)));
    f.render_widget(Paragraph::new(Line::from(spans)), f.area());
    let x = (prefix_cols + shown.chars().count()) as u16;
    f.set_cursor_position((x.min(f.area().width.saturating_sub(1)), f.area().y));
}

/// Single-choice picker. Up/Down move, Enter selects. Returns the chosen
/// string, or errors if `items` is empty / the user cancels.
pub fn select(prompt: &str, items: Vec<String>) -> Result<String> {
    if items.is_empty() {
        bail!("nothing to choose from");
    }
    let picks = run_picker(prompt, &items, false, &[])?;
    picks
        .into_iter()
        .next()
        .and_then(|i| items.get(i).cloned())
        .ok_or_else(|| anyhow::anyhow!("cancelled"))
}

/// Multi-choice picker. Space toggles an item, Enter confirms. Returns the
/// chosen strings (possibly empty).
pub fn multiselect(prompt: &str, items: Vec<String>) -> Result<Vec<String>> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let picks = run_picker(prompt, &items, true, &[])?;
    Ok(picks
        .into_iter()
        .filter_map(|i| items.get(i).cloned())
        .collect())
}

/// Multi-choice picker returning the selected *indices* into `items`, with
/// `default_checked` pre-toggled on entry. Used where the caller needs to map
/// the picks back to a parallel array (e.g. the agent registry).
pub fn multiselect_indexed(
    prompt: &str,
    items: &[String],
    default_checked: &[usize],
) -> Result<Vec<usize>> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    run_picker(prompt, items, true, default_checked)
}

/// Shared list-picker loop for [`select`] and the multi-select entry points.
/// Returns the selected indices into `items` (one for single-select, N for
/// multi). `initial_checked` seeds the toggles for multi-select.
fn run_picker(
    prompt: &str,
    items: &[String],
    multi: bool,
    initial_checked: &[usize],
) -> Result<Vec<usize>> {
    let height = (items.len() as u16 + 2).min(14);
    let mut cursor = 0usize;
    let mut checked = vec![false; items.len()];
    for &i in initial_checked {
        if let Some(slot) = checked.get_mut(i) {
            *slot = true;
        }
    }
    let mut cancelled = false;
    let mut confirmed: Vec<usize> = Vec::new();
    {
        let mut ui = Inline::enter(height)?;
        loop {
            ui.terminal
                .draw(|f| draw_picker(f, prompt, items, cursor, &checked, multi))?;
            let Some((code, mods)) = next_key()? else {
                continue;
            };
            if is_ctrl_c(code, mods) {
                cancelled = true;
                break;
            }
            match code {
                KeyCode::Esc => {
                    cancelled = true;
                    break;
                }
                KeyCode::Up => cursor = cursor.saturating_sub(1),
                KeyCode::Down if cursor + 1 < items.len() => cursor += 1,
                KeyCode::Char(' ') if multi => checked[cursor] = !checked[cursor],
                KeyCode::Enter => {
                    if multi {
                        confirmed = checked
                            .iter()
                            .enumerate()
                            .filter(|(_, &c)| c)
                            .map(|(i, _)| i)
                            .collect();
                    } else {
                        confirmed = vec![cursor];
                    }
                    break;
                }
                _ => {}
            }
        }
    }
    if cancelled {
        bail!("cancelled");
    }
    let summary: Vec<&str> = confirmed
        .iter()
        .filter_map(|&i| items.get(i).map(String::as_str))
        .collect();
    println!("  ? {prompt} {}", summary.join(", "));
    Ok(confirmed)
}

fn draw_picker(
    f: &mut Frame,
    prompt: &str,
    items: &[String],
    cursor: usize,
    checked: &[bool],
    multi: bool,
) {
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(f.area());
    f.render_widget(Paragraph::new(question(prompt)), chunks[0]);

    let list: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let mark = if multi {
                if checked[i] {
                    "[x] "
                } else {
                    "[ ] "
                }
            } else {
                ""
            };
            ListItem::new(Line::from(format!("  {mark}{label}")))
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(cursor));
    let widget = List::new(list).highlight_style(
        Style::default()
            .bg(ACCENT)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD),
    );
    f.render_stateful_widget(widget, chunks[1], &mut state);
}

// ---- a plain, non-ratatui pause --------------------------------------------

/// Wait for Enter before the caller redraws the menu. Deliberately plain
/// (cooked-mode `stdin`) so it composes with the action's `println!` output and
/// the test harness can drive it with a bare Enter.
pub fn pause() {
    use std::io::Write;
    print!("\n  (press Enter to continue) ");
    let _ = io::stdout().flush();
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta81_filter_matches_labels_case_insensitively() {
        let rows = vec![
            Row::Header("Container lifecycle".into()),
            Row::Item("Restart the container".into()),
            Row::Item("Stop the container".into()),
            Row::Item("Refresh status".into()),
        ];
        // Header is never selectable.
        assert_eq!(visible_items(&rows, ""), vec![1, 2, 3]);
        // Substring, case-insensitive; the harness sends label fragments.
        assert_eq!(visible_items(&rows, "refresh"), vec![3]);
        assert_eq!(visible_items(&rows, "the container"), vec![1, 2]);
        assert!(visible_items(&rows, "nope").is_empty());
    }
}
