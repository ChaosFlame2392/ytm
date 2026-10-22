# 04: Queue Selection Set & Custom Playback Reordering

**What to build:** Inside the floating queue modal, listeners can cherry-pick a temporary sequence of songs using `Space`, which toggles selection marks (`[x]` vs `[ ]`) on any track. Pressing `c` clears all current selections at once. When `Enter` is pressed with one or more tracks marked, `ytm` reconstructs the active playback queue so that the marked tracks are scheduled at the front in the exact order they were picked, followed by the remaining playlist tracks. Playback immediately transitions to the first selected track, the prefetcher is updated to cache the next track, and the modal closes back to the 2-line status bar. If no tracks are marked, `Enter` preserves the direct single-track jump behavior.

**Blocked by:** 06-2: NVIM-Style Floating Queue Modal & Direct Track Jump

**Status:** ready-for-agent

- [ ] Pressing `Space` in the modal toggles selection marking (`[x]` / `[ ]`) on the hovered track.
- [ ] Tracks in the selection set preserve their picked order.
- [ ] Pressing `c` unmarks all selected tracks and empties the selection set.
- [ ] Visual indicators distinguish selected tracks (`[x]`) from unselected tracks (`[ ]`) in the list.
- [ ] Pressing `Enter` with selections rebuilds the queue (selected tracks first in pick order, followed by remaining tracks).
- [ ] Playback transitions immediately to the first selected track and updates the prefetcher.
- [ ] Pressing `Enter` with an empty selection set directly jumps to the single hovered track.
- [ ] Unit tests in `tests/queue_test.rs` verify queue reordering with selection sets.
- [ ] Integration tests in `tests/app_test.rs` verify selection toggling, queue rebuild, and playback transition.
