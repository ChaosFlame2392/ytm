# 01: Clean Screen Startup & In-Track Seeking

**What to build:** When launching `ytm`, the terminal screen is cleared and Track 1 loads in an Initial Paused State (`[⏸]`) displaying the clean 2-line status bar, waiting for user input (`Space` to play or `Tab` to browse). During playback, pressing `h` / `l` or `Left` / `Right` arrow keys performs relative 5-second in-track seeking (backward and forward) with immediate visual timecode feedback on the status bar. Rewinding clamps cleanly to `00:00`, while forward seeking past track duration triggers natural EOF track advancement to the next song in the queue. Track skipping remains strictly bound to `<` and `>`.

**Blocked by:** None (can start immediately)

**Status:** completed

- [x] `ytm` clears the terminal screen upon launch (`Clear(ClearType::All)`).
- [x] Track 1 loads in paused mode (`[⏸]`) without auto-playing audio until `Space` is pressed.
- [x] `h` and `Left` arrow keys seek backward by 5 seconds relative to the current position.
- [x] `l` and `Right` arrow keys seek forward by 5 seconds relative to the current position.
- [x] Backward seeking before `00:00` clamps cleanly to `00:00`.
- [x] Forward seeking beyond track duration triggers track completion and transitions to the next queue track.
- [x] Line 1 of the status bar updates immediately with the new seek position without waiting for the 250ms tick.
- [x] `<` and `>` (as well as `,` and `.`) continue to function strictly for previous/next track skips.
- [x] Integration tests in `tests/app_test.rs` verify startup paused state, seek IPC commands, and boundary clamping.

