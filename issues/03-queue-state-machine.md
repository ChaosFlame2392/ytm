# 03: Queue State Machine with Dynamic Shuffle & Iceberg Reset

**What to build:** The queue logic handling natural sequential playback (top of iceberg downward), instant reset back to track 1 (`r`), single-track loop (`l`), and Spotify-style shuffle toggling (`s`) that switches seamlessly between randomized upcoming songs and returning to the natural playlist order from the active track.

**Blocked by:** 01: Playlist Metadata & URL Storage, 02: Headless MPV IPC Audio Controller

**Status:** ready-for-agent

- [ ] Maintain `tracks` list, current track pointer, and natural index.
- [ ] Implement `next()` and `prev()` respecting active loop and shuffle state.
- [ ] Implement `toggle_shuffle()`: when enabled, randomizes upcoming queue; when disabled, snaps upcoming queue back to `natural_index + 1`.
- [ ] Implement `reset_to_top()`: sets current track back to index 0 (top of the iceberg).
- [ ] Unit tests for queue transitions and shuffle toggle behavior.
