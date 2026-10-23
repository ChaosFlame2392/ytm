# 08-1: Dedicated Authentication & External Process Command Builder

**What to build:** Move cookie and authentication handling out of `src/cache.rs` into a dedicated subsystem (`src/auth.rs`). Provide clean resolution hierarchy: `YTM_COOKIES_FILE`, `YTM_COOKIES_FROM_BROWSER`, standard user config `~/.config/ytm/cookies.txt` (avoiding disposable cache directories), and platform-aware browser detection. Validate browser identifiers instead of using unvalidated raw strings. Centralize `yt-dlp` command construction to remove duplicated logic in `src/prefetch.rs` and `src/playlist.rs`, and provide a clean helper for `mpv` cookie configuration in `src/mpv.rs`.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Create dedicated `src/auth.rs` containing `CookieSource` and browser validation.
- [ ] Support config file lookup in `~/.config/ytm/cookies.txt` (and `$XDG_CONFIG_HOME/ytm/cookies.txt`), removing cookie lookup from disposable cache paths.
- [ ] Provide unified `build_ytdlp_command()` helper for assembling `yt-dlp` invocations with standard flags and resolved cookies.
- [ ] Provide `configure_mpv_command()` helper for injecting cookie arguments into `mpv`.
- [ ] Refactor `src/cache.rs` to remove divergent cookie resolution logic.
- [ ] Refactor `src/prefetch.rs`, `src/playlist.rs`, and `src/mpv.rs` to consume the centralized helpers.
- [ ] Unit tests in `src/auth.rs` and `tests/cache_test.rs` verifying the resolution hierarchy and argument formatting offline.
