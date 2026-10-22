use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ytm::ui::{
    format_line1, format_line2, format_time, parse_key_event, render_status_bar,
    wipe_status_lines, KeyAction, StatusState,
};

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
    let (l1, l2) = state.format_lines();
    assert_eq!(l1, "[▶] Track A (00:30 / 03:00)  [⚡ Cached]");
    assert_eq!(l2, "[Shuffle: ON] [Loop: OFF]  Next: Track B");
}

#[test]
fn test_render_status_bar_and_wipe_ansi() {
    let mut buf = Vec::new();
    render_status_bar(&mut buf, "Line 1 Content", "Line 2 Content").unwrap();
    let rendered = String::from_utf8(buf).unwrap();
    // Must contain carriage return \r, line clear \x1b[2K, newline \n, and move up \x1b[1A\r
    assert_eq!(
        rendered,
        "\r\x1b[2KLine 1 Content\n\r\x1b[2KLine 2 Content\x1b[1A\r"
    );

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

    // Next track: >, ., Right
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('>'), KeyModifiers::empty())),
        Some(KeyAction::NextTrack)
    );
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('.'), KeyModifiers::empty())),
        Some(KeyAction::NextTrack)
    );
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Right, KeyModifiers::empty())),
        Some(KeyAction::NextTrack)
    );

    // Prev track: <, ,, Left
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('<'), KeyModifiers::empty())),
        Some(KeyAction::PreviousTrack)
    );
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char(','), KeyModifiers::empty())),
        Some(KeyAction::PreviousTrack)
    );
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Left, KeyModifiers::empty())),
        Some(KeyAction::PreviousTrack)
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

    // Single loop: l, L
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('l'), KeyModifiers::empty())),
        Some(KeyAction::ToggleLoop)
    );
    assert_eq!(
        parse_key_event(make_key_event(KeyCode::Char('L'), KeyModifiers::empty())),
        Some(KeyAction::ToggleLoop)
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

