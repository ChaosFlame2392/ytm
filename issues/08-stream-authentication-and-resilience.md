# Issue 08: Stream Authentication, Resilient Playback & Error Diagnostics

**Labels:** `ready-for-agent`

## Problem Statement

When streaming YouTube Music playlists in `ytm`, tracks that are already cached locally in the audio cache play instantly and reliably. However, uncached tracks fail to stream or prefetch because YouTube's player API returns automated bot verification challenges ("Sign in to confirm you're not a bot") to unauthenticated CLI clients.

When this occurs, the player fails silently: the playback engine receives an error, but the status bar continues displaying a stopped state without telling the listener whether the issue is an offline internet connection, a blocked track, or an authentication failure. Furthermore, attempting to support authentication introduced architectural issues: credentials were placed in disposable cache locations, process argument formatting was coupled into the disk cache module, browser selection was hardcoded to Safari (which can trigger Full Disk Access permission hurdles on macOS), and slow, unmocked live network calls were introduced into the standard offline test suite.

Listeners need `ytm` to stream and prefetch tracks reliably across platforms, safely extract browser session credentials when needed, clearly distinguish offline internet issues from streaming failures in the terminal UI, and maintain a fast, hermetic, 100% offline test suite.

## Solution

Provide a clean, modular stream authentication and process-runner architecture for `ytm`:

1. **Dedicated Authentication & Process Configuration Module**: Move credential and browser extraction out of the disk cache module into a specialized subsystem. Support automatic browser session detection with a graceful fallback chain, custom cookie files in standard user configuration directories (`~/.config/ytm/cookies.txt`), and explicit environment variable overrides (`YTM_COOKIES_FROM_BROWSER`, `YTM_COOKIES_FILE`).
2. **Resilient Playback & User-Facing Error Diagnostics**: Teach the player event loop to handle playback backend error events (`EndFile` with error reasons). Instead of hanging silently in the status bar, display descriptive, non-scrolling error indicators (e.g., `[!] Offline - check network` or `[!] Stream Error - skipping to next track`), and automatically advance to the next track in the playback queue when streaming fails.
3. **Hermetic Test Suite with Guarded Integration Seams**: Ensure all standard unit and behavioral tests execute completely offline using mock IPC and simulated command runners. Move all real network-hitting integration tests behind explicit test flags so standard automated testing remains deterministic and fast.

## User Stories

1. As a listener on macOS, I want `ytm` to seamlessly play uncached tracks without manual setup, so that I can listen to new music immediately without friction.
2. As a listener who uses Chrome, Brave, Firefox, or Edge, I want `ytm` to automatically detect my active browser session or allow me to configure my browser via `YTM_COOKIES_FROM_BROWSER`, so that I am not forced to use Safari.
3. As a listener on Linux or non-macOS systems, I want `ytm` to gracefully detect supported browsers (such as Chrome or Firefox) rather than assuming Safari is installed, so that streaming works portably.
4. As a privacy-conscious user, I want `ytm` to read custom cookie files from standard configuration locations (`~/.config/ytm/cookies.txt`) rather than disposable cache directories, so that disk cleaning tools do not delete my credentials.
5. As a listener whose internet connection drops, I want the status bar to show `[!] Offline` instead of silently freezing, so that I immediately know the problem is my network.
6. As a listener who encounters a region-blocked or deleted track, I want `ytm` to display a brief error indicator and advance to the next track in the playback queue, so that playback does not halt indefinitely in silence.
7. As a listener using in-track seeking on a streaming track, I want seeking errors at EOF to be handled gracefully without triggering an unrecoverable player crash.
8. As a developer running tests offline, I want the full test suite to pass in milliseconds without making external HTTP requests, so that my development workflow is fast and reliable.
9. As a developer testing streaming resilience, I want dedicated integration tests guarded by test attributes that I can run on demand when testing real YouTube backend interactions.
10. As a listener inspecting prefetch activity, I want background prefetch download errors to be recorded cleanly in an internal diagnostics buffer without dumping error text across the active raw terminal display.
11. As a listener who restarts `ytm` after an error, I want the initial paused state on Track 1 to reflect readiness cleanly without leftover error states from previous runs.
12. As a terminal user, I want error notifications in the status bar to respect terminal column boundaries and avoid pushing lines into scrollback, so that my terminal history remains completely pristine.
13. As a listener using the playlist viewer modal, I want tracks that previously encountered stream errors to be distinguishable from cached tracks, so that I know which tracks require a network connection.
14. As a listener switching between mobile hotspot and home Wi-Fi, I want streaming requests to re-evaluate connection health without requiring an application restart.
15. As a terminal user pressing `q` or `Ctrl+C` while a stream error is displaying, I want the application to terminate cleanly, restoring the cursor and shutting down all background helper processes.

## Implementation Decisions

- **Domain Module Separation**:
  - The local audio cache module remains strictly focused on disk caching: LRU eviction, file existence, and file path resolution for cached audio files.
  - A dedicated authentication and process configuration module is introduced to handle cookie resolution, browser inspection, and CLI flag formatting for helper processes.
  - A centralized helper command builder encapsulates the construction of external media extraction commands, eliminating duplicated argument assembly and error mapping.

- **Browser & Credential Resolution Hierarchy**:
  1. `YTM_COOKIES_FILE` environment variable pointing to an existing file.
  2. `YTM_COOKIES_FROM_BROWSER` environment variable specifying a supported browser identifier.
  3. User configuration directory file (`~/.config/ytm/cookies.txt` or `$XDG_CONFIG_HOME/ytm/cookies.txt`).
  4. Platform-aware auto-detection: On macOS, verify Safari/Chrome/Firefox availability before selecting; on Linux, inspect common browser profiles.
  5. Fallback unauthenticated mode: If no credentials exist, attempt unauthenticated streaming and report clear diagnostics if bot verification is encountered.

- **Playback Error Handling & Event Loop State Machine**:
  - The playback controller translates backend error events (`end-file` with reason `error`) into structured internal events.
  - The application coordinator captures playback failures and updates the status state with a temporary error notification.
  - When an uncached track fails to play, the coordinator logs the failure, marks the track as unplayable for the current session, and automatically advances the queue cursor to the next playable track.
  - The status bar formatter incorporates an optional error segment that overwrites within the existing 2-line footprint, strictly adhering to terminal width limits and avoiding scrollback dumps.

- **Background Prefetch Resilience**:
  - Prefetch worker errors (such as HTTP 403 or bot verification failures) are swallowed silently from stdout/stderr, captured in an in-memory status record, and exposed to the status bar only when the active track is affected.

## Testing Decisions

- **Black-box Behavioral Testing at High Seams**:
  - Test external behavior from the application coordinator and status bar level rather than asserting on private internal structs.
  - Test that backend error events trigger queue advancement and update the rendered status lines with proper error states.
  - Test that credential resolvers produce valid process argument lists for browser variants and file variants.
- **Hermetic Mocking & Offline Fast Path**:
  - The default test suite must rely exclusively on mock IPC streams and mock downloaders, completing under 3 seconds with zero external network access.
  - Prior art: Existing mock responder threads in the test suite using Unix stream pairs for MPV IPC simulation.
- **Guarded Network Tests**:
  - Live network tests verifying real-world YouTube stream extraction and playback must be marked with ignore attributes, requiring explicit opt-in flags to execute.

## Out of Scope

- Developing custom web scrapers or reverse-engineering proprietary YouTube DRM.
- Graphical browser login prompts, OAuth popups, or embedded WebView authentication workflows.
- Permanent storage of user passwords or plaintext login credentials.
- Multi-channel visualizers or video playback rendering.

## Further Notes

- This specification resolves the architectural and scope concerns identified in Issue 07 and code review of commit `68057ba`, providing a long-term maintainable architecture for streaming and caching YouTube audio.
