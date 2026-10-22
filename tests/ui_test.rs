use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ytm::ui::{
    calculate_modal_rect, enter_alternate_screen, format_line1, format_line2, format_modal_lines,
    format_time, leave_alternate_screen, parse_key_event, parse_modal_key_event,
    render_floating_modal, render_status_bar, render_status_lines, wipe_status_lines, KeyAction,
    ModalKeyAction, ModalRect, ModalState, StatusState,
};
use ytm::{Queue, Track};

#[test]
fn test_format_time_variants() {
    assert_eq!(format_time(None), "--:--");
    assert_eq!(format_time(Some(-1.0)), "--:--");
    assert_eq!(format_time(Some(0.0)), "00:00");
    assert_eq!(format_time(Some(9.4)), "00:09");
    assert_eq!(format_time(Some(59.9)), "00:59");
    assert_eq!(format_time(Some(60.0)), "01:00");
    assert_eq!(format_time(Some(125.0)), "02:05");
    assert_eq!(format_time(Some(3599.0)), "59:59");
    assert_eq!(format_time(Some(3661.0)), "61:01");
}

#[test]
fn test_format_line1_playing_and_cached() {
    let line = format_line1(false, "Bohemian Rhapsody", Some(65.0), Some(354.0), true);
    assert_eq!(
        line,
        "[▶] Bohemian Rhapsody (01:05 / 05:54)  [⚡ Cached]"
    );
}

#[test]
fn test_format_line1_paused_and_streaming() {
    let line = format_line1(true, "Starman", None, Some(250.0), false);
    assert_eq!(
        line,
        "[⏸] Starman (--:-- / 04:10)  [🌐 Stream]"
    );
}

#[test]
fn test_format_line1_no_duration() {
    let line = format_line1(false, "Live Stream Track", Some(12.0), None, false);
    assert_eq!(
        line,
        "[▶] Live Stream Track (00:12 / --:--)  [🌐 Stream]"
    );
}

#[test]
fn test_format_line2_defaults() {
    let line = format_line2(false, false, Some("Heroes"));
    assert_eq!(
        line,
        "[Shuffle: OFF] [Loop: OFF]  Next: Heroes"
    );
}

#[test]
fn test_format_line2_shuffle_and_loop_on() {
    let line = format_line2(true, true, Some("Life on Mars"));
    assert_eq!(
        line,
        "[Shuffle: ON] [Loop: ON]  Next: Life on Mars"
    );
}

#[test]
fn test_format_line2_no_next_track() {
    let line = format_line2(false, false, None);
    assert_eq!(
        line,
        "[Shuffle: OFF] [Loop: OFF]  Next: None"
    );
}

#[test]
fn test_status_state_helper() {
    let state = StatusState {
        is_paused: false,
        title: "Track A",
        position: Some(30.0),
        duration: Some(180.0),
        is_cached: true,
        shuffle: true,
        single_loop: false,
        next_title: Some("Track B"),
    };
    assert_eq!(state.format_line1(), "[▶] Track A (00:30 / 03:00)  [⚡ Cached]");
    assert_eq!(state.format_line2(), "[Shuffle: ON] [Loop: OFF]  Next: Track B");
    let (l1, l2) = state.format_lines();
    assert_eq!(l1, "[▶] Track A (00:30 / 03:00)  [⚡ Cached]");
    assert_eq!(l2, "[Shuffle: ON] [Loop: OFF]  Next: Track B");
}

#[test]
fn test_render_status_bar_and_wipe_ansi() {
    let mut buf = Vec::new();
    render_status_lines(&mut buf, "Line 1 Content", "Line 2 Content").unwrap();
    let rendered = String::from_utf8(buf).unwrap();
    // Must contain carriage return \r, line clear \x1b[2K, newline \n, and move up \x1b[1A\r
    assert_eq!(
        rendered,
        "\r\x1b[2KLine 1 Content\n\r\x1b[2KLine 2 Content\x1b[1A\r"
    );

    let state = StatusState {
        is_paused: false,
        title: "Track 1",
        position: Some(10.0),
        duration: Some(60.0),
        is_cached: false,
        shuffle: false,
        single_loop: false,
        next_title: None,
    };
    let mut state_buf = Vec::new();
    render_status_bar(&mut state_buf, &state).unwrap();
    let state_rendered = String::from_utf8(state_buf).unwrap();
    assert!(state_rendered.contains("[▶] Track 1 (00:10 / 01:00)  [🌐 Stream]"));
    assert!(state_rendered.contains("[Shuffle: OFF] [Loop: OFF]  Next: None"));

    let mut wipe_buf = Vec::new();
    wipe_status_lines(&mut wipe_buf).unwrap();
    let wiped = String::from_utf8(wipe_buf).unwrap();
    assert_eq!(
        wiped,
        "\r\x1b[2K\n\r\x1b[2K\x1b[1A\r"
    );
}

fn make_key_event(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::empty(),
    }
}

#[test]
fn test_parse_key_event_bindings() {
    // Space: Pause/Resume
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char(' '), KeyModifiers::empty())),
        Some(KeyAction::TogglePause)
    );

    // Next track: >, .
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('>'), KeyModifiers::empty())),
        Some(KeyAction::NextTrack)
    );
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('.'), KeyModifiers::empty())),
        Some(KeyAction::NextTrack)
    );

    // Prev track: <, ,
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('<'), KeyModifiers::empty())),
        Some(KeyAction::PreviousTrack)
    );
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char(','), KeyModifiers::empty())),
        Some(KeyAction::PreviousTrack)
    );

    // Seek backward: h, H, Left
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('h'), KeyModifiers::empty())),
        Some(KeyAction::SeekBackward)
    );
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('H'), KeyModifiers::empty())),
        Some(KeyAction::SeekBackward)
    );
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Left, KeyModifiers::empty())),
        Some(KeyAction::SeekBackward)
    );

    // Seek forward: l, L, Right
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('l'), KeyModifiers::empty())),
        Some(KeyAction::SeekForward)
    );
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('L'), KeyModifiers::empty())),
        Some(KeyAction::SeekForward)
    );
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Right, KeyModifiers::empty())),
        Some(KeyAction::SeekForward)
    );

    // Shuffle: s, S
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('s'), KeyModifiers::empty())),
        Some(KeyAction::ToggleShuffle)
    );
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('S'), KeyModifiers::empty())),
        Some(KeyAction::ToggleShuffle)
    );

    // Reset to top: r, R
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('r'), KeyModifiers::empty())),
        Some(KeyAction::ResetToTop)
    );
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('R'), KeyModifiers::empty())),
        Some(KeyAction::ResetToTop)
    );

    // Quit: q, Q, Ctrl+C
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('q'), KeyModifiers::empty())),
        Some(KeyAction::Quit)
    );
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('Q'), KeyModifiers::empty())),
        Some(KeyAction::Quit)
    );
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Some(KeyAction::Quit)
    );
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('C'), KeyModifiers::CONTROL)),
        Some(KeyAction::Quit)
    );

    // Ignored keys
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('x'), KeyModifiers::empty())),
        None
    );
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Enter, KeyModifiers::empty())),
        None
    );

    // Release events should be ignored
    let mut release_ev = make_key_event(KeyCode::Char(' '), KeyModifiers::empty());
    release_ev.kind = KeyEventKind::Release;
    assert_eq!(parse_key_event(release_ev), None);
}

#[test]
fn test_truncate_to_width_cases() {
    use ytm::ui::truncate_to_width;

    assert_eq!(truncate_to_width("Hello", 0), "");
    assert_eq!(truncate_to_width("Hello", 5), "Hello");
    assert_eq!(truncate_to_width("Hello", 10), "Hello");
    assert_eq!(truncate_to_width("Hello World", 5), "Hell…");
    assert_eq!(truncate_to_width("Hello World", 8), "Hello W…");
    assert_eq!(truncate_to_width("", 5), "");
}

#[test]
fn test_signal_handler_registration() {
    use ytm::ui::setup_signal_handler;
    let signal = setup_signal_handler().expect("Failed to register signal handler");
    // Initially false
    assert!(!signal.load(std::sync::atomic::Ordering::Relaxed));
}

#[test]
fn test_terminal_guard_leave_and_wipe() {
    let mut wipe_buf = Vec::new();
    ytm::ui::wipe_status_lines(&mut wipe_buf).unwrap();
    assert_eq!(wipe_buf, b"\r\x1b[2K\n\r\x1b[2K\x1b[1A\r");
}

#[test]
fn test_terminal_guard_init_screen_clear_and_cursor() {
    let mut buf = Vec::new();
    ytm::ui::init_screen(&mut buf).unwrap();
    let rendered = String::from_utf8(buf).unwrap();
    assert!(rendered.contains("\x1b[2J"), "Should clear entire screen");
    assert!(
        rendered.contains("\x1b[1;1H") || rendered.contains("\x1b[H"),
        "Should reposition cursor to (0, 0)"
    );
    assert!(rendered.contains("\x1b[?25l"), "Should hide cursor");
}

#[test]
fn test_modal_state_dimensions_calculation() {
    // 100 cols x 40 rows: ~80% width = 80, ~70% height = 28
    let rect = calculate_modal_rect(100, 40);
    assert_eq!(rect.width, 80);
    assert_eq!(rect.height, 28);
    assert_eq!(rect.x, 10);
    assert_eq!(rect.y, 6);
    assert_eq!(rect.content_width(), 78);
    assert_eq!(rect.content_height(), 26);

    // 80 cols x 24 rows: ~80% width = 64, ~70% height = 16
    let rect = calculate_modal_rect(80, 24);
    assert_eq!(rect.width, 64);
    assert_eq!(rect.height, 16);
    assert_eq!(rect.x, 8);
    assert_eq!(rect.y, 4);
    assert_eq!(rect.content_width(), 62);
    assert_eq!(rect.content_height(), 14);

    // Small dimensions clamp without underflow
    let rect_small = calculate_modal_rect(15, 6);
    assert!(rect_small.width <= 15);
    assert!(rect_small.height <= 6);
}

#[test]
fn test_modal_state_open_and_centering() {
    let mut modal = ModalState::default();
    assert!(!modal.is_open);
    assert_eq!(modal.viewer_cursor, 0);
    assert_eq!(modal.viewport_offset, 0);

    // Opening on item 0 with 30 items, content_height 10:
    // Viewport remains at 0
    modal.open(0, 30, 10);
    assert!(modal.is_open);
    assert_eq!(modal.viewer_cursor, 0);
    assert_eq!(modal.viewport_offset, 0);

    // Opening on item 14 with 30 items, content_height 10:
    // 14 - 10/2 = 9. Viewport offset is centered at 9.
    modal.open(14, 30, 10);
    assert_eq!(modal.viewer_cursor, 14);
    assert_eq!(modal.viewport_offset, 9);

    // Opening near the bottom: item 28 with 30 items, content_height 10:
    // 28 - 5 = 23, but 23 + 10 > 30, clamped to 30 - 10 = 20.
    modal.open(28, 30, 10);
    assert_eq!(modal.viewer_cursor, 28);
    assert_eq!(modal.viewport_offset, 20);

    // Edge case: total items <= content_height
    modal.open(2, 5, 10);
    assert_eq!(modal.viewer_cursor, 2);
    assert_eq!(modal.viewport_offset, 0);

    // Edge case: empty queue
    modal.open(0, 0, 10);
    assert_eq!(modal.viewer_cursor, 0);
    assert_eq!(modal.viewport_offset, 0);

    modal.close();
    assert!(!modal.is_open);
}

#[test]
fn test_modal_state_viewport_scrolling_navigation() {
    let mut modal = ModalState::default();
    let total_items = 25;
    let content_height = 8;

    modal.open(0, total_items, content_height);
    assert_eq!(modal.viewer_cursor, 0);
    assert_eq!(modal.viewport_offset, 0);

    // Move cursor down within initial viewport (indices 0..7)
    for expected_cursor in 1..8 {
        modal.move_cursor_down(total_items, content_height);
        assert_eq!(modal.viewer_cursor, expected_cursor);
        assert_eq!(modal.viewport_offset, 0, "Viewport should not scroll while cursor < 8");
    }

    // Moving down to index 8 should scroll viewport offset to 1
    modal.move_cursor_down(total_items, content_height);
    assert_eq!(modal.viewer_cursor, 8);
    assert_eq!(modal.viewport_offset, 1);

    // Advance all the way to last item (24)
    for _ in 9..=24 {
        modal.move_cursor_down(total_items, content_height);
    }
    assert_eq!(modal.viewer_cursor, 24);
    assert_eq!(modal.viewport_offset, 25 - 8); // 17

    // Moving down past end does not advance
    modal.move_cursor_down(total_items, content_height);
    assert_eq!(modal.viewer_cursor, 24);
    assert_eq!(modal.viewport_offset, 17);

    // Move up until scrolling starts
    for _ in 0..7 {
        modal.move_cursor_up(content_height);
    }
    // Cursor is now at 17 (= viewport_offset)
    assert_eq!(modal.viewer_cursor, 17);
    assert_eq!(modal.viewport_offset, 17);

    // Moving up to 16 scrolls viewport to 16
    modal.move_cursor_up(content_height);
    assert_eq!(modal.viewer_cursor, 16);
    assert_eq!(modal.viewport_offset, 16);

    // Move up to 0
    for _ in 0..16 {
        modal.move_cursor_up(content_height);
    }
    assert_eq!(modal.viewer_cursor, 0);
    assert_eq!(modal.viewport_offset, 0);

    // Moving up at 0 stays at 0
    modal.move_cursor_up(content_height);
    assert_eq!(modal.viewer_cursor, 0);
    assert_eq!(modal.viewport_offset, 0);
}

#[test]
fn test_modal_key_event_parsing() {
    // Normal mode Tab opens modal
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Tab, KeyModifiers::empty())),
        Some(KeyAction::OpenModal)
    );

    // Modal mode keys
    assert_eq!(
        parse_modal_key_event(make_key_event(KeyCode::Char('j'), KeyModifiers::empty())),
        Some(ModalKeyAction::CursorDown)
    );
    assert_eq!(
        parse_modal_key_event(make_key_event(KeyCode::Char('J'), KeyModifiers::empty())),
        Some(ModalKeyAction::CursorDown)
    );
    assert_eq!(
        parse_modal_key_event(make_key_event(KeyCode::Down, KeyModifiers::empty())),
        Some(ModalKeyAction::CursorDown)
    );

    assert_eq!(
        parse_modal_key_event(make_key_event(KeyCode::Char('k'), KeyModifiers::empty())),
        Some(ModalKeyAction::CursorUp)
    );
    assert_eq!(
        parse_modal_key_event(make_key_event(KeyCode::Char('K'), KeyModifiers::empty())),
        Some(ModalKeyAction::CursorUp)
    );
    assert_eq!(
        parse_modal_key_event(make_key_event(KeyCode::Up, KeyModifiers::empty())),
        Some(ModalKeyAction::CursorUp)
    );

    // Commit jump: Enter
    assert_eq!(
        parse_modal_key_event(make_key_event(KeyCode::Enter, KeyModifiers::empty())),
        Some(ModalKeyAction::Commit)
    );

    // Close modal: Tab, Esc, q, Q
    assert_eq!(
        parse_modal_key_event(make_key_event(KeyCode::Tab, KeyModifiers::empty())),
        Some(ModalKeyAction::Close)
    );
    assert_eq!(
        parse_modal_key_event(make_key_event(KeyCode::Esc, KeyModifiers::empty())),
        Some(ModalKeyAction::Close)
    );
    assert_eq!(
        parse_modal_key_event(make_key_event(KeyCode::Char('q'), KeyModifiers::empty())),
        Some(ModalKeyAction::Close)
    );
    assert_eq!(
        parse_modal_key_event(make_key_event(KeyCode::Char('Q'), KeyModifiers::empty())),
        Some(ModalKeyAction::Close)
    );

    // Space toggles selection
    assert_eq!(
        parse_modal_key_event(make_key_event(KeyCode::Char(' '), KeyModifiers::empty())),
        Some(ModalKeyAction::ToggleSelection)
    );

    // c and C clear selection
    assert_eq!(
        parse_modal_key_event(make_key_event(KeyCode::Char('c'), KeyModifiers::empty())),
        Some(ModalKeyAction::ClearSelection)
    );
    assert_eq!(
        parse_modal_key_event(make_key_event(KeyCode::Char('C'), KeyModifiers::empty())),
        Some(ModalKeyAction::ClearSelection)
    );

    // Quit app in modal: Ctrl+C (takes precedence over clear selection)
    assert_eq!(
        parse_modal_key_event(make_key_event(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Some(ModalKeyAction::Quit)
    );

    // Unmapped keys
    assert_eq!(
        parse_modal_key_event(make_key_event(KeyCode::Char('x'), KeyModifiers::empty())),
        None
    );

    // Release events ignored
    let mut rel = make_key_event(KeyCode::Char('j'), KeyModifiers::empty());
    rel.kind = KeyEventKind::Release;
    assert_eq!(parse_modal_key_event(rel), None);
}

#[test]
fn test_modal_state_selection_set_pick_order_and_clear() {
    let mut modal = ModalState::default();
    assert!(modal.selection_set.is_empty());
    assert!(!modal.is_selected(0));

    // Select index 2, then 4, then 1 (pick order: 2, 4, 1)
    modal.toggle_selection(2);
    assert_eq!(modal.selection_set, vec![2]);
    assert!(modal.is_selected(2));
    assert!(!modal.is_selected(4));

    modal.toggle_selection(4);
    modal.toggle_selection(1);
    assert_eq!(modal.selection_set, vec![2, 4, 1]);
    assert!(modal.is_selected(2));
    assert!(modal.is_selected(4));
    assert!(modal.is_selected(1));
    assert!(!modal.is_selected(3));

    // Toggle 4 again -> unmarks and removes 4 from selection set
    modal.toggle_selection(4);
    assert_eq!(modal.selection_set, vec![2, 1]);
    assert!(!modal.is_selected(4));

    // Clear all selections with 'c'
    modal.clear_selection();
    assert!(modal.selection_set.is_empty());
    assert!(!modal.is_selected(2));
    assert!(!modal.is_selected(1));
}

#[test]
fn test_modal_rendering_with_selection_indicators() {
    let tracks = vec![
        Track {
            index: 0,
            id: "id_0".to_string(),
            title: "Track Zero".to_string(),
        },
        Track {
            index: 1,
            id: "id_1".to_string(),
            title: "Track One".to_string(),
        },
        Track {
            index: 2,
            id: "id_2".to_string(),
            title: "Track Two".to_string(),
        },
    ];
    let queue = Queue::new(tracks);
    let rect = ModalRect {
        x: 0,
        y: 0,
        width: 40,
        height: 5, // 1 header, 3 content, 1 footer
    };
    let mut modal = ModalState::default();
    modal.open(0, 3, rect.content_height());

    // Mark track 0 and track 2
    modal.toggle_selection(0);
    modal.toggle_selection(2);

    let lines = format_modal_lines(&rect, &modal, &queue);
    // Line 1: Track 0 is active (▶) AND selected ([x])
    assert!(lines[1].contains("[x]"), "Track 0 should show [x]");
    assert!(lines[1].contains("Track Zero"));

    // Line 2: Track 1 is unselected ([ ])
    assert!(lines[2].contains("[ ]"), "Track 1 should show [ ]");
    assert!(!lines[2].contains("[x]"), "Track 1 should not show [x]");
    assert!(lines[2].contains("Track One"));

    // Line 3: Track 2 is selected ([x])
    assert!(lines[3].contains("[x]"), "Track 2 should show [x]");
    assert!(lines[3].contains("Track Two"));

    // Clear selections and re-render
    modal.clear_selection();
    let lines_cleared = format_modal_lines(&rect, &modal, &queue);
    assert!(lines_cleared[1].contains("[ ]"), "Track 0 should show [ ] after clear");
    assert!(lines_cleared[2].contains("[ ]"), "Track 1 should show [ ] after clear");
    assert!(lines_cleared[3].contains("[ ]"), "Track 2 should show [ ] after clear");
    assert!(!lines_cleared[1].contains("[x]"));
    assert!(!lines_cleared[2].contains("[x]"));
    assert!(!lines_cleared[3].contains("[x]"));
}


#[test]
fn test_modal_border_and_content_rendering_in_memory() {
    let tracks = (0..5)
        .map(|i| Track {
            index: i,
            id: format!("id_{:03}", i),
            title: format!("Song Title {:03}", i),
        })
        .collect();
    let queue = Queue::new(tracks);
    let rect = ModalRect {
        x: 2,
        y: 2,
        width: 50,
        height: 7, // 1 header, 5 content, 1 footer
    };

    let mut modal = ModalState::default();
    modal.open(0, 5, rect.content_height());

    let mut buf = Vec::new();
    render_floating_modal(&mut buf, &rect, &modal, &queue).unwrap();
    let rendered = String::from_utf8(buf).unwrap();

    // Verify rounded borders
    assert!(rendered.contains('╭'), "Should render top-left rounded corner");
    assert!(rendered.contains('╮'), "Should render top-right rounded corner");
    assert!(rendered.contains('╰'), "Should render bottom-left rounded corner");
    assert!(rendered.contains('╯'), "Should render bottom-right rounded corner");
    assert!(rendered.contains('─'), "Should render horizontal border lines");
    assert!(rendered.contains('│'), "Should render vertical border lines");

    // Header with position badge
    assert!(
        rendered.contains("[1/5]"),
        "Header must contain position badge [1/5]"
    );

    // Active track marker for track 0
    assert!(
        rendered.contains('▶'),
        "Active track 0 must be marked with ▶"
    );

    // Footer with key hints
    assert!(
        rendered.contains("Enter: Play") || rendered.contains("Enter: Jump") || rendered.contains("Enter"),
        "Footer should indicate Enter action"
    );
    assert!(
        rendered.contains("j/k"),
        "Footer should show scroll/navigation key hints"
    );
    assert!(
        rendered.contains("Esc") || rendered.contains("Close"),
        "Footer should show dismiss key hints"
    );

    // Moving viewer cursor updates header badge and highlight
    modal.move_cursor_down(5, rect.content_height());
    modal.move_cursor_down(5, rect.content_height());
    assert_eq!(modal.viewer_cursor, 2);

    let mut buf2 = Vec::new();
    render_floating_modal(&mut buf2, &rect, &modal, &queue).unwrap();
    let rendered2 = String::from_utf8(buf2).unwrap();

    assert!(
        rendered2.contains("[3/5]"),
        "Header position badge must update to [3/5]"
    );
    // Track 0 still has ▶ (active playing track)
    assert!(rendered2.contains('▶'));
    // Track 2 title is present and highlighted
    assert!(rendered2.contains("Song Title 002"));
}

#[test]
fn test_modal_empty_queue_rendering() {
    let queue = Queue::default();
    let rect = ModalRect {
        x: 0,
        y: 0,
        width: 40,
        height: 6,
    };
    let mut modal = ModalState::default();
    modal.open(0, 0, rect.content_height());

    let mut buf = Vec::new();
    render_floating_modal(&mut buf, &rect, &modal, &queue).unwrap();
    let rendered = String::from_utf8(buf).unwrap();

    assert!(rendered.contains("[0/0]"), "Position badge should show [0/0]");
    assert!(
        rendered.contains("empty") || rendered.contains("No Track"),
        "Should show empty indicator"
    );
}

#[test]
fn test_alternate_screen_enter_and_leave() {
    let mut enter_buf = Vec::new();
    enter_alternate_screen(&mut enter_buf).unwrap();
    let enter_str = String::from_utf8(enter_buf).unwrap();
    assert!(
        enter_str.contains("\x1b[?1049h"),
        "Should issue crossterm EnterAlternateScreen ANSI code"
    );

    let mut leave_buf = Vec::new();
    leave_alternate_screen(&mut leave_buf).unwrap();
    let leave_str = String::from_utf8(leave_buf).unwrap();
    assert!(
        leave_str.contains("\x1b[?1049l"),
        "Should issue crossterm LeaveAlternateScreen ANSI code"
    );
}

#[test]
fn test_format_modal_lines_structure() {
    let tracks = vec![
        Track {
            index: 0,
            id: "t0".to_string(),
            title: "First Track".to_string(),
        },
        Track {
            index: 1,
            id: "t1".to_string(),
            title: "Second Track".to_string(),
        },
    ];
    let queue = Queue::new(tracks);
    let rect = ModalRect {
        x: 0,
        y: 0,
        width: 30,
        height: 4,
    };
    let mut modal = ModalState::default();
    modal.open(0, 2, rect.content_height());

    let lines = format_modal_lines(&rect, &modal, &queue);
    assert_eq!(lines.len(), 4);
    assert!(lines[0].starts_with('╭'));
    assert!(lines[0].ends_with('╮'));
    assert!(lines[1].starts_with('│'));
    assert!(lines[1].contains("First Track"));
    assert!(lines[2].starts_with('│'));
    assert!(lines[2].contains("Second Track"));
    assert!(lines[3].starts_with('╰'));
    assert!(lines[3].ends_with('╯'));
}


