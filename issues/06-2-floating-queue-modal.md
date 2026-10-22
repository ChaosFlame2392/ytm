# 02: NVIM-Style Floating Queue Modal & Direct Track Jump

**What to build:** Pressing `Tab` enters Crossterm's Alternate Screen buffer and renders an NVIM-style centered floating modal (`╭─╮│╰─╯`) sized to ~80% width and ~70% height over the terminal. The modal displays the active playback queue, initially anchored and centered on the currently playing track with a position badge (e.g. `[14/150]`). Navigating with `j` / `k` or `Down` / `Up` arrow keys smoothly scrolls the viewport around the Viewer Cursor. Pressing `Enter` on any track immediately starts playing that track, updates the queue cursor, and closes the modal back to the clean 2-line status bar. Pressing `Tab`, `q`, or `Esc` closes the modal without affecting ongoing playback. Opening and closing the modal leaves zero scrollback artifacts in the terminal.

**Blocked by:** 06-1: Clean Screen Startup & In-Track Seeking

**Status:** ready-for-agent

- [ ] Pressing `Tab` enters the alternate screen buffer and displays the centered floating modal.
- [ ] Modal border renders cleanly using rounded unicode box-drawing characters (`╭─╮│╰─╯`).
- [ ] Modal dimensions dynamically scale to ~80% width and ~70% height of terminal columns and rows.
- [ ] Opening the modal centers the viewport and cursor on the currently playing track.
- [ ] `j` / `Down` moves the Viewer Cursor down; `k` / `Up` moves the Viewer Cursor up.
- [ ] Viewport scrolling dynamically keeps the cursor visible for playlists larger than the modal height.
- [ ] Header shows track count and position badge; footer shows navigation hints.
- [ ] Pressing `Enter` on a highlighted track switches playback to that track and dismisses the modal.
- [ ] Pressing `Tab`, `q`, or `Esc` dismisses the modal and restores the 2-line status bar without altering playback.
- [ ] Unit tests in `tests/ui_test.rs` verify modal rendering, borders, and viewport calculations.
- [ ] Integration tests in `tests/app_test.rs` verify modal opening, navigation, and track jump.
