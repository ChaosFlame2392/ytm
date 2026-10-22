# ytm (Terminal YouTube Music Streamer)

A native, ultra-lightweight terminal client for streaming and caching YouTube Music playlists with zero memory pressure.

## Playback & Queue

**Track**:
A single playable audio item defined by a YouTube video ID and track title.
_Avoid_: Song, video, media item

**Playlist**:
The ordered collection of tracks retrieved from a YouTube playlist URL.
_Avoid_: Album, tracklist

**Natural Order**:
The original sequential position of a track in the source playlist.
_Avoid_: Default index, original order

**Playback Queue**:
The active sequence and cursor specifying the current and upcoming playback order.
_Avoid_: Playhead, tracklist queue

**Iceberg Playback**:
The workflow of starting playback from the latest added tracks at the top of a playlist and looping back on demand.
_Avoid_: Top-looping, recent-first playback

## Navigation & Viewer (Proposed)

**In-Track Seeking**:
Moving the playback position forward or backward within the currently playing track.
_Avoid_: Fast-forward/rewind, scrubbing, track jumping

**Seek Step**:
The fixed duration in seconds applied to each relative seek action.
_Avoid_: Seek interval, scrub increment

**Playlist Viewer / Floating Queue Modal**:
An interactive, NVIM-style bordered floating window displaying the active playback queue with viewport scrolling.
_Avoid_: Song browser, curses menu, track picker, pop-up

**Viewer Cursor**:
The scrollable selection point within the floating queue modal, independent of the active playback track.
_Avoid_: Hover index, browse pointer, highlight bar

**Queue Order Cycle**:
The cyclic reordering of the playback queue via `r` through distinct states: Original, Random, Re-centered Sequential, and Re-rolled Random.
_Avoid_: Shuffle mode, random toggle

**Initial Paused State**:
The startup state where track 1 is loaded but playback remains paused until explicitly started.
_Avoid_: Pre-play, standby mode

**Modal Search Filter**:
An interactive query input within the floating queue modal that live-filters displayed tracks by title matching.
_Avoid_: Search bar, query box, finder

**Search Resolution**:
Exiting the search filter upon pressing `Enter` by locating and centering the selected matching track within the full active queue context.
_Avoid_: Search commit, match jump

**Queue Selection Set**:
The temporary collection of tracks marked with `Space` within the floating queue modal to construct a custom playback sequence.
_Avoid_: Multi-select list, staged tracks, playlist basket

**Range Loop**:
Restricting automatic track progression to repeat within a bounded subset of tracks rather than the full playlist or a single track.
_Avoid_: Section loop, A-B repeat, queue loop


