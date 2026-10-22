# 03: Two-Stage Modal Search Filter

**What to build:** While inside the floating queue modal, pressing `/` activates an inline search prompt (`Search: <query>`). As the user types, the list live-filters to show only tracks whose titles contain the query (case-insensitive). The user navigates matching results with `j` / `k` or `Down` / `Up` arrow keys. Pressing `Enter` on a matched song resolves the search: it exits search mode, restores the full queue view, and centers the Viewer Cursor directly on that song within its surrounding queue context (natural or shuffled), allowing inspection of adjacent tracks before committing playback. Pressing `Esc` in search mode clears the search query and restores the previous queue cursor position.

**Blocked by:** 06-2: NVIM-Style Floating Queue Modal & Direct Track Jump

**Status:** completed

- [x] Pressing `/` within the floating modal enters search mode and displays the search prompt.
- [x] Character inputs append to the search query and trigger immediate live title filtering.
- [x] `Backspace` deletes characters from the query string and updates results.
- [x] `j` / `k` and `Down` / `Up` navigate through matching results.
- [x] If no matches exist, an indicator (e.g. `[No matching tracks]`) is displayed.
- [x] Pressing `Enter` on a match resolves the search, exits search mode, and centers the full queue view on that song in its actual active queue context.
- [x] Pressing `Esc` in search mode aborts the search, restoring the full queue and original cursor.
- [x] Integration tests in `tests/app_test.rs` verify search filtering, navigation, resolution, and cancellation.
