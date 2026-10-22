use crate::playlist::Track;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queue {
    pub tracks: Vec<Track>,
    pub playback_order: Vec<usize>,
    pub cursor: usize,
    pub shuffle: bool,
    pub single_loop: bool,
}

impl Default for Queue {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl Queue {
    pub fn new(tracks: Vec<Track>) -> Self {
        let n = tracks.len();
        Self {
            tracks,
            playback_order: (0..n).collect(),
            cursor: 0,
            shuffle: false,
            single_loop: false,
        }
    }

    pub fn current_track(&self) -> Option<&Track> {
        let idx = *self.playback_order.get(self.cursor)?;
        self.tracks.get(idx)
    }

    /// Returns the upcoming track in playback order without modifying cursor position.
    /// Note: Returns the next track in sequence even when `single_loop` is enabled.
    /// This allows ahead-of-time prefetching and informs the user via UI what plays
    /// when looping is disabled or when the track is manually skipped.
    pub fn peek_next(&self) -> Option<&Track> {
        if self.playback_order.is_empty() {
            return None;
        }
        let next_cursor = if self.cursor + 1 < self.playback_order.len() {
            self.cursor + 1
        } else {
            0
        };
        let idx = *self.playback_order.get(next_cursor)?;
        self.tracks.get(idx)
    }

    pub fn natural_index(&self) -> Option<usize> {
        self.playback_order.get(self.cursor).copied()
    }

    /// Unconditionally advances to the next track in playback order, bypassing `single_loop`.
    /// Used for manual skip actions (e.g. key `>`).
    pub fn skip_next(&mut self) -> Option<&Track> {
        if self.playback_order.is_empty() {
            return None;
        }
        if self.cursor + 1 < self.playback_order.len() {
            self.cursor += 1;
        } else {
            self.cursor = 0;
        }
        self.current_track()
    }

    /// Advances playback for natural track completion (EOF).
    /// If `single_loop` is enabled, repeats the current track without advancing.
    /// Otherwise, advances to the next track in playback order.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<&Track> {
        if self.playback_order.is_empty() {
            return None;
        }
        if self.single_loop {
            return self.current_track();
        }
        self.skip_next()
    }

    pub fn prev(&mut self) -> Option<&Track> {
        if self.playback_order.is_empty() {
            return None;
        }
        if self.cursor > 0 {
            self.cursor -= 1;
        } else {
            self.cursor = 0;
        }
        self.current_track()
    }

    /// Directly jumps the queue cursor to `cursor_index` and returns the track.
    /// Returns None and leaves the cursor unchanged if `cursor_index` is out of bounds.
    pub fn jump_to(&mut self, cursor_index: usize) -> Option<&Track> {
        if cursor_index < self.playback_order.len() {
            self.cursor = cursor_index;
            self.current_track()
        } else {
            None
        }
    }

    /// Reconstructs `playback_order` such that the tracks at `selected_indices` appear
    /// at the front of the queue in the exact order they were selected (pick order),
    /// followed by the remaining unselected tracks in their relative order.
    /// Resets `cursor` to 0 and returns the first track.
    pub fn reorder_with_selection(&mut self, selected_indices: &[usize]) -> Option<&Track> {
        if self.playback_order.is_empty() {
            self.cursor = 0;
            return None;
        }

        let n = self.playback_order.len();
        let mut new_order = Vec::with_capacity(n);
        let mut selected_mask = vec![false; n];

        for &idx in selected_indices {
            if idx < n && !selected_mask[idx] {
                selected_mask[idx] = true;
                new_order.push(self.playback_order[idx]);
            }
        }

        for (idx, &selected) in selected_mask.iter().enumerate() {
            if !selected {
                new_order.push(self.playback_order[idx]);
            }
        }


        self.playback_order = new_order;
        self.cursor = 0;
        self.current_track()
    }

    /// Returns the indices in `playback_order` whose track title matches `query` case-insensitively.
    /// If `query` is empty, returns all indices in `0..playback_order.len()`.
    pub fn search_indices(&self, query: &str) -> Vec<usize> {
        let q = query.to_lowercase();
        self.playback_order
            .iter()
            .enumerate()
            .filter_map(|(item_idx, &track_idx)| {
                let track = self.tracks.get(track_idx)?;
                if q.is_empty() || track.title.to_lowercase().contains(&q) {
                    Some(item_idx)
                } else {
                    None
                }
            })
            .collect()
    }


    pub fn toggle_loop(&mut self) -> bool {
        self.single_loop = !self.single_loop;
        self.single_loop
    }

    pub fn reset_to_top(&mut self) {
        self.cursor = 0;
        if self.tracks.is_empty() {
            self.playback_order.clear();
            return;
        }
        self.playback_order = (0..self.tracks.len()).collect();
        if self.shuffle && self.playback_order.len() > 1 {
            use rand::seq::SliceRandom;
            self.playback_order[1..].shuffle(&mut rand::thread_rng());
        }
    }

    pub fn toggle_shuffle_with_rng<R: rand::Rng + ?Sized>(&mut self, rng: &mut R) -> bool {
        self.shuffle = !self.shuffle;
        if self.shuffle {
            if self.cursor < self.playback_order.len() {
                use rand::seq::SliceRandom;
                self.playback_order[self.cursor + 1..].shuffle(rng);
            }
        } else {
            let n = self.tracks.len();
            if n > 0 && self.cursor < self.playback_order.len() {
                let natural_idx = self.playback_order[self.cursor];
                self.playback_order[self.cursor + 1..].sort_by_key(|&idx| {
                    if idx > natural_idx {
                        idx - natural_idx
                    } else {
                        idx + n - natural_idx
                    }
                });
            }
        }
        self.shuffle
    }

    pub fn toggle_shuffle(&mut self) -> bool {
        self.toggle_shuffle_with_rng(&mut rand::thread_rng())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tracks(n: usize) -> Vec<Track> {
        (0..n)
            .map(|i| Track {
                index: i,
                id: format!("video_{}", i),
                title: format!("Title {}", i),
            })
            .collect()
    }

    #[test]
    fn test_natural_sequential_playback() {
        let tracks = sample_tracks(3);
        let mut queue = Queue::new(tracks);

        assert_eq!(queue.cursor, 0);
        assert_eq!(queue.natural_index(), Some(0));
        assert_eq!(queue.current_track().unwrap().id, "video_0");

        // Advance to 1
        let t1 = queue.next().expect("track 1");
        assert_eq!(t1.id, "video_1");
        assert_eq!(queue.cursor, 1);
        assert_eq!(queue.natural_index(), Some(1));

        // Advance to 2
        let t2 = queue.next().expect("track 2");
        assert_eq!(t2.id, "video_2");
        assert_eq!(queue.cursor, 2);
        assert_eq!(queue.natural_index(), Some(2));

        // Advance past end -> wraps to 0
        let t0 = queue.next().expect("track 0");
        assert_eq!(t0.id, "video_0");
        assert_eq!(queue.cursor, 0);
        assert_eq!(queue.natural_index(), Some(0));

        // prev() from 0 stays at 0
        let p0 = queue.prev().expect("stays at 0");
        assert_eq!(p0.id, "video_0");
        assert_eq!(queue.cursor, 0);

        // Advance to 1 then prev back to 0
        queue.next();
        assert_eq!(queue.cursor, 1);
        let p_prev = queue.prev().expect("back to 0");
        assert_eq!(p_prev.id, "video_0");
        assert_eq!(queue.cursor, 0);
    }

    #[test]
    fn test_empty_and_single_track_edge_cases() {
        // Empty queue
        let mut empty = Queue::default();
        assert_eq!(empty.cursor, 0);
        assert_eq!(empty.current_track(), None);
        assert_eq!(empty.natural_index(), None);
        assert_eq!(empty.next(), None);
        assert_eq!(empty.prev(), None);

        // Single track
        let mut single = Queue::new(sample_tracks(1));
        assert_eq!(single.cursor, 0);
        assert_eq!(single.natural_index(), Some(0));
        assert_eq!(single.current_track().unwrap().id, "video_0");

        // Next wraps back to 0
        assert_eq!(single.next().unwrap().id, "video_0");
        assert_eq!(single.cursor, 0);
        assert_eq!(single.prev().unwrap().id, "video_0");
        assert_eq!(single.cursor, 0);
    }

    #[test]
    fn test_peek_next_behavior() {
        let empty = Queue::default();
        assert_eq!(empty.peek_next(), None);

        let single = Queue::new(sample_tracks(1));
        assert_eq!(single.peek_next().unwrap().id, "video_0");

        let mut queue = Queue::new(sample_tracks(3));
        // At 0, peek_next returns track 1 without advancing cursor
        assert_eq!(queue.peek_next().unwrap().id, "video_1");
        assert_eq!(queue.cursor, 0);

        queue.next();
        assert_eq!(queue.cursor, 1);
        assert_eq!(queue.peek_next().unwrap().id, "video_2");
        assert_eq!(queue.cursor, 1);

        queue.next();
        assert_eq!(queue.cursor, 2);
        // At last element, peek_next wraps to track 0
        assert_eq!(queue.peek_next().unwrap().id, "video_0");
        assert_eq!(queue.cursor, 2);
    }

    #[test]
    fn test_toggle_loop_prevents_advancement() {
        let mut queue = Queue::new(sample_tracks(3));
        assert!(!queue.single_loop);

        // Turn loop on
        assert!(queue.toggle_loop());
        assert!(queue.single_loop);

        // peek_next() still returns upcoming track (track 1)
        assert_eq!(queue.peek_next().unwrap().id, "video_1");

        // next() stays on track 0
        let t0 = queue.next().expect("stays on track 0");
        assert_eq!(t0.id, "video_0");
        assert_eq!(queue.cursor, 0);

        let t0_again = queue.next().expect("still track 0");
        assert_eq!(t0_again.id, "video_0");
        assert_eq!(queue.cursor, 0);

        // skip_next() unconditionally advances to track 1 even with loop on
        let t1_skip = queue.skip_next().expect("skip advances to track 1");
        assert_eq!(t1_skip.id, "video_1");
        assert_eq!(queue.cursor, 1);
        assert!(queue.single_loop);

        // next() now loops on track 1
        let t1_loop = queue.next().expect("loops on track 1");
        assert_eq!(t1_loop.id, "video_1");
        assert_eq!(queue.cursor, 1);

        // Turn loop off
        assert!(!queue.toggle_loop());
        assert!(!queue.single_loop);

        // next() advances to track 2
        let t2 = queue.next().expect("advances to track 2");
        assert_eq!(t2.id, "video_2");
        assert_eq!(queue.cursor, 2);
    }

    #[test]
    fn test_reset_to_top() {
        let mut queue = Queue::new(sample_tracks(5));
        queue.next();
        queue.next();
        queue.next();
        assert_eq!(queue.cursor, 3);
        assert_eq!(queue.natural_index(), Some(3));

        queue.reset_to_top();
        assert_eq!(queue.cursor, 0);
        assert_eq!(queue.natural_index(), Some(0));
        assert_eq!(queue.current_track().unwrap().id, "video_0");

        // Edge case: empty queue
        let mut empty = Queue::default();
        empty.reset_to_top();
        assert_eq!(empty.cursor, 0);
        assert_eq!(empty.current_track(), None);
    }

    #[test]
    fn test_toggle_shuffle_on_randomizes_upcoming_preserving_current() {
        let n = 20;
        let mut queue = Queue::new(sample_tracks(n));
        queue.next();
        queue.next();
        assert_eq!(queue.cursor, 2);
        assert_eq!(queue.natural_index(), Some(2));
        assert_eq!(queue.current_track().unwrap().id, "video_2");

        let before_upcoming = queue.playback_order[3..].to_vec();
        assert_eq!(before_upcoming, (3..n).collect::<Vec<_>>());

        assert!(queue.toggle_shuffle());
        assert!(queue.shuffle);
        // Current track and position preserved
        assert_eq!(queue.cursor, 2);
        assert_eq!(queue.natural_index(), Some(2));
        assert_eq!(queue.current_track().unwrap().id, "video_2");

        // History preserved
        assert_eq!(&queue.playback_order[0..2], &[0, 1]);

        // Upcoming shuffled: same set of elements, but reordered
        let mut after_upcoming_sorted = queue.playback_order[3..].to_vec();
        after_upcoming_sorted.sort();
        assert_eq!(after_upcoming_sorted, (3..n).collect::<Vec<_>>());

        // Extremely low probability (1/17! < 1e-14) of being identical to sorted
        assert_ne!(queue.playback_order[3..], before_upcoming);
    }

    #[test]
    fn test_toggle_shuffle_off_restores_natural_order() {
        // Scenario 1: Start at index 0, scramble upcoming, toggle OFF
        let mut queue = Queue::new(sample_tracks(5));
        queue.shuffle = true;
        queue.playback_order = vec![0, 4, 2, 1, 3];
        queue.cursor = 0;

        assert!(!queue.toggle_shuffle()); // toggled OFF
        assert!(!queue.shuffle);
        assert_eq!(queue.cursor, 0);
        assert_eq!(queue.natural_index(), Some(0));
        assert_eq!(queue.current_track().unwrap().id, "video_0");
        // Upcoming tracks restored in sequential natural order 1..5
        assert_eq!(queue.playback_order, vec![0, 1, 2, 3, 4]);

        // Scenario 2: At track 2 with remaining upcoming scrambled
        let mut queue2 = Queue::new(sample_tracks(6));
        queue2.shuffle = true;
        queue2.cursor = 2;
        queue2.playback_order = vec![0, 1, 2, 5, 3, 4];

        assert!(!queue2.toggle_shuffle()); // toggled OFF
        assert_eq!(queue2.cursor, 2);
        assert_eq!(queue2.natural_index(), Some(2));
        assert_eq!(queue2.current_track().unwrap().id, "video_2");
        // Remaining unplayed upcoming 3, 4, 5 reordered sequentially
        assert_eq!(queue2.playback_order, vec![0, 1, 2, 3, 4, 5]);

        // Scenario 3: Wrap-around case: playing track 4 in 6-track playlist, remaining {1, 5, 0}
        let mut queue3 = Queue::new(sample_tracks(6));
        queue3.shuffle = true;
        queue3.cursor = 2;
        queue3.playback_order = vec![2, 3, 4, 1, 5, 0];
        // At cursor 2, track is 4 (natural_index = 4). Upcoming are 1, 5, 0.
        assert_eq!(queue3.natural_index(), Some(4));

        assert!(!queue3.toggle_shuffle()); // toggled OFF
        assert_eq!(queue3.cursor, 2);
        assert_eq!(queue3.natural_index(), Some(4));
        // Order starting from natural_index + 1 (5), wrapping to 0, 1:
        // Upcoming must be [5, 0, 1]!
        assert_eq!(queue3.playback_order, vec![2, 3, 4, 5, 0, 1]);
        assert_eq!(queue3.peek_next().unwrap().id, "video_5");
    }
}
