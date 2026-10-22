use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::style::Print;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
};
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

/// Formats the first line of the in-place status bar from discrete arguments.
/// Kept as a convenient helper for tests.
pub fn format_line1(
    paused: bool,
    title: &str,
    pos: Option<f64>,
    dur: Option<f64>,
    is_cached: bool,
) -> String {
    StatusState {
        is_paused: paused,
        title,
        position: pos,
        duration: dur,
        is_cached,
        shuffle: false,
        single_loop: false,
        next_title: None,
    }
    .format_line1()
}

/// Formats the second line of the in-place status bar from discrete arguments.
/// Kept as a convenient helper for tests.
pub fn format_line2(shuffle: bool, single_loop: bool, next_title: Option<&str>) -> String {
    StatusState {
        is_paused: false,
        title: "",
        position: None,
        duration: None,
        is_cached: false,
        shuffle,
        single_loop,
        next_title,
    }
    .format_line2()
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
    /// Formats the first line of the in-place status bar:
    /// `[▶]` or `[⏸]`, track title, position / duration formatted as `MM:SS`, and `[⚡ Cached]` vs `[🌐 Stream]`.
    pub fn format_line1(&self) -> String {
        let icon = if self.is_paused { "[⏸]" } else { "[▶]" };
        let time_str = format!("({} / {})", format_time(self.position), format_time(self.duration));
        let badge = if self.is_cached {
            "[⚡ Cached]"
        } else {
            "[🌐 Stream]"
        };
        format!("{} {} {}  {}", icon, self.title, time_str, badge)
    }

    /// Formats the second line of the in-place status bar:
    /// `[Shuffle: ON/OFF]`, `[Loop: ON/OFF]`, and `Next: <next title>`.
    pub fn format_line2(&self) -> String {
        let shuffle_badge = if self.shuffle {
            "[Shuffle: ON]"
        } else {
            "[Shuffle: OFF]"
        };
        let loop_badge = if self.single_loop {
            "[Loop: ON]"
        } else {
            "[Loop: OFF]"
        };
        let next_str = format!("Next: {}", self.next_title.unwrap_or("None"));
        format!("{} {}  {}", shuffle_badge, loop_badge, next_str)
    }

    /// Formats both status bar lines as a pair (line1, line2).
    pub fn format_lines(&self) -> (String, String) {
        (self.format_line1(), self.format_line2())
    }
}

/// Renders 2 lines strictly in place (`\r\x1b[2K`), overwriting without scrolling the terminal.
/// Accepts `&StatusState` directly, truncates to terminal width if available, and flushes output.
pub fn render_status_bar<W: Write>(writer: &mut W, state: &StatusState<'_>) -> io::Result<()> {
    let (mut l1, mut l2) = state.format_lines();
    if let Ok((cols, _)) = crossterm::terminal::size() {
        let width = cols as usize;
        if width > 0 {
            l1 = truncate_to_width(&l1, width);
            l2 = truncate_to_width(&l2, width);
        }
    }
    render_status_lines(writer, &l1, &l2)
}

/// Helper function to write raw status lines in-place without scrolling.
pub fn render_status_lines<W: Write>(writer: &mut W, line1: &str, line2: &str) -> io::Result<()> {
    write!(writer, "\r\x1b[2K{}\n\r\x1b[2K{}\x1b[1A\r", line1, line2)?;
    writer.flush()
}

/// Erases both status lines in-place and leaves the cursor at line 1 column 0.
pub fn wipe_status_lines<W: Write>(writer: &mut W) -> io::Result<()> {
    write!(writer, "\r\x1b[2K\n\r\x1b[2K\x1b[1A\r")?;
    writer.flush()
}

/// Actions triggered by terminal keybindings in normal playback mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    TogglePause,
    NextTrack,
    PreviousTrack,
    SeekForward,
    SeekBackward,
    ToggleShuffle,
    ResetToTop,
    ToggleLoop,
    OpenModal,
    Quit,
}

/// Actions triggered by keybindings inside the floating queue modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalKeyAction {
    CursorDown,
    CursorUp,
    ToggleSelection,
    ClearSelection,
    Commit,
    Close,
    Quit,
}

/// Parses a crossterm KeyEvent into a semantic player action in normal mode.
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
        KeyCode::Tab => Some(KeyAction::OpenModal),
        KeyCode::Char(' ') => Some(KeyAction::TogglePause),
        KeyCode::Char('>') | KeyCode::Char('.') => Some(KeyAction::NextTrack),
        KeyCode::Char('<') | KeyCode::Char(',') => Some(KeyAction::PreviousTrack),
        KeyCode::Char('h') | KeyCode::Char('H') | KeyCode::Left => Some(KeyAction::SeekBackward),
        KeyCode::Char('l') | KeyCode::Char('L') | KeyCode::Right => Some(KeyAction::SeekForward),
        KeyCode::Char('s') | KeyCode::Char('S') => Some(KeyAction::ToggleShuffle),
        KeyCode::Char('r') | KeyCode::Char('R') => Some(KeyAction::ResetToTop),
        KeyCode::Char('q') | KeyCode::Char('Q') => Some(KeyAction::Quit),
        _ => None,
    }
}

/// Parses a crossterm KeyEvent into a semantic modal action when the queue modal is open.
pub fn parse_modal_key_event(event: KeyEvent) -> Option<ModalKeyAction> {
    if event.kind == KeyEventKind::Release {
        return None;
    }

    if event.modifiers.contains(KeyModifiers::CONTROL) {
        match event.code {
            KeyCode::Char('c') | KeyCode::Char('C') => return Some(ModalKeyAction::Quit),
            _ => {}
        }
    }

    match event.code {
        KeyCode::Char('j') | KeyCode::Char('J') | KeyCode::Down => Some(ModalKeyAction::CursorDown),
        KeyCode::Char('k') | KeyCode::Char('K') | KeyCode::Up => Some(ModalKeyAction::CursorUp),
        KeyCode::Char(' ') => Some(ModalKeyAction::ToggleSelection),
        KeyCode::Char('c') | KeyCode::Char('C') => Some(ModalKeyAction::ClearSelection),
        KeyCode::Enter => Some(ModalKeyAction::Commit),
        KeyCode::Tab | KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            Some(ModalKeyAction::Close)
        }
        _ => None,
    }
}


/// Clears the terminal screen, repositions cursor to top-left (0, 0), and hides cursor.
pub fn init_screen<W: Write>(writer: &mut W) -> io::Result<()> {
    execute!(writer, Clear(ClearType::All), MoveTo(0, 0), Hide)
}

/// RAII Terminal cleanup guard. Ensures raw mode is disabled and cursor is restored
/// upon return, panic, or exit signal.
pub struct TerminalGuard {
    active: bool,
}

impl TerminalGuard {
    /// Enables raw mode, clears screen, moves cursor to top-left, hides cursor,
    /// and installs a panic hook to guarantee terminal cleanup.
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(e) = init_screen(&mut stdout) {
            let _ = disable_raw_mode();
            return Err(e);
        }

        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let mut out = io::stdout();
            let _ = execute!(out, LeaveAlternateScreen);
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
            let _ = execute!(stdout, LeaveAlternateScreen);
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

/// Rectangular dimensions and position for rendering centered modal boxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModalRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl ModalRect {
    pub fn content_height(&self) -> usize {
        self.height.saturating_sub(2) as usize
    }

    pub fn content_width(&self) -> usize {
        self.width.saturating_sub(2) as usize
    }
}

/// Computes centered modal dimensions scaling to ~80% width and ~70% height.
pub fn calculate_modal_rect(cols: u16, rows: u16) -> ModalRect {
    let safe_cols = if cols == 0 { 80 } else { cols };
    let safe_rows = if rows == 0 { 24 } else { rows };

    let width = ((safe_cols as u32 * 80) / 100) as u16;
    let height = ((safe_rows as u32 * 70) / 100) as u16;

    let width = width.max(20.min(safe_cols)).min(safe_cols);
    let height = height.max(5.min(safe_rows)).min(safe_rows);

    let x = safe_cols.saturating_sub(width) / 2;
    let y = safe_rows.saturating_sub(height) / 2;

    ModalRect {
        x,
        y,
        width,
        height,
    }
}

/// State tracking for the floating playlist viewer modal.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModalState {
    pub is_open: bool,
    pub viewer_cursor: usize,
    pub viewport_offset: usize,
    pub selection_set: Vec<usize>,
}

impl ModalState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the modal anchored and centered on the active track index.
    pub fn open(&mut self, active_cursor: usize, total_items: usize, content_height: usize) {
        self.is_open = true;
        self.selection_set.clear();
        self.viewer_cursor = if total_items == 0 {
            0
        } else {
            active_cursor.min(total_items - 1)
        };
        let half = content_height / 2;
        self.viewport_offset = self.viewer_cursor.saturating_sub(half);
        self.clamp_viewport(total_items, content_height);
    }

    /// Closes the modal.
    pub fn close(&mut self) {
        self.is_open = false;
        self.selection_set.clear();
    }

    /// Toggles the selection of the item at `index`, preserving pick order.
    pub fn toggle_selection(&mut self, index: usize) {
        if let Some(pos) = self.selection_set.iter().position(|&i| i == index) {
            self.selection_set.remove(pos);
        } else {
            self.selection_set.push(index);
        }
    }

    /// Returns true if the item at `index` is in the selection set.
    pub fn is_selected(&self, index: usize) -> bool {
        self.selection_set.contains(&index)
    }

    /// Empties the selection set.
    pub fn clear_selection(&mut self) {
        self.selection_set.clear();
    }

    /// Moves the viewer cursor down by one row and updates viewport scrolling.
    pub fn move_cursor_down(&mut self, total_items: usize, content_height: usize) {
        if total_items == 0 {
            return;
        }
        if self.viewer_cursor + 1 < total_items {
            self.viewer_cursor += 1;
            self.adjust_viewport_for_cursor(content_height);
        }
    }

    /// Moves the viewer cursor up by one row and updates viewport scrolling.
    pub fn move_cursor_up(&mut self, content_height: usize) {
        if self.viewer_cursor > 0 {
            self.viewer_cursor -= 1;
            self.adjust_viewport_for_cursor(content_height);
        }
    }

    /// Adjusts viewport offset so that `viewer_cursor` remains within the visible area.
    pub fn adjust_viewport_for_cursor(&mut self, content_height: usize) {
        if content_height == 0 {
            return;
        }
        if self.viewer_cursor < self.viewport_offset {
            self.viewport_offset = self.viewer_cursor;
        } else if self.viewer_cursor >= self.viewport_offset + content_height {
            self.viewport_offset = self.viewer_cursor + 1 - content_height;
        }
    }

    /// Clamps viewport offset to valid range given total items and visible content height.
    pub fn clamp_viewport(&mut self, total_items: usize, content_height: usize) {
        if total_items <= content_height {
            self.viewport_offset = 0;
        } else if self.viewport_offset + content_height > total_items {
            self.viewport_offset = total_items - content_height;
        }
    }
}

/// Formats the complete floating modal lines including rounded unicode borders,
/// header with position badge, track list rows, and footer key hints.
pub fn format_modal_lines(
    rect: &ModalRect,
    modal: &ModalState,
    queue: &crate::queue::Queue,
) -> Vec<String> {
    let width = rect.width as usize;
    let content_height = rect.content_height();
    let content_width = rect.content_width();
    let total_items = queue.playback_order.len();

    let mut lines = Vec::with_capacity(rect.height as usize);

    // 1. Top border with Header: [current/total]
    let badge = if total_items == 0 {
        "[0/0]".to_string()
    } else {
        format!("[{}/{}]", modal.viewer_cursor + 1, total_items)
    };
    let header_text = format!(" Queue {} ", badge);

    let top_border = if width <= 2 {
        "╭╮".to_string()
    } else {
        let inner_width = width - 2;
        if header_text.chars().count() + 2 <= inner_width {
            let mut s = String::from("╭─");
            s.push_str(&header_text);
            let used = 1 + header_text.chars().count();
            let remaining = inner_width.saturating_sub(used);
            s.push_str(&"─".repeat(remaining));
            s.push('╮');
            s
        } else {
            let mut s = String::from("╭");
            s.push_str(&"─".repeat(inner_width));
            s.push('╮');
            s
        }
    };
    lines.push(top_border);

    // 2. Content rows
    if total_items == 0 {
        for row in 0..content_height {
            if row == 0 {
                let msg = "  (Queue is empty)";
                let msg_chars = msg.chars().count();
                let padding = content_width.saturating_sub(msg_chars);
                lines.push(format!("│{}{}│", msg, " ".repeat(padding)));
            } else {
                lines.push(format!("│{}│", " ".repeat(content_width)));
            }
        }
    } else {
        for row in 0..content_height {
            let item_index = modal.viewport_offset + row;
            if item_index < total_items {
                let track_idx = queue.playback_order[item_index];
                let track = &queue.tracks[track_idx];
                let is_active = item_index == queue.cursor;
                let is_cursor = item_index == modal.viewer_cursor;

                let active_marker = if is_active { "▶ " } else { "  " };
                let select_marker = if modal.is_selected(item_index) { "[x] " } else { "[ ] " };
                let num_str = format!("{:>3}. ", item_index + 1);
                let prefix = format!(" {}{}{}", active_marker, select_marker, num_str);
                let prefix_len = prefix.chars().count();

                let avail_title_len = content_width.saturating_sub(prefix_len + 1);
                let title = truncate_to_width(&track.title, avail_title_len);


                let mut row_content = format!("{}{}", prefix, title);
                let content_chars = row_content.chars().count();
                if content_chars < content_width {
                    row_content.push_str(&" ".repeat(content_width - content_chars));
                }

                if is_cursor {
                    lines.push(format!("│\x1b[7m{}\x1b[0m│", row_content));
                } else {
                    lines.push(format!("│{}│", row_content));
                }
            } else {
                lines.push(format!("│{}│", " ".repeat(content_width)));
            }
        }
    }

    // 3. Bottom border with Footer hints
    let footer_text = " Enter: Play │ j/k: Scroll │ Tab/q/Esc: Close ";
    let bottom_border = if width <= 2 {
        "╰╯".to_string()
    } else {
        let inner_width = width - 2;
        if footer_text.chars().count() + 2 <= inner_width {
            let mut s = String::from("╰─");
            s.push_str(footer_text);
            let used = 1 + footer_text.chars().count();
            let remaining = inner_width.saturating_sub(used);
            s.push_str(&"─".repeat(remaining));
            s.push('╯');
            s
        } else {
            let short_footer = " Enter: Play │ Esc: Close ";
            if short_footer.chars().count() + 2 <= inner_width {
                let mut s = String::from("╰─");
                s.push_str(short_footer);
                let used = 1 + short_footer.chars().count();
                let remaining = inner_width.saturating_sub(used);
                s.push_str(&"─".repeat(remaining));
                s.push('╯');
                s
            } else {
                let mut s = String::from("╰");
                s.push_str(&"─".repeat(inner_width));
                s.push('╯');
                s
            }
        }
    };
    lines.push(bottom_border);

    lines
}

/// Renders the floating queue modal directly to the terminal/writer at (rect.x, rect.y).
pub fn render_floating_modal<W: Write>(
    writer: &mut W,
    rect: &ModalRect,
    modal: &ModalState,
    queue: &crate::queue::Queue,
) -> io::Result<()> {
    let lines = format_modal_lines(rect, modal, queue);
    for (i, line) in lines.iter().enumerate() {
        execute!(
            writer,
            MoveTo(rect.x, rect.y + i as u16),
            Print(line)
        )?;
    }
    writer.flush()
}

/// Switches the terminal to the alternate screen buffer, clears it, and hides the cursor.
pub fn enter_alternate_screen<W: Write>(writer: &mut W) -> io::Result<()> {
    execute!(
        writer,
        EnterAlternateScreen,
        Clear(ClearType::All),
        Hide
    )?;
    writer.flush()
}

/// Restores the terminal from the alternate screen buffer back to the primary screen.
pub fn leave_alternate_screen<W: Write>(writer: &mut W) -> io::Result<()> {
    execute!(
        writer,
        LeaveAlternateScreen,
        Hide
    )?;
    writer.flush()
}
