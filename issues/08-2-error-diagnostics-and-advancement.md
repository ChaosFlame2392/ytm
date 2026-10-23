# 08-2: Resilient Error Diagnostics & Queue Advancement

**What to build:** Handle playback failures and backend error events gracefully in `App::handle_mpv_event`. When a track fails to stream or decode (producing `EndFile { reason: EndFileReason::Error }`), update the status bar with non-scrolling error feedback (e.g. `[!] Stream Error`), mark the track so it doesn't loop infinitely, and automatically advance to the next track in the playback queue. Format error feedback within the existing 2-line in-place status bar without exceeding terminal width or spilling into scrollback.

**Blocked by:** 08-1

**Status:** ready-for-agent

- [ ] Update `App::handle_mpv_event` to handle `EndFileReason::Error` and `EndFileReason::Other`.
- [ ] Automatically advance to the next track in the queue when an uncached stream error occurs.
- [ ] Add temporary error notification state to `App` and `StatusState` displayed in the 2-line status bar.
- [ ] Ensure error messages strictly respect terminal column width and CJK display boundaries via `unicode-width`.
- [ ] Tests in `tests/app_test.rs` asserting automatic advancement on MPV error events and proper status bar formatting.
