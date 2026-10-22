# 02: Headless MPV IPC Audio Controller

**What to build:** A background `mpv` process controller communicating over a local Unix domain socket (`/tmp/ytm-mpv.sock`). Completely silences all mpv terminal output and provides programmatic control for loading files/URLs, seeking, pausing, querying playback time, and detecting track completion.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] Spawn `mpv` in headless mode (`--no-video --really-quiet --input-ipc-server=/tmp/ytm-mpv.sock`).
- [ ] Connect UnixStream to the IPC socket and exchange JSON-RPC commands.
- [ ] Implement `loadfile(path_or_url)`, `toggle_pause()`, `get_position()`, `get_duration()`.
- [ ] Listen for `eof-reached` or playback end events to trigger track transition.
- [ ] Gracefully terminate `mpv` on player exit.
