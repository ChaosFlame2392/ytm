use ytm::{Queue, Track};

fn make_tracks(count: usize) -> Vec<Track> {
    (0..count)
        .map(|i| Track {
            index: i,
            id: format!("id_{:03}", i),
            title: format!("Track {:03}", i),
        })
        .collect()
}

#[test]
fn test_queue_initial_state() {
    let tracks = make_tracks(5);
    let queue = Queue::new(tracks);

    assert_eq!(queue.cursor, 0);
    assert_eq!(queue.natural_index(), Some(0));
    assert_eq!(queue.current_track().unwrap().id, "id_000");
    assert_eq!(queue.peek_next().unwrap().id, "id_001");
    assert!(!queue.shuffle);
    assert!(!queue.single_loop);
}

#[test]
fn test_natural_playback_and_wrap_around() {
    let mut queue = Queue::new(make_tracks(3));

    assert_eq!(queue.current_track().unwrap().id, "id_000");
    assert_eq!(queue.peek_next().unwrap().id, "id_001");

    assert_eq!(queue.next().unwrap().id, "id_001");
    assert_eq!(queue.peek_next().unwrap().id, "id_002");

    assert_eq!(queue.next().unwrap().id, "id_002");
    assert_eq!(queue.peek_next().unwrap().id, "id_000"); // wraps to 0

    assert_eq!(queue.next().unwrap().id, "id_000");
    assert_eq!(queue.cursor, 0);

    // prev() moves backward or clamps to 0
    assert_eq!(queue.prev().unwrap().id, "id_000");
    assert_eq!(queue.cursor, 0);

    queue.next();
    assert_eq!(queue.cursor, 1);
    assert_eq!(queue.prev().unwrap().id, "id_000");
    assert_eq!(queue.cursor, 0);
}

#[test]
fn test_single_track_loop() {
    let mut queue = Queue::new(make_tracks(3));

    queue.toggle_loop();
    assert!(queue.single_loop);

    // Calling next() repeatedly stays on track 0
    for _ in 0..5 {
        assert_eq!(queue.next().unwrap().id, "id_000");
        assert_eq!(queue.cursor, 0);
        assert_eq!(queue.natural_index(), Some(0));
    }

    // peek_next() still returns the upcoming track 1
    assert_eq!(queue.peek_next().unwrap().id, "id_001");

    // skip_next() manually advances past the loop
    assert_eq!(queue.skip_next().unwrap().id, "id_001");
    assert_eq!(queue.cursor, 1);
    assert_eq!(queue.natural_index(), Some(1));

    // now next() loops on track 1
    assert_eq!(queue.next().unwrap().id, "id_001");
    assert_eq!(queue.cursor, 1);

    queue.toggle_loop();
    assert!(!queue.single_loop);

    // Now advances to track 2
    assert_eq!(queue.next().unwrap().id, "id_002");
    assert_eq!(queue.cursor, 2);
    assert_eq!(queue.natural_index(), Some(2));
}

#[test]
fn test_shuffle_toggle_workflow_and_iceberg_reset() {
    let n = 25;
    let mut queue = Queue::new(make_tracks(n));

    // 1. Advance naturally 3 times: 0 -> 1 -> 2 -> 3
    queue.next();
    queue.next();
    queue.next();
    assert_eq!(queue.cursor, 3);
    assert_eq!(queue.natural_index(), Some(3));
    assert_eq!(queue.current_track().unwrap().id, "id_003");

    // 2. Toggle shuffle ON
    assert!(queue.toggle_shuffle());
    assert!(queue.shuffle);
    // Preserves current track and position
    assert_eq!(queue.cursor, 3);
    assert_eq!(queue.natural_index(), Some(3));
    assert_eq!(queue.current_track().unwrap().id, "id_003");
    // Preserves played history
    assert_eq!(&queue.playback_order[0..3], &[0, 1, 2]);

    // 3. Advance in shuffled queue
    let next_track_index = queue.next().expect("shuffled track").index;
    assert_eq!(queue.cursor, 4);
    let next_natural = queue.natural_index().unwrap();
    assert_eq!(next_track_index, next_natural);

    // 4. Toggle shuffle OFF (unshuffle)
    assert!(!queue.toggle_shuffle());
    assert!(!queue.shuffle);
    // Still on currently playing track
    assert_eq!(queue.cursor, 4);
    assert_eq!(queue.natural_index(), Some(next_natural));
    assert_eq!(queue.current_track().unwrap().index, next_natural);

    // The upcoming tracks must start at natural_index + 1
    let expected_next_idx = if next_natural + 1 < n {
        // Look for the next natural index among remaining upcoming
        let upcoming = &queue.playback_order[5..];
        upcoming[0]
    } else {
        queue.playback_order[5]
    };
    assert_eq!(queue.peek_next().unwrap().index, expected_next_idx);

    // 5. Test iceberg reset to top (`r`)
    queue.reset_to_top();
    assert_eq!(queue.cursor, 0);
    assert_eq!(queue.natural_index(), Some(0));
    assert_eq!(queue.current_track().unwrap().id, "id_000");
    assert_eq!(queue.peek_next().unwrap().id, "id_001");
}

#[test]
fn test_empty_and_single_track_behavior() {
    let mut empty = Queue::default();
    assert!(empty.tracks.is_empty());
    assert!(empty.playback_order.is_empty());
    assert_eq!(empty.cursor, 0);
    assert_eq!(empty.natural_index(), None);
    assert_eq!(empty.current_track(), None);
    assert_eq!(empty.peek_next(), None);
    assert_eq!(empty.next(), None);
    assert_eq!(empty.prev(), None);

    // Toggling shuffle/loop on empty does not crash
    assert!(empty.toggle_shuffle());
    assert!(!empty.toggle_shuffle());
    assert!(empty.toggle_loop());
    assert!(!empty.toggle_loop());
    empty.reset_to_top();
    assert_eq!(empty.current_track(), None);

    // Single track queue
    let mut single = Queue::new(make_tracks(1));
    assert_eq!(single.cursor, 0);
    assert_eq!(single.natural_index(), Some(0));
    assert_eq!(single.current_track().unwrap().id, "id_000");
    assert_eq!(single.peek_next().unwrap().id, "id_000");

    assert_eq!(single.next().unwrap().id, "id_000");
    assert_eq!(single.prev().unwrap().id, "id_000");

    assert!(single.toggle_shuffle());
    assert_eq!(single.current_track().unwrap().id, "id_000");
    assert!(!single.toggle_shuffle());
    assert_eq!(single.current_track().unwrap().id, "id_000");
}

#[test]
fn test_queue_jump_to_natural_and_shuffled_and_bounds() {
    let mut queue = Queue::new(make_tracks(5));
    assert_eq!(queue.cursor, 0);

    // Jump to index 2 in natural order
    let track = queue.jump_to(2).expect("valid index 2");
    assert_eq!(track.id, "id_002");
    assert_eq!(queue.cursor, 2);
    assert_eq!(queue.natural_index(), Some(2));

    // Jump to index 4 (last item)
    let track = queue.jump_to(4).expect("valid index 4");
    assert_eq!(track.id, "id_004");
    assert_eq!(queue.cursor, 4);
    assert_eq!(queue.natural_index(), Some(4));

    // Out of bounds jump: does not change cursor and returns None
    assert_eq!(queue.jump_to(5), None);
    assert_eq!(queue.cursor, 4);
    assert_eq!(queue.jump_to(100), None);
    assert_eq!(queue.cursor, 4);

    // Jump in shuffled state
    assert!(queue.toggle_shuffle());
    let target_idx = 1;
    let expected_track_id = queue.tracks[queue.playback_order[target_idx]].id.clone();
    let track = queue.jump_to(target_idx).expect("valid jump in shuffled queue");
    assert_eq!(track.id, expected_track_id);
    assert_eq!(queue.cursor, target_idx);

    // Jump out of bounds in shuffled state
    assert_eq!(queue.jump_to(10), None);
    assert_eq!(queue.cursor, target_idx);

    // Edge case: Empty queue
    let mut empty = Queue::default();
    assert_eq!(empty.jump_to(0), None);
    assert_eq!(empty.cursor, 0);
}

#[test]
fn test_queue_reorder_with_selection_pick_order_and_remaining() {
    let mut queue = Queue::new(make_tracks(5));
    // Initial order: [0, 1, 2, 3, 4]
    assert_eq!(queue.playback_order, vec![0, 1, 2, 3, 4]);

    // Pick indices: [3, 1, 4]
    // Tracks at these indices are track 3, track 1, track 4.
    // Remaining unselected tracks in relative order: track 0, track 2.
    let first = queue.reorder_with_selection(&[3, 1, 4]).expect("returns first track");
    assert_eq!(first.id, "id_003");
    assert_eq!(queue.cursor, 0);
    assert_eq!(queue.playback_order, vec![3, 1, 4, 0, 2]);
    assert_eq!(queue.current_track().unwrap().id, "id_003");
    assert_eq!(queue.peek_next().unwrap().id, "id_001");
}

#[test]
fn test_queue_reorder_with_selection_shuffled_state() {
    let mut queue = Queue::new(make_tracks(5));
    queue.shuffle = true;
    queue.playback_order = vec![4, 2, 0, 3, 1];
    queue.cursor = 2; // currently at track 0

    // Selected indices in the active playback order: [3, 1]
    // Index 3 is track 3. Index 1 is track 2.
    // Remaining unselected indices: 0 (track 4), 2 (track 0), 4 (track 1).
    // Expected new playback order: [3, 2, 4, 0, 1].
    let first = queue.reorder_with_selection(&[3, 1]).expect("returns first track");
    assert_eq!(first.id, "id_003");
    assert_eq!(queue.cursor, 0);
    assert_eq!(queue.playback_order, vec![3, 2, 4, 0, 1]);
    assert_eq!(queue.peek_next().unwrap().id, "id_002");
}

#[test]
fn test_queue_reorder_with_selection_edge_cases() {
    // 1. Empty selection set maintains current relative order and resets cursor to 0
    let mut queue = Queue::new(make_tracks(4));
    queue.cursor = 2;
    let first = queue.reorder_with_selection(&[]).expect("returns first track");
    assert_eq!(first.id, "id_000");
    assert_eq!(queue.cursor, 0);
    assert_eq!(queue.playback_order, vec![0, 1, 2, 3]);

    // 2. Out-of-bounds indices and duplicate indices in selection set
    let mut queue2 = Queue::new(make_tracks(4));
    // Index 2 is track 2. Duplicate 2 should be ignored. Index 99 is out-of-bounds.
    let first2 = queue2.reorder_with_selection(&[2, 99, 2, 0]).expect("handles duplicates and OOB");
    assert_eq!(first2.id, "id_002");
    assert_eq!(queue2.cursor, 0);
    // Picked: [2, 0]. Remaining: [1, 3]
    assert_eq!(queue2.playback_order, vec![2, 0, 1, 3]);

    // 3. Empty queue returns None
    let mut empty = Queue::default();
    assert_eq!(empty.reorder_with_selection(&[0, 1]), None);
    assert_eq!(empty.cursor, 0);

    // 4. Single-track queue
    let mut single = Queue::new(make_tracks(1));
    let s_first = single.reorder_with_selection(&[0]).expect("single track");
    assert_eq!(s_first.id, "id_000");
    assert_eq!(single.cursor, 0);
    assert_eq!(single.playback_order, vec![0]);
}

#[test]
fn test_queue_search_indices() {
    let tracks = vec![
        Track { index: 0, id: "0".into(), title: "Bohemian Rhapsody".into() },
        Track { index: 1, id: "1".into(), title: "Starman".into() },
        Track { index: 2, id: "2".into(), title: "Radioactive".into() },
        Track { index: 3, id: "3".into(), title: "Radio Ga Ga".into() },
    ];
    let mut queue = Queue::new(tracks);

    // 1. Empty query returns all item indices in playback_order
    assert_eq!(queue.search_indices(""), vec![0, 1, 2, 3]);

    // 2. Case-insensitive substring search
    assert_eq!(queue.search_indices("RADIO"), vec![2, 3]);
    assert_eq!(queue.search_indices("rhapsody"), vec![0]);
    assert_eq!(queue.search_indices("man"), vec![1]);

    // 3. No match
    assert_eq!(queue.search_indices("Nonexistent"), Vec::<usize>::new());

    // 4. In reordered playback_order, search_indices returns indices into playback_order
    queue.playback_order = vec![3, 1, 0, 2];
    // "Radio" matches playback_order[0] (track 3) and playback_order[3] (track 2)
    assert_eq!(queue.search_indices("Radio"), vec![0, 3]);

    // 5. Empty queue returns empty
    let empty_queue = Queue::default();
    assert_eq!(empty_queue.search_indices("test"), Vec::<usize>::new());
    assert_eq!(empty_queue.search_indices(""), Vec::<usize>::new());
}

