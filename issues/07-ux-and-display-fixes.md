# Issue 07: Post-Launch Terminal Display, Seeking Stability & Queue Subsetting

**Labels:** `completed`

## Problem Statement

During real-world dogfooding of `ytm` v0.1.0 with diverse YouTube Music playlists (including multi-lingual Japanese/Korean/Chinese tracks, rapid seeking, and custom queue workflows), listeners encountered several usability and stability issues:

1. **CJK Wide-Character Scrolling & Border Skewing**:
   - `truncate_to_width` and modal row formatting measure string length with `s.chars().count()` instead of terminal display cell width.
   - East Asian characters (Hangul `마법소녀`, Kanji/Chinese `魔法少女`, Katakana `フィオニー`, fullwidth punctuation `【】`, emojis `⚡`) occupy 2 terminal columns.
   - When a title exceeds terminal width, the 2-line in-place status bar wraps to a 3rd line. The subsequent `\x1b[1A` ANSI escape only moves up 1 line, dumping a new line into terminal scrollback on every 250ms tick.
   - In the modal, CJK lines push the right border `│` outward or wrap characters around to the left border.

2. **Hold `l` Past Duration Crashes Player**:
   - Holding `l` (relative forward seek 5s) past track duration causes MPV to reject the seek command once EOF is reached, returning `{"error": "error running command"}`.
   - `App::handle_action` uses `?` on `self.seek(5.0)`, bubbling the error up to `App::run`, which crashes with `Player error: MPV IPC error: error running command`.

3. **Hold `h` Stutter / Noise at 00:00**:
   - Holding `h` (rewind) at 30Hz keyboard repeat when already at `00:00` repeatedly sends relative seek to MPV, causing rapid buffer flushes and audio stuttering.

4. **`^H` Backspace in Search Modal**:
   - `parse_modal_search_key_event` only inspects `KeyCode::Backspace`. In terminals where erase is configured to `^H`, keypresses arrive as `KeyCode::Char('h')` with `KeyModifiers::CONTROL`, `\x08`, or `\x7f`, ignoring backspace.

5. **Playback Auto-Run Continuity**:
   - Track 1 loads in paused state (`[⏸]`). Once the listener initiates playback (`Space` or `Enter`), subsequent tracks and queue advancements should automatically play without remaining paused.

6. **Play/Pause Icon Convention**:
   - Standard music apps display `[▶]` when paused (action to play) and `[⏸]` when playing (action to pause). Currently inverted.

7. **Queue Subsetting & Range Looping**:
   - When tracks are cherry-picked with `Space` and committed with `Enter`, the user expects this selection set to form an active isolated queue (looping over only those tracks, and shuffling among them with `s`), until reset with `r`.

8. **Prefetch Stderr Pollution**:
   - Background worker failures (e.g. YouTube 403 HTTP forbidden) print via `eprintln!`, dumping raw text lines onto the active raw terminal display.

## Acceptance Criteria

- [x] Add `unicode-width` crate and update `truncate_to_width` and `format_modal_row` to compute and truncate based on actual terminal column display width (`UnicodeWidthStr::width`).
- [x] Ensure status bar lines never exceed terminal column width even with mixed CJK/ASCII/emojis.
- [x] Seeking forward past duration clamps cleanly to track end or transitions to next queue track without erroring or crashing.
- [x] Seeking backward when position is already `00:00` ignores redundant seek commands to prevent audio stutter noise.
- [x] In `parse_modal_search_key_event`, support `^H` (`Ctrl+h`), `\x08`, and `\x7f` as `SearchBackspace`.
- [x] Ensure `play_current()` unpauses MPV once initial playback has been triggered.
- [x] Flip the status bar icon: display `[▶]` when paused, `[⏸]` when playing.
- [x] Support isolated queue loop and shuffle for selected tracks until iceberg reset (`r`).
- [x] Route prefetch background errors away from stdout/stderr when terminal raw mode is active.
