//! The notification trust boundary, shared by the host listener and the
//! laptop-side TCP listener.
//!
//! The wire format a container sends is `event` or `event<TAB>label`. The event
//! must match a fixed whitelist and the label — the only attacker-influenceable
//! text that reaches a desktop notification under the "Remote dev" brand — is
//! stripped to a safe charset and length-capped. This mirrors the sanitization
//! that lived in both `host_listener.py` and `host_notify.sh`.

/// Max bytes read per event from any transport.
pub const READ_LIMIT: usize = 128;

/// Max length of a sanitized label.
pub const LABEL_MAX: usize = 40;

/// The notification brand prefix. Agent-agnostic: introdus runs whatever agent
/// you install, so the popup is branded by the harness, not by Claude Code (its
/// first and once-only supported agent).
pub const BRAND: &str = "Remote dev";

/// A validated notification event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A task finished.
    Done,
    /// The agent is awaiting input.
    Waiting,
}

impl Event {
    /// Parse the event keyword; `None` for anything off the whitelist.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "done" => Some(Event::Done),
            "waiting" => Some(Event::Waiting),
            _ => None,
        }
    }

    /// The wire keyword.
    pub fn keyword(self) -> &'static str {
        match self {
            Event::Done => "done",
            Event::Waiting => "waiting",
        }
    }

    /// The desktop-notification body.
    pub fn body(self) -> &'static str {
        match self {
            Event::Done => "Task complete",
            Event::Waiting => "Awaiting your input",
        }
    }
}

/// A parsed, validated notification: a whitelisted event plus a sanitized label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub event: Event,
    pub label: String,
}

impl Notification {
    /// Parse a raw wire line (`event` or `event<TAB>label`). Returns `None` if
    /// the line is empty or the event isn't whitelisted.
    pub fn parse(raw: &str) -> Option<Self> {
        let line = raw.trim();
        if line.is_empty() {
            return None;
        }
        let (event_str, label_str) = match line.split_once('\t') {
            Some((e, l)) => (e, l),
            None => (line, ""),
        };
        let event = Event::parse(event_str)?;
        Some(Self {
            event,
            label: sanitize_label(label_str),
        })
    }

    /// The notification title: the brand, plus the label when present
    /// (`Remote dev: <label>`).
    pub fn title(&self) -> String {
        if self.label.is_empty() {
            BRAND.to_owned()
        } else {
            format!("{BRAND}: {}", self.label)
        }
    }
}

/// Strip a label to `[A-Za-z0-9._-]` and cap it at [`LABEL_MAX`] chars.
pub fn sanitize_label(label: &str) -> String {
    label
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .take(LABEL_MAX)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta96_rejects_unknown_events() {
        assert!(Notification::parse("").is_none());
        assert!(Notification::parse("rm -rf /").is_none());
        assert!(Notification::parse("done\tweb").is_some());
    }

    #[test]
    fn ta97_sanitizes_and_caps_label() {
        assert_eq!(sanitize_label("we b;rm$"), "webrm");
        assert_eq!(sanitize_label(&"x".repeat(100)).len(), LABEL_MAX);
        // Control chars / brand-spoofing text are stripped.
        assert_eq!(sanitize_label("evil\nClaude Code"), "evilClaudeCode");
    }

    #[test]
    fn ta98_title_uses_label_when_present() {
        let n = Notification::parse("done\tmyproj").unwrap();
        assert_eq!(n.event, Event::Done);
        assert_eq!(n.title(), "Remote dev: myproj");
        let n = Notification::parse("waiting").unwrap();
        assert_eq!(n.title(), "Remote dev");
        assert_eq!(n.event.body(), "Awaiting your input");
    }
}
