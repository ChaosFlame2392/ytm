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
        let _ = cache.prune_lru();
        Self {
            queue,
            cache,
            prefetcher,
            mpv,
        }
    }

    /// Loads and plays the current track in the queue.
    /// If not cached, starts playing immediately via remote streaming URL and initiates background caching.
    /// In all cases, triggers ahead-of-time prefetching of the next track.
    pub fn play_current(&mut self) -> Result<(), MpvError> {
        if let Some(track) = self.queue.current_track() {
            let is_cached = self.cache.is_cached(&track.id);
            let source = self.cache.resolve_source(&track.id);
            self.mpv.loadfile(source.as_str())?;
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
        self.mpv.pause()?;
        self.play_current()?;
        Ok(())
    }

    /// Seeks relatively within the currently playing track by `offset_seconds`.
    pub fn seek(&mut self, offset_seconds: f64) -> Result<(), MpvError> {
        self.mpv.seek(offset_seconds)
    }

    /// Handles user key actions, modifying playback or queue state accordingly.
    /// Returns `Ok(false)` if the application should terminate.
    pub fn handle_action(&mut self, action: KeyAction) -> Result<bool, MpvError> {
        match action {
            KeyAction::TogglePause => {
                self.mpv.toggle_pause()?;
            }
            KeyAction::NextTrack => {
                if self.queue.skip_next().is_some() {
                    self.play_current()?;
                }
            }
            KeyAction::PreviousTrack => {
                if self.queue.prev().is_some() {
                    self.play_current()?;
                }
            }
            KeyAction::SeekForward => {
                self.seek(5.0)?;
            }
            KeyAction::SeekBackward => {
                self.seek(-5.0)?;
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
        }
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

            let _ = self.render_status(&mut stdout);

            if crossterm::event::poll(tick_rate)? {
                if let crossterm::event::Event::Key(key_event) = crossterm::event::read()? {
                    if let Some(action) = parse_key_event(key_event) {
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

