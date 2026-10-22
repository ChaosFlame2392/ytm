use crossterm::cursor::{Hide, Show};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::io::{self, Write};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Formats playback position or duration in seconds to `MM:SS` format.
/// Returns `"--:--"` if the value is None, NaN, or negative.
pub fn format_time(seconds: Option<f64>) -> String {
    match seconds {
        Some(s) if s >= 0.0 && !s.is_nan() => {
            let total_secs = s as u64;
            let minutes = total_secs / 60;
            let secs = total_secs % 60;
            format!("{:02}:{:02}", minutes, secs)
        }
        _ => "--:--".to_string(),
    }
}

/// Formats the first line of the in-place status bar:
/// `[▶]` or `[⏸]`, track title, position / duration formatted as `MM:SS`, and `[⚡ Cached]` vs `[🌐 Stream]`.
pub fn format_line1(
    paused: bool,
    title: &str,
    pos: Option<f64>,
    dur: Option<f64>,
    is_cached: bool,
) -> String {
    let icon = if paused { "[⏸]" } else { "[▶]" };
    let time_str = format!("({} / {})", format_time(pos), format_time(dur));
    let badge = if is_cached {
        "[⚡ Cached]"
    } else {
        "[🌐 Stream]"
    };
    format!("{} {} {}  {}", icon, title, time_str, badge)
}

/// Formats the second line of the in-place status bar:
/// `[Shuffle: ON/OFF]`, `[Loop: ON/OFF]`, and `Next: <next title>`.
pub fn format_line2(shuffle: bool, single_loop: bool, next_title: Option<&str>) -> String {
    let shuffle_badge = if shuffle {
        "[Shuffle: ON]"
    } else {
        "[Shuffle: OFF]"
    };
    let loop_badge = if single_loop {
        "[Loop: ON]"
    } else {
        "[Loop: OFF]"
    };
    let next_str = format!("Next: {}", next_title.unwrap_or("None"));
    format!("{} {}  {}", shuffle_badge, loop_badge, next_str)
}

/// Truncates string to a maximum character width to prevent terminal wrapping.
/// If truncated, appends '…' while keeping total character length <= max_width.
pub fn truncate_to_width(s: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if s.chars().count() > max_width {
        let truncated: String = s.chars().take(max_width.saturating_sub(1)).collect();
        format!("{}…", truncated)
    } else {
        s.to_string()
    }
}

/// Represents the instantaneous playback status displayed in the terminal UI.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusState<'a> {
    pub is_paused: bool,
    pub title: &'a str,
    pub position: Option<f64>,
    pub duration: Option<f64>,
    pub is_cached: bool,
    pub shuffle: bool,
    pub single_loop: bool,
    pub next_title: Option<&'a str>,
}

impl<'a> StatusState<'a> {
    pub fn format_lines(&self) -> (String, String) {
        let line1 = format_line1(
            self.is_paused,
            self.title,
            self.position,
            self.duration,
            self.is_cached,
        );
        let line2 = format_line2(self.shuffle, self.single_loop, self.next_title);
        (line1, line2)
    }
}

/// Renders 2 lines strictly in place (`\r\x1b[2K`), overwriting without scrolling the terminal.
pub fn render_status_bar<W: Write>(writer: &mut W, line1: &str, line2: &str) -> io::Result<()> {
    write!(writer, "\r\x1b[2K{}\n\r\x1b[2K{}\x1b[1A\r", line1, line2)?;
    writer.flush()
}

/// Erases both status lines in-place and leaves the cursor at line 1 column 0.
pub fn wipe_status_lines<W: Write>(writer: &mut W) -> io::Result<()> {
    write!(writer, "\r\x1b[2K\n\r\x1b[2K\x1b[1A\r")?;
    writer.flush()
}

/// Actions triggered by terminal keybindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    TogglePause,
    NextTrack,
    PreviousTrack,
    ToggleShuffle,
    ResetToTop,
    ToggleLoop,
    Quit,
}

/// Parses a crossterm KeyEvent into a semantic player action.
pub fn parse_key_event(event: KeyEvent) -> Option<KeyAction> {
    if event.kind == KeyEventKind::Release {
        return None;
    }

    if event.modifiers.contains(KeyModifiers::CONTROL) {
        match event.code {
            KeyCode::Char('c') | KeyCode::Char('C') => return Some(KeyAction::Quit),
            _ => {}
        }
    }

    match event.code {
        KeyCode::Char(' ') => Some(KeyAction::TogglePause),
        KeyCode::Char('>') | KeyCode::Char('.') | KeyCode::Right => Some(KeyAction::NextTrack),
        KeyCode::Char('<') | KeyCode::Char(',') | KeyCode::Left => Some(KeyAction::PreviousTrack),
        KeyCode::Char('s') | KeyCode::Char('S') => Some(KeyAction::ToggleShuffle),
        KeyCode::Char('r') | KeyCode::Char('R') => Some(KeyAction::ResetToTop),
        KeyCode::Char('l') | KeyCode::Char('L') => Some(KeyAction::ToggleLoop),
        KeyCode::Char('q') | KeyCode::Char('Q') => Some(KeyAction::Quit),
        _ => None,
    }
}

/// RAII Terminal cleanup guard. Ensures raw mode is disabled and cursor is restored
/// upon return, panic, or exit signal.
pub struct TerminalGuard {
    active: bool,
}

impl TerminalGuard {
    /// Enables raw mode, hides cursor, and installs a panic hook to guarantee terminal cleanup.
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(e) = execute!(stdout, Hide) {
            let _ = disable_raw_mode();
            return Err(e);
        }

        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let mut out = io::stdout();
            let _ = wipe_status_lines(&mut out);
            let _ = execute!(out, Show);
            let _ = disable_raw_mode();
            default_hook(info);
        }));

        Ok(Self { active: true })
    }

    /// Explicitly restores terminal state and wipes status lines.
    pub fn leave(&mut self) {
        if self.active {
            self.active = false;
            let mut stdout = io::stdout();
            let _ = wipe_status_lines(&mut stdout);
            let _ = execute!(stdout, Show);
            let _ = disable_raw_mode();
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.leave();
    }
}

/// Registers SIGINT and SIGTERM handlers to set the returned AtomicBool flag to true.
pub fn setup_signal_handler() -> io::Result<Arc<AtomicBool>> {
    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term))?;
    Ok(term)
}
