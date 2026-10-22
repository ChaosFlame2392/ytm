# 01: Playlist Metadata & URL Storage

**What to build:** The CLI interface for setting and caching playlist URLs. Allows passing `ytm <URL>` or `--sync` to switch playlists or force-refresh. Caches the active playlist URL in `~/Library/Caches/ytm/playlist.url` and extracts track metadata into `~/Library/Caches/ytm/tracks.tsv` via `yt-dlp`.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] Support reading active playlist URL from `~/Library/Caches/ytm/playlist.url`, defaulting to Chills playlist.
- [ ] Support passing a new URL argument to swap the active playlist without editing config.
- [ ] Extract and format tracks into `<index>\t<id>\t<title>` via `yt-dlp --no-warnings --flat-playlist`.
- [ ] Auto-sync if cache is missing or older than 15 minutes.
