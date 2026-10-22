use crate::cache::AudioCache;
use crate::mpv::{EndFileReason, MpvController, MpvError, MpvEvent};
use crate::prefetch::AudioPrefetcher;
use crate::queue::Queue;
use crate::ui::{
    parse_key_event, render_status_bar, setup_signal_handler, KeyAction, StatusState,
    TerminalGuard,
};
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
}

impl App {
    pub fn new(
        queue: Queue,
        cache: AudioCache,
        prefetcher: AudioPrefetcher,
        mpv: MpvController,
    ) -> Self {
        Self {
            queue,
            cache,
            prefetcher,
            mpv,
        }
    }

    /// Loads and plays the current track in the queue, then schedules prefetching the next track.
    pub fn play_current(&mut self) -> Result<(), MpvError> {
        if let Some(track) = self.queue.current_track() {
            let source = self.cache.resolve_source(&track.id);
            self.mpv.loadfile(source.as_str())?;
            self.prefetcher.prefetch_next(&self.queue);
        }
        Ok(())
    }

    /// Handles user key actions, modifying playback or queue state accordingly.
    /// Returns `Ok(false)` if the application should terminate.
    pub fn handle_action(&mut self, action: KeyAction) -> Result<bool, MpvError> {
        match action {
            KeyAction::TogglePause => {
                self.mpv.toggle_pause()?;
            }
            KeyAction::NextTrack => {
                if self.queue.next().is_some() {
                    self.play_current()?;
                }
            }
            KeyAction::PreviousTrack => {
                if self.queue.prev().is_some() {
                    self.play_current()?;
                }
            }
            KeyAction::ToggleShuffle => {
                self.queue.toggle_shuffle();
                self.prefetcher.prefetch_next(&self.queue);
            }
            KeyAction::ResetToTop => {
                self.queue.reset_to_top();
                self.play_current()?;
            }
            KeyAction::ToggleLoop => {
                self.queue.toggle_loop();
            }
            KeyAction::Quit => {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Processes MPV IPC events, advancing track automatically on EOF.
    pub fn handle_mpv_event(&mut self, event: MpvEvent) -> Result<(), MpvError> {
        if let MpvEvent::EndFile {
            reason: EndFileReason::Eof,
        } = event
        {
            if self.queue.next().is_some() {
                self.play_current()?;
            }
        }
        Ok(())
    }

    /// Builds instantaneous status snapshot from MPV and queue state.
    pub fn current_status_state(&self) -> StatusState<'_> {
        let is_paused = self.mpv.is_paused().unwrap_or(false);
        let position = self.mpv.get_position().unwrap_or(None);
        let duration = self.mpv.get_duration().unwrap_or(None);
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
        }
    }

    /// Formats and renders 2-line status bar to the given writer, truncating to terminal width if available.
    pub fn render_status<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        let state = self.current_status_state();
        let (mut l1, mut l2) = state.format_lines();
        if let Ok((cols, _)) = crossterm::terminal::size() {
            let width = cols as usize;
            if width > 0 {
                l1 = crate::ui::truncate_to_width(&l1, width);
                l2 = crate::ui::truncate_to_width(&l2, width);
            }
        }
        render_status_bar(writer, &l1, &l2)
    }

    /// Runs the interactive event loop with the specified termination signal flag.
    pub fn run_with_signal(
        &mut self,
        term_signal: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut guard = TerminalGuard::enter()?;

        let _ = self.play_current();

        let mut stdout = io::stdout();
        let tick_rate = Duration::from_millis(250);

        loop {
            if term_signal.load(Ordering::Relaxed) {
                break;
            }

            while let Some(ev) = self.mpv.poll_event() {
                let _ = self.handle_mpv_event(ev);
            }

            let _ = self.render_status(&mut stdout);

            if crossterm::event::poll(tick_rate)? {
                if let crossterm::event::Event::Key(key_event) = crossterm::event::read()? {
                    if let Some(action) = parse_key_event(key_event) {
                        let keep_running = self.handle_action(action)?;
                        if !keep_running {
                            break;
                        }
                        let _ = self.render_status(&mut stdout);
                    }
                }
            }
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

