# 05: Non-Scrolling 2-Line In-Place Terminal UI & Signal Cleanup

**What to build:** The user-facing terminal interface using crossterm raw mode. Renders 2 lines strictly in place (`\r\x1b[2K`), displaying the current track name, playback progress bar, shuffle/loop mode badges, and the up-next song. Dispatches non-blocking keyboard inputs (`Space`, `>`, `<`, `s`, `r`, `l`, `q`, `Ctrl+C`) and cleanly clears the terminal display and restores the cursor on exit.

**Blocked by:** 04: Ahead-of-Time Audio Prefetcher with Mole-Friendly LRU Pruning

**Status:** ready-for-agent

- [ ] Enter raw mode, hide cursor, and capture single keypress events non-blockingly.
- [ ] Format and overwrite 2-line status bar in-place on a 250ms tick loop.
- [ ] Render playback states: playing `[▶]`, paused `[⏸]`, current time, total duration, and cache indicator `[⚡ Cached]`.
- [ ] Bind keys: `Space` (pause), `>` (next), `<` (prev), `s` (shuffle), `r` (top of iceberg), `l` (loop), `q` / `Ctrl+C` (quit).
- [ ] Gracefully handle terminal exit (`SIGINT`/`SIGTERM` or `q`), clearing the status bar lines and unhiding cursor.
- [ ] Deploy compiled release binary to `~/.local/bin/ytm`.
