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

    queue.toggle_loop();
    assert!(!queue.single_loop);

    // Now advances to track 1
    assert_eq!(queue.next().unwrap().id, "id_001");
    assert_eq!(queue.cursor, 1);
    assert_eq!(queue.natural_index(), Some(1));
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
    assert_eq!(&queue.queue[0..3], &[0, 1, 2]);

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
        let upcoming = &queue.queue[5..];
        upcoming[0]
    } else {
        queue.queue[5]
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
    assert!(empty.queue.is_empty());
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
