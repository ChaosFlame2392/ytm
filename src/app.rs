use crate::cache::AudioCache;
use crate::mpv::{EndFileReason, MpvController, MpvError, MpvEvent};
use crate::prefetch::AudioPrefetcher;
use crate::queue::Queue;
use crate::ui::{
    calculate_modal_rect, enter_alternate_screen, leave_alternate_screen, parse_key_event,
    parse_modal_key_event, parse_modal_search_key_event, render_floating_modal, render_status_bar,
    render_status_bar_with_width, setup_signal_handler, KeyAction, ModalKeyAction, ModalState,
    StatusState, TerminalGuard,
};
use std::collections::HashSet;
use std::io::{self, Write};
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Core application coordinator tying together Queue, AudioCache, AudioPrefetcher,
/// MpvController, and terminal UI loop.
pub struct App {
    pub queue: Queue,
    pub cache: AudioCache,
    pub prefetcher: AudioPrefetcher,
    pub mpv: MpvController,
    pub modal: ModalState,
    pub playback_started: bool,
    pub error_message: Option<String>,
    pub failed_tracks: HashSet<String>,
}

impl App {
    pub fn new(
        queue: Queue,
        cache: AudioCache,
        prefetcher: AudioPrefetcher,
        mpv: MpvController,
    ) -> Self {
        let _ = cache.prune_lru();
        Self {
            queue,
            cache,
            prefetcher,
            mpv,
            modal: ModalState::default(),
            playback_started: false,
            error_message: None,
            failed_tracks: HashSet::new(),
        }
    }

    /// Loads and plays the current track in the queue.
    /// If not cached, starts playing immediately via remote streaming URL and initiates background caching.
    /// In all cases, triggers ahead-of-time prefetching of the next track.
    /// If playback has been initiated by listener, ensures MPV is unpaused.
    pub fn play_current(&mut self) -> Result<(), MpvError> {
        if let Some(track) = self.queue.current_track() {
            let is_cached = self.cache.is_cached(&track.id);
            let source = self.cache.resolve_source(&track.id);
            self.mpv.loadfile(source.as_str())?;
            if self.playback_started {
                self.mpv.unpause()?;
            }
            if !is_cached {
                self.prefetcher.prefetch(&track.id);
            }
            self.prefetcher.prefetch_next(&self.queue);
        }
        Ok(())
    }

    /// Loads Track 1 in Initial Paused State (`[⏸]`) without auto-playing audio.
    /// Prefetches upcoming tracks and prepares status bar.
    pub fn load_initial(&mut self) -> Result<(), MpvError> {
        self.playback_started = false;
        self.error_message = None;
        self.failed_tracks.clear();
        self.mpv.pause()?;
        self.play_current()?;
        Ok(())
    }

    /// Seeks relatively within the currently playing track by `offset_seconds`.
    /// Ignores rewind commands when already at `00:00` to prevent stutter noise.
    /// Clamps or ignores EOF errors on forward seeking without crashing.
    pub fn seek(&mut self, offset_seconds: f64) -> Result<(), MpvError> {
        if offset_seconds < 0.0 {
            if let Ok(Some(pos)) = self.mpv.get_position() {
                if pos <= 0.05 {
                    return Ok(());
                }
            }
        }
        match self.mpv.seek(offset_seconds) {
            Ok(()) => Ok(()),
            Err(MpvError::IpcError(ref _e)) if offset_seconds > 0.0 => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Handles user key actions, modifying playback or queue state accordingly.
    /// Returns `Ok(false)` if the application should terminate.
    pub fn handle_action(&mut self, action: KeyAction) -> Result<bool, MpvError> {
        match action {
            KeyAction::TogglePause => {
                self.playback_started = true;
                self.error_message = None;
                self.mpv.toggle_pause()?;
            }
            KeyAction::NextTrack => {
                self.error_message = None;
                if self.queue.skip_next().is_some() {
                    self.play_current()?;
                }
            }
            KeyAction::PreviousTrack => {
                self.error_message = None;
                if self.queue.prev().is_some() {
                    self.play_current()?;
                }
            }
            KeyAction::SeekForward => {
                let _ = self.seek(5.0);
            }
            KeyAction::SeekBackward => {
                let _ = self.seek(-5.0);
            }
            KeyAction::ToggleShuffle => {
                self.queue.toggle_shuffle();
                self.prefetcher.prefetch_next(&self.queue);
            }
            KeyAction::ResetToTop => {
                self.error_message = None;
                self.failed_tracks.clear();
                self.queue.reset_to_top();
                self.play_current()?;
            }
            KeyAction::ToggleLoop => {
                self.queue.toggle_loop();
            }
            KeyAction::OpenModal => {
                self.open_modal();
            }
            KeyAction::Quit => {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Opens the floating queue modal anchored and centered on the active track.
    pub fn open_modal(&mut self) {
        let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
        let rect = calculate_modal_rect(cols, rows);
        self.modal.open(
            self.queue.cursor,
            self.queue.playback_order.len(),
            rect.content_height(),
        );
    }

    /// Closes the floating queue modal.
    pub fn close_modal(&mut self) {
        self.modal.close();
    }

    /// Handles user actions inside the floating queue modal.
    /// Returns `Ok(false)` if the application should terminate.
    pub fn handle_modal_action(&mut self, action: ModalKeyAction) -> Result<bool, MpvError> {
        let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
        let rect = calculate_modal_rect(cols, rows);
        let content_height = rect.content_height();

        match action {
            ModalKeyAction::CursorDown => {
                if self.modal.search_active {
                    self.modal.move_search_down(content_height);
                } else {
                    self.modal
                        .move_cursor_down(self.queue.playback_order.len(), content_height);
                }
            }
            ModalKeyAction::CursorUp => {
                if self.modal.search_active {
                    self.modal.move_search_up(content_height);
                } else {
                    self.modal.move_cursor_up(content_height);
                }
            }
            ModalKeyAction::ToggleSelection => {
                let total_items = self.queue.playback_order.len();
                if total_items > 0 && self.modal.viewer_cursor < total_items {
                    self.modal.toggle_selection(self.modal.viewer_cursor);
                }
            }
            ModalKeyAction::ClearSelection => {
                self.modal.clear_selection();
            }
            ModalKeyAction::Commit => {
                self.playback_started = true;
                self.error_message = None;
                if !self.modal.selection_set.is_empty() {
                    let selections = self.modal.selection_set.clone();
                    self.close_modal();
                    if self.queue.reorder_with_selection(&selections).is_some() {
                        self.play_current()?;
                    }
                } else {
                    let target_idx = self.modal.viewer_cursor;
                    self.close_modal();
                    if self.queue.jump_to(target_idx).is_some() {
                        self.play_current()?;
                    }
                }
            }
            ModalKeyAction::Close => {
                self.close_modal();
            }
            ModalKeyAction::Quit => {
                self.close_modal();
                return Ok(false);
            }
            ModalKeyAction::SearchActivate => {
                self.modal.start_search(&self.queue);
            }
            ModalKeyAction::SearchChar(c) => {
                self.modal.append_search_char(c, &self.queue, content_height);
            }
            ModalKeyAction::SearchBackspace => {
                self.modal.pop_search_char(&self.queue, content_height);
            }
            ModalKeyAction::SearchResolve => {
                self.modal
                    .resolve_search(self.queue.playback_order.len(), content_height);
            }
            ModalKeyAction::SearchAbort => {
                self.modal.abort_search();
            }
        }
        Ok(true)
    }


    /// Renders the floating queue modal to the given writer.
    pub fn render_modal<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
        let rect = calculate_modal_rect(cols, rows);
        render_floating_modal(
            writer,
            &rect,
            &self.modal,
            &self.queue,
            Some(&self.cache),
            Some(&self.failed_tracks),
        )
    }

    /// Handles a stream or decode error from playback backend.
    /// Sets transient error message, marks track as failed, and advances to next track if available.
    pub fn handle_stream_error(&mut self, detail: Option<&str>) -> Result<(), MpvError> {
        let is_offline = if let Some(d) = detail {
            let lower = d.to_lowercase();
            lower.contains("offline")
                || lower.contains("network")
                || lower.contains("connection")
                || lower.contains("resolve")
                || lower.contains("timeout")
                || lower.contains("unreachable")
                || lower.contains("disconnected")
        } else {
            false
        };

        let msg = if is_offline {
            match detail {
                Some(d) if !d.is_empty() => format!("[!] Offline: {}", d),
                _ => "[!] Offline".to_string(),
            }
        } else {
            match detail {
                Some(d) if !d.is_empty() => format!("[!] Stream Error: {}", d),
                _ => "[!] Stream Error".to_string(),
            }
        };
        self.error_message = Some(msg);

        if let Some(track) = self.queue.current_track() {
            self.failed_tracks.insert(track.id.clone());
        }

        if self
            .queue
            .skip_next_matching(|track| !self.failed_tracks.contains(&track.id))
            .is_some()
        {
            self.play_current()?;
        }

        Ok(())
    }

    /// Processes MPV IPC events, advancing track automatically on EOF or handling playback errors.
    pub fn handle_mpv_event(&mut self, event: MpvEvent) -> Result<(), MpvError> {
        match event {
            MpvEvent::EndFile {
                reason: EndFileReason::Eof,
            } => {
                self.error_message = None;
                if self.queue.next().is_some() {
                    self.play_current()?;
                }
            }
            MpvEvent::EndFile {
                reason: EndFileReason::Error,
            } => {
                self.handle_stream_error(None)?;
            }
            MpvEvent::EndFile {
                reason: EndFileReason::Other(ref msg),
            } => {
                self.handle_stream_error(Some(msg.as_str()))?;
            }
            MpvEvent::PlaybackRestart => {
                self.error_message = None;
            }
            _ => {}
        }
        Ok(())
    }

    /// Builds instantaneous status snapshot from MPV and queue state.
    pub fn current_status_state(&self) -> StatusState<'_> {
        let is_paused = self.mpv.is_paused().unwrap_or(false);
        let position = self.mpv.get_position().unwrap_or(None).map(|p| p.max(0.0));
        let duration = self.mpv.get_duration().unwrap_or(None).map(|d| d.max(0.0));
        let current_track = self.queue.current_track();
        let title = current_track
            .map(|t| t.title.as_str())
            .unwrap_or("No Track Loaded");
        let is_cached = current_track
            .map(|t| self.cache.is_cached(&t.id))
            .unwrap_or(false);
        let next_title = self.queue.peek_next().map(|t| t.title.as_str());

        StatusState {
            is_paused,
            title,
            position,
            duration,
            is_cached,
            shuffle: self.queue.shuffle,
            single_loop: self.queue.single_loop,
            next_title,
            error_message: self.error_message.as_deref(),
        }
    }

    /// Formats and renders 2-line status bar to the given writer with an optional column width limit.
    pub fn render_status_with_width<W: Write>(
        &self,
        writer: &mut W,
        max_width: Option<usize>,
    ) -> io::Result<()> {
        let state = self.current_status_state();
        render_status_bar_with_width(writer, &state, max_width)
    }

    /// Formats and renders 2-line status bar to the given writer.
    pub fn render_status<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        let state = self.current_status_state();
        render_status_bar(writer, &state)
    }

    /// Runs the interactive event loop with the specified termination signal flag.
    pub fn run_with_signal(
        &mut self,
        term_signal: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut guard = TerminalGuard::enter()?;

        let _ = self.load_initial();

        let mut stdout = io::stdout();
        let tick_rate = Duration::from_millis(250);

        loop {
            if term_signal.load(Ordering::Relaxed) {
                break;
            }

            while let Some(ev) = self.mpv.poll_event() {
                let _ = self.handle_mpv_event(ev);
            }

            if self.modal.is_open {
                let _ = self.render_modal(&mut stdout);
            } else {
                let _ = self.render_status(&mut stdout);
            }

            if crossterm::event::poll(tick_rate)? {
                if let crossterm::event::Event::Key(key_event) = crossterm::event::read()? {
                    if self.modal.is_open {
                        let modal_action = if self.modal.search_active {
                            parse_modal_search_key_event(key_event)
                        } else {
                            parse_modal_key_event(key_event)
                        };
                        if let Some(modal_action) = modal_action {
                            match modal_action {
                                ModalKeyAction::Close => {
                                    self.close_modal();
                                    let _ = leave_alternate_screen(&mut stdout);
                                    let _ = self.render_status(&mut stdout);
                                }
                                ModalKeyAction::Commit => {
                                    let keep_running = self.handle_modal_action(modal_action)?;
                                    let _ = leave_alternate_screen(&mut stdout);
                                    let _ = self.render_status(&mut stdout);
                                    if !keep_running {
                                        break;
                                    }
                                }
                                ModalKeyAction::Quit => {
                                    self.close_modal();
                                    let _ = leave_alternate_screen(&mut stdout);
                                    break;
                                }
                                _ => {
                                    let keep_running = self.handle_modal_action(modal_action)?;
                                    if !keep_running {
                                        break;
                                    }
                                    let _ = self.render_modal(&mut stdout);
                                }

                            }
                        }
                    } else if let Some(action) = parse_key_event(key_event) {
                        if action == KeyAction::OpenModal {
                            let _ = enter_alternate_screen(&mut stdout);
                            self.open_modal();
                            let _ = self.render_modal(&mut stdout);
                        } else {
                            let keep_running = self.handle_action(action)?;
                            if !keep_running {
                                break;
                            }
                            while let Some(ev) = self.mpv.poll_event() {
                                let _ = self.handle_mpv_event(ev);
                            }
                            let _ = self.render_status(&mut stdout);
                        }
                    }
                }
            }
        }

        if self.modal.is_open {
            let _ = leave_alternate_screen(&mut stdout);
        }
        guard.leave();
        let _ = self.mpv.quit();
        Ok(())
    }

    /// Runs the main interactive event loop with 250ms tick rate.
    pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let term_signal = setup_signal_handler()?;
        self.run_with_signal(term_signal)
    }
}

