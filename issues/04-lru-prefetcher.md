# 04: Ahead-of-Time Audio Prefetcher with Mole-Friendly LRU Pruning

**What to build:** An asynchronous background downloader that proactively caches upcoming queue tracks to `~/Library/Caches/ytm/audio/<id>.opus` using `yt-dlp`. If a song is cached, it plays from local disk; otherwise it streams and caches simultaneously. Enforces a 500MB LRU cache cap, pruning oldest accessed files so disk space remains tidy and compatible with cleaner apps like Mole.

**Blocked by:** 03: Queue State Machine with Dynamic Shuffle & Iceberg Reset

**Status:** ready-for-agent

- [ ] Store audio in standard macOS cache: `~/Library/Caches/ytm/audio/`.
- [ ] On track change, inspect `queue[current + 1]` and spawn background download worker if not cached.
- [ ] Prefer local `.opus` file if present, falling back to YouTube streaming URL.
- [ ] Implement LRU cache eviction: check directory size, delete oldest accessed `.opus` files if exceeding 500MB until below 400MB.
- [ ] Unit tests for LRU size calculation and pruning order.
