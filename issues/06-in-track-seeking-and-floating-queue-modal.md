# Issue 06: In-Track Seeking & NVIM-Style Floating Queue Modal

**Labels:** `ready-for-agent`

## Problem Statement

While `ytm` succeeds as an ultra-lightweight, memory-efficient YouTube Music streamer with a clean 2-line in-place status bar, listeners encounter several critical interaction and navigation hurdles:
1. **No In-Track Scrubbing**: Listeners cannot move forward or backward within a track (e.g. to skip an intro, replay a favorite bridge, or scrub through long mixes/podcasts). The `Left` and `Right` arrow keys currently perform full track skips, leading to accidental track loss instead of fine-grained seeking.
2. **Invisible Playlist & Inability to Jump Arbitrarily**: The 2-line status bar provides no visibility into upcoming tracks or the overall playlist catalog. To reach track 45 in a 100-track playlist, a user must hit skip 44 times, with no ability to browse, inspect, or jump directly.
3. **No Ephemeral Queue Manipulation**: Users have no quick way to pick a handful of tracks from their playlist to play next in a specific temporary sequence.
4. **Disorienting Auto-Play on Launch**: Launching `ytm` currently starts blaring audio on Track 1 immediately without clearing the terminal screen, depriving the user of an initial paused orientation window where they can view the tracklist or select a different starting point.

## Solution

1. **In-Track Seeking**: Provide relative 5-second backward and forward seeking within the currently playing track via `h` / `l` and `Left` / `Right` arrow keys with instantaneous visual feedback on the status bar. Seeking clamps cleanly to `00:00` on rewind and triggers natural track advancement when seeking past duration. Track skipping remains strictly bound to `<` and `>`.
2. **Clean Paused Startup**: Clear the terminal display on launch and load Track 1 in an Initial Paused State (`[⏸]`), waiting for user initiation (`Space` to play or `Tab` to browse).
3. **NVIM-Style Floating Queue Modal**: Render an interactive centered modal window (`╭─╮│╰─╯`) sized to ~80% width and ~70% height in Crossterm's Alternate Screen buffer when `Tab` is pressed. The modal displays the active playback queue with viewport scrolling centered on the current track, exiting cleanly without leaving terminal scrollback artifacts.
4. **Queue Selection Set & Instant Jump**: Within the modal, `Space` toggles track selection marking (`[x]`), and `Enter` constructs a new playback queue with the selected tracks at the front followed by the remainder of the playlist. If no tracks are marked, `Enter` immediately jumps to and plays the hovered track.
5. **Two-Stage Modal Search Filter**: Pressing `/` in the modal opens an inline search prompt that live-filters tracks by title. Pressing `Enter` on a matched track resolves the search by clearing the filter and centering the full queue view on that track in its active context (shuffled or natural), allowing inspection before committing playback.

## User Stories

1. As a listener, I want to press `l` or `Right` arrow during playback, so that the audio jumps forward by 5 seconds within the current track.
2. As a listener, I want to press `h` or `Left` arrow during playback, so that the audio rewinds by 5 seconds within the current track.
3. As a listener, I want backward in-track seeking to clamp at `00:00`, so that rewinding near the start never causes errors or skips to previous tracks unexpectedly.
4. As a listener, I want forward in-track seeking past the end of the song to trigger an automatic transition to the next track, so that seeking beyond duration feels natural and matches standard streaming apps.
5. As a listener, I want the status bar's elapsed timecode `(MM:SS / MM:SS)` to update immediately upon pressing a seek key, so that in-track navigation feels instant and responsive.
6. As a listener, I want `<` and `>` (as well as `,` and `.`) to remain dedicated to track skips, so that in-track seeking and track navigation never conflict.
7. As a listener launching `ytm`, I want the terminal screen to be cleared and Track 1 loaded in a paused state, so that music does not blare unexpectedly before I am ready.
8. As a listener, I want to press `Space` from the initial paused state, so that audio playback begins smoothly on the first track.
9. As a listener, I want to press `Tab` at any time, so that an NVIM-style floating modal opens showing the active playback queue.
10. As a listener, I want the floating queue modal to open in the terminal's alternate screen buffer, so that opening and scrolling through the modal never pollutes my shell scrollback history.
11. As a listener, I want the floating queue modal to automatically focus and center its viewport on the currently playing track upon opening, so that I immediately see where I am in the playlist.
12. As a listener, I want to navigate the modal's viewer cursor up and down using `j`/`k` and `Down`/`Up` arrow keys, so that browsing tracks feels familiar and ergonomic.
13. As a listener, I want the modal's viewport to smoothly scroll when the viewer cursor moves past visible boundaries, so that I can browse playlists of arbitrary length (e.g. 200+ songs).
14. As a listener, I want the modal to display track numbers, titles, cache indicators (`[⚡]`), and an overall position badge (e.g. `[14/150]`), so that I have complete context about my library.
15. As a listener, I want to press `Enter` on any highlighted track with no tracks marked, so that playback immediately jumps to that song and closes the modal.
16. As a listener, I want to press `Space` on tracks in the modal, so that I can mark individual songs (`[x]`) into a temporary queue selection set.
17. As a listener, I want to press `Space` on an already marked track, so that it is unmarked and removed from the selection set.
18. As a listener, I want to press `c` while in the modal, so that all current track selections are cleared at once.
19. As a listener, I want to press `Enter` after selecting multiple tracks with `Space`, so that the playback queue is reordered to play those selected tracks first in pick order, followed by the rest of the playlist.
20. As a listener, I want to press `/` while inside the modal, so that an inline search prompt opens allowing me to filter tracks by title.
21. As a listener typing into the search prompt, I want the displayed list to filter in real-time, so that I can locate specific tracks within seconds.
22. As a listener navigating filtered search results, I want pressing `Enter` on a match to resolve the search and place the viewer cursor on that song within the full active queue, so that I can inspect surrounding tracks in their actual context.
23. As a listener in search mode, I want pressing `Esc` to cancel the search filter, so that I return directly to my prior position in the full queue view.
24. As a listener, I want to press `q`, `Tab`, or `Esc` from the modal's normal view, so that the modal closes and restores the clean 2-line status bar without interrupting ongoing playback.
25. As a listener with shuffle active, I want the modal to display the true active playback order, so that what I see matches what will play next.
26. As a listener, I want pressing `s` to toggle shuffle ON and OFF in YouTube Music style, and `r` to reset playback back to Track 1, preserving predictable queue mechanics.

## Implementation Decisions

- **Architectural Seams**:
  - The feature will be implemented directly across existing application seams:
    - `MpvController`: Add `seek(offset_seconds: f64) -> Result<(), MpvError>` delegating to MPV IPC command `["seek", "<seconds>", "relative"]`.
    - `Queue`: Add queue manipulation methods:
      - `jump_to(cursor_index: usize) -> Option<&Track>`
      - `reorder_with_selection(selected_indices: &[usize]) -> Option<&Track>`
    - `TerminalGuard`: Augment `enter()` to execute `Clear(ClearType::All)` and cursor repositioning to ensure a clean startup canvas.
    - `UI`: Introduce a modal state machine and rendering pipeline:
      - `ModalState`: Tracks `is_open: bool`, `viewer_cursor: usize`, `viewport_offset: usize`, `selection_set: Vec<usize>`, and `SearchState` (`active: bool`, `query: String`, `filtered_indices: Vec<usize>`).
      - `render_floating_modal`: Renders a centered box using unicode rounded borders (`╭─╮│╰─╯`), computing box geometry from dynamic terminal dimensions (`cols * 0.8`, `rows * 0.7`). Uses alternate screen buffer mode (`EnterAlternateScreen` / `LeaveAlternateScreen`).
    - `App`: Connect keyboard events to the modal when open and standard playback when closed. When starting, load Track 1 in paused mode without auto-playing. Immediate UI rendering is triggered on seek and modal navigation without waiting for the 250ms tick.

- **Keybinding Contracts**:
  - **Normal Mode**:
    - `Space`: Toggle Pause/Play
    - `>` / `.` : Skip to Next Track
    - `<` / `,` : Skip to Previous Track
    - `h` / `Left`: Seek backward 5s
    - `l` / `Right`: Seek forward 5s
    - `s`: Toggle Shuffle (ON randomizes upcoming; OFF restores sequential order from current track)
    - `r`: Reset to Track 1 (Iceberg Reset)
    - `Tab`: Open Floating Queue Modal
    - `q` / `Ctrl+C`: Quit Application
  - **Modal Browsing Mode**:
    - `j` / `Down`: Move Viewer Cursor down
    - `k` / `Up`: Move Viewer Cursor up
    - `Space`: Toggle selection on hovered track (`[x]` / `[ ]`)
    - `c`: Clear all track selections
    - `/`: Activate Modal Search Filter
    - `Enter`: Commit action (jump to hovered track if no selections, or play custom selection set if selections exist)
    - `Tab` / `q` / `Esc`: Close modal and restore 2-line status bar
  - **Modal Search Mode**:
    - Characters: Append to query string and live-filter list
    - `Backspace`: Remove last character from query
    - `j` / `k` / `Up` / `Down`: Navigate filtered matches
    - `Enter`: Resolve search, restoring full queue view centered on the chosen match
    - `Esc`: Abort search, restoring prior full queue position

## Testing Decisions

- **Testing Seams**:
  - Highest Seam: `App` integration tests (`tests/app_test.rs`). Tests will drive mock MPV Unix sockets and feed crossterm `KeyEvent` streams into `App::handle_action` and modal event loops, verifying that:
    - `h`/`l`/arrows send the correct `["seek", "5", "relative"]` JSON IPC payloads.
    - `App` starts in paused state without sending an initial unpause or auto-play.
    - `Tab` triggers modal opening, `Space` tracks selection, and `Enter` executes correct queue reorganization and track switching.
    - Search `/` live-filters and resolves back to full queue context.
  - UI Rendering Seam: `tests/ui_test.rs`. Tests will invoke `render_floating_modal` against in-memory `Vec<u8>` buffers, asserting:
    - Correct rounded border ANSI rendering.
    - Proper viewport slicing when playlist size exceeds terminal window rows.
    - Accurate rendering of `[x]` selection marks, `[⚡]` cache badges, and `▶` active track indicators.
  - Queue Unit Seam: `tests/queue_test.rs`. Tests will verify `reorder_with_selection` and `jump_to` under both linear and shuffled states.

## Out of Scope

- Permanent modification or deletion of playlist tracks from the remote YouTube source.
- Multi-column curses file managers or split panes.
- Sub-range looping (A-B repeat) or custom persistent playlists.
- Mouse click support.

## Further Notes

- Canonical domain terminology is maintained in [GLOSSARY.md](file:///Users/balkrishan/Development/ytm/GLOSSARY.md).
- Architectural rationale for alternate screen modal buffers is documented in [docs/adr/0001-floating-queue-modal-and-in-track-seeking.md](file:///Users/balkrishan/Development/ytm/docs/adr/0001-floating-queue-modal-and-in-track-seeking.md).
