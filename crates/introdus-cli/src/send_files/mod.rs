//! `introdus send-files` — a dev-machine tool to push files/folders into a
//! running introdus container, local or on an ssh-reachable host.
//!
//! Three stages, each a full-screen ratatui view over one owned alternate
//! screen ([`App`]): pick a host (this machine or a `~/.ssh/config` alias),
//! pick one of its running introdus containers, then a dual-pane file browser
//! ([`browser`]) to send. Esc steps back a stage (and quits from the first).
//!
//! The transport is `podman cp` for local and a tar-stream over ssh for remote
//! (see [`transfer`]); nothing here needs the project's `.introdus/config.env`
//! or a shared mount — a running container is enough.

mod browser;
mod transfer;

use std::io::{self, Stdout};
use std::time::Duration;

use anyhow::{anyhow, Result};
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, KeyCode};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use introdus_core::containers::{parse_ps, Container, PS_FORMAT};
use introdus_core::remote::Location;
use introdus_core::sshconfig;

use crate::ui::{is_ctrl_c, next_key, ACCENT, DIM};

type Backend = CrosstermBackend<Stdout>;

/// Braille spinner frames, matching the control panel's.
const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Entry point for the `send-files` subcommand.
pub fn run() -> Result<()> {
    let mut app = App::new()?;
    let aliases = sshconfig::read_host_aliases();
    let mut host_labels = vec!["this machine (local)".to_owned()];
    host_labels.extend(aliases.iter().map(|a| format!("{a}  (ssh)")));

    let mut stage = Stage::Host;
    loop {
        stage = match stage {
            Stage::Host => match app.select(
                "Send files — pick a host",
                "where the target container runs",
                &host_labels,
            )? {
                None => return Ok(()),
                Some(0) => Stage::Container(Location::Local),
                Some(i) => Stage::Container(Location::Remote(aliases[i - 1].clone())),
            },
            Stage::Container(loc) => app.container_stage(loc)?,
            Stage::Browse(loc, container) => {
                browser::browse(&mut app, &loc, &container)?;
                Stage::Container(loc)
            }
        };
    }
}

/// Which of the three views is active.
enum Stage {
    Host,
    Container(Location),
    Browse(Location, String),
}

/// Owns the alternate screen for a `send-files` session; restored on drop.
struct App {
    term: Terminal<Backend>,
}

impl App {
    fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let mut term = Terminal::new(CrosstermBackend::new(stdout))?;
        term.hide_cursor()?;
        Ok(Self { term })
    }

    /// Pick a container on `loc`, or step back to the host picker. Listing runs
    /// behind a spinner (an ssh round-trip can be slow); an unreachable host or
    /// an empty list shows an info screen and returns to [`Stage::Host`].
    fn container_stage(&mut self, loc: Location) -> Result<Stage> {
        let probe = loc.clone();
        let listed = self.spinner(&format!("querying {}…", loc.label()), move || {
            list_containers(&probe)
        })?;
        let containers = match listed {
            Ok(cs) => cs,
            Err(e) => {
                self.info("Couldn't list containers", &[e.to_string()])?;
                return Ok(Stage::Host);
            }
        };
        if containers.is_empty() {
            self.info(
                "No introdus containers",
                &[format!("Nothing running on {}.", loc.label())],
            )?;
            return Ok(Stage::Host);
        }
        let labels: Vec<String> = containers
            .iter()
            .map(|c| format!("{}   ({})", c.name, c.state))
            .collect();
        match self.select(
            &format!("Pick a container on {}", loc.label()),
            "its filesystem is the browser's right pane",
            &labels,
        )? {
            None => Ok(Stage::Host),
            Some(i) => Ok(Stage::Browse(loc, containers[i].name.clone())),
        }
    }

    /// A single-choice list view. Returns the chosen index, or `None` on
    /// Esc/`q`/Ctrl-C (the caller's "go back").
    fn select(&mut self, title: &str, subtitle: &str, items: &[String]) -> Result<Option<usize>> {
        let mut cursor = 0usize;
        loop {
            self.term
                .draw(|f| draw_list(f, title, subtitle, items, cursor))?;
            let Some((code, mods)) = next_key()? else {
                continue;
            };
            if is_ctrl_c(code, mods) {
                return Ok(None);
            }
            match code {
                KeyCode::Esc | KeyCode::Char('q') => return Ok(None),
                KeyCode::Up => cursor = cursor.saturating_sub(1),
                KeyCode::Down if cursor + 1 < items.len() => cursor += 1,
                KeyCode::Enter if !items.is_empty() => return Ok(Some(cursor)),
                _ => {}
            }
        }
    }

    /// A centered message view; any key dismisses it.
    fn info(&mut self, title: &str, body: &[String]) -> Result<()> {
        loop {
            self.term.draw(|f| draw_center(f, title, body))?;
            if next_key()?.is_some() {
                return Ok(());
            }
        }
    }

    /// Run `f` on a worker thread while animating a centered spinner, so a long
    /// transfer or ssh round-trip doesn't freeze the UI. Keystrokes are
    /// discarded meanwhile. `Err` only on a thread panic — `f`'s own result is
    /// returned as `T`.
    fn spinner<T, F>(&mut self, label: &str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let handle = std::thread::spawn(f);
        let mut spin = 0usize;
        loop {
            let frame = SPINNER[spin % SPINNER.len()];
            self.term
                .draw(|f| draw_center(f, &format!("{frame}  working"), &[label.to_owned()]))?;
            spin = spin.wrapping_add(1);
            if handle.is_finished() {
                return handle
                    .join()
                    .map_err(|_| anyhow!("`{label}` task panicked"));
            }
            if event::poll(Duration::from_millis(120))? {
                let _ = event::read()?; // the task owns the UI; drop input
            }
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.term.backend_mut(), LeaveAlternateScreen);
        let _ = self.term.show_cursor();
    }
}

/// List the running introdus containers at `loc` (blocking; run via a spinner).
fn list_containers(loc: &Location) -> Result<Vec<Container>> {
    let out = loc
        .podman(&["ps", "--format", PS_FORMAT])
        .stdout_quiet()
        .map_err(|e| anyhow!("{}: {e}", loc.label()))?;
    Ok(parse_ps(&out))
}

// ---- rendering --------------------------------------------------------------

fn draw_list(f: &mut Frame, title: &str, subtitle: &str, items: &[String], cursor: usize) {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(f.area());

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            title.to_owned(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))),
        rows[0],
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            subtitle.to_owned(),
            Style::default().fg(DIM),
        ))),
        rows[1],
    );

    let list_items: Vec<ListItem> = items
        .iter()
        .map(|s| ListItem::new(Line::from(format!("  {s}"))))
        .collect();
    let hl = Style::default()
        .bg(ACCENT)
        .fg(ratatui::style::Color::Black)
        .add_modifier(Modifier::BOLD);
    let mut state = ListState::default();
    state.select((!items.is_empty()).then_some(cursor));
    f.render_stateful_widget(
        List::new(list_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(DIM)),
            )
            .highlight_style(hl),
        rows[2],
        &mut state,
    );

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "↑/↓ move · Enter select · Esc back",
            Style::default().fg(DIM),
        ))),
        rows[3],
    );
}

fn draw_center(f: &mut Frame, title: &str, body: &[String]) {
    let area = f.area();
    let mut lines = vec![Line::from(Span::styled(
        title.to_owned(),
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ))];
    lines.push(Line::from(""));
    lines.extend(body.iter().map(|l| Line::from(l.as_str())));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "(press any key)",
        Style::default().fg(DIM),
    )));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT));
    // A simple full-area panel — good enough for a picker interstitial.
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}
