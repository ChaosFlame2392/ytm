# Specification: ytm (Lightweight YouTube Music Terminal Streamer)

## Problem Statement

Desktop music clients (like Pear Desktop or official web apps) are built on Chromium/Electron, consuming between 500MB and 1GB+ of RAM with multiple helper processes and GPU overhead. On an 8GB RAM Mac, this causes memory pressure and system throttling. Meanwhile, existing CLI and TUI players either suffer from awkward UX (over-reliance on Mac-unfriendly function keys, broken `^H` backspace handling), require cumbersome authentication setups, or flood the terminal scrollback with unreadable media tag and buffer dumps on every track skip. Furthermore, standard YouTube playback lacks an intuitive "iceberg" workflow—looping the top few recent tracks and seamlessly transitioning into deep playlist playback with music-app style shuffle toggling and zero-latency audio caching.

## Solution

A native, ultra-lightweight Rust CLI utility (`ytm`) consuming ~15–20 MB of RAM that drives a silent, background `mpv` process over a local Unix IPC socket. It features a clean, non-scrolling 2-line terminal status bar, Spotify-style shuffle toggling (returning seamlessly to the natural playlist position when disabled), an instantaneous reset button (`r`) to loop back to track 1, and an automatic ahead-of-time background prefetcher that caches upcoming tracks to standard macOS user cache (`~/Library/Caches/ytm/audio/`) with a 500MB LRU limit (cleanable by tools like Mole).

## User Stories

1. As a listener on an 8GB Mac, I want an audio streamer that runs in ~20MB of RAM, so that listening to music never causes memory pressure or swaps my system memory.
2. As a terminal user, I want the player status to overwrite in place on 2 lines, so that skipping tracks does not dump scrolling walls of text and clutter my terminal history.
3. As a listener with an "iceberg" playlist, I want playback to start naturally at the top recent tracks, so that I immediately hear my latest additions.
4. As a listener, I want to press `r` at any arbitrary time, so that playback instantly jumps back to track 1 at the top of the iceberg.
5. As a listener, I want to press `s` to toggle shuffle on, so that upcoming songs are chosen randomly from the playlist.
6. As a listener, I want to press `s` again to toggle shuffle off, so that playback resumes at the natural sequential position of the currently playing track in the original playlist.
7. As a listener, I want the player to preemptively download the next track in the queue to local disk while the current song is playing, so that song transitions are instant and immune to network buffering.
8. As a listener, I want replaying earlier tracks or looping top tracks to read directly from local disk cache, so that repeated listens consume zero network bandwidth.
9. As a Mac owner, I want audio cache stored in standard `~/Library/Caches/ytm/audio/` and capped at 500MB, so that disk cleaner utilities like Mole can safely scan and purge it without breaking the player.
10. As a user, I want to pass a new playlist URL via CLI (`ytm <URL>`), so that I can swap playlists without editing configuration files.
11. As a listener, I want standard playback controls (`Space` for pause/resume, `>` for next, `<` for prev, `l` for single-track loop), so that controlling music feels familiar.
12. As a terminal user, I want `q` and `Ctrl+C` to quit cleanly, so that all background audio processes terminate and the terminal cursor is properly restored.

## Implementation Decisions

- **Engine:** Built in native Rust as a single compiled binary at `~/.local/bin/ytm`.
- **Playback Backend:** Managed child `mpv` process running in `--no-video` mode controlled silently via JSON IPC over a local Unix domain socket (`/tmp/ytm-mpv.sock`).
- **Terminal Presentation:** Crossterm raw mode with carriage return `\r` and ANSI line clearing `\x1b[2K`, restoring the cursor and terminal buffer on exit.
- **Cache Architecture:**
  - Track list metadata: `~/Library/Caches/ytm/tracks.tsv`
  - Audio files: `~/Library/Caches/ytm/audio/<video_id>.opus`
  - LRU Eviction: Background thread enforces a 500MB cap, pruning oldest accessed files when exceeded.
- **Prefetch Worker:** Dedicated background thread that monitors queue position and uses `yt-dlp` to fetch `queue[current + 1]` into the audio cache before the current track finishes.
- **Queue State Machine:**
  - Maintains `natural_index` (original position in playlist) and `queue` (active playback order).
  - Toggling shuffle preserves `current_song_id`, reshuffles remaining tracks on enable, and restores linear order starting from `natural_index + 1` on disable.

## Testing Decisions

- **Black-box behavioral testing:** Verify queue state transitions, IPC socket communication, and LRU pruning against real or mock inputs without relying on internal structs.
- **Unit Tests:**
  - Queue state machine: verify natural order -> shuffle -> unshuffle returns to exact expected track index.
  - LRU cache eviction: verify files over 500MB are deleted in oldest-access order.
- **Integration Tests:**
  - IPC client test: spawn a headless mpv instance with socket, verify get_property, pause, and play commands.

## Out of Scope

- In-terminal playlist editing or adding songs (user manages playlists via web browser).
- Visualizers, album art rendering, or multi-pane curses interfaces.
- Proprietary account login / browser cookie extraction (operates on standard YouTube playlist URLs).

## Further Notes

- The binary will be deployed to `~/.local/bin/ytm` which is already present on the user's system `$PATH`.
