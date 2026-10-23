use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;
use unicode_width::UnicodeWidthStr;
use ytm::app::App;
use ytm::cache::AudioCache;
use ytm::mpv::MpvController;
use ytm::playlist::Track;
use ytm::prefetch::{AudioDownloader, AudioPrefetcher};
use ytm::queue::Queue;
use ytm::ui::{
    format_line1, format_modal_row, parse_modal_search_key_event, truncate_to_width, KeyAction,
    ModalKeyAction,
};

struct MockDownloader;

impl AudioDownloader for MockDownloader {
    fn download(&self, _video_id: &str, _tmp_path: &std::path::Path) -> Result<(), String> {
        Err("HTTP 403 Forbidden".to_string())
    }
}

fn setup_test_app() -> (App, UnixStream) {
    let (client_stream, server_stream) = UnixStream::pair().unwrap();
    server_stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let mpv = MpvController::from_stream(client_stream);

    let temp_dir = std::env::temp_dir().join(format!("ytm-repro-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let cache = AudioCache::new(temp_dir);

    let prefetcher = AudioPrefetcher::with_downloader(cache.clone(), MockDownloader);

    let tracks = vec![
        Track {
            index: 0,
            id: "vid_0".to_string(),
            title: "Track Zero".to_string(),
        },
        Track {
            index: 1,
            id: "vid_1".to_string(),
            title: "Track One".to_string(),
        },
        Track {
            index: 2,
            id: "vid_2".to_string(),
            title: "Track Two".to_string(),
        },
        Track {
            index: 3,
            id: "vid_3".to_string(),
            title: "Track Three".to_string(),
        },
    ];
    let queue = Queue::new(tracks);
    let app = App::new(queue, cache, prefetcher, mpv);
    (app, server_stream)
}

/// Strip ANSI escape sequences to measure pure terminal column display width
fn strip_ansi(s: &str) -> String {
    let mut result = String::new();
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c == 'm' {
                in_escape = false;
            }
        } else {
            result.push(c);
        }
    }
    result
}

// 1. CJK Wide-Character Truncation & Modal Row Width
#[test]
fn test_repro_cjk_wide_character_truncation() {
    // "마법소녀" is 4 characters, but 8 terminal columns wide.
    // Truncating to max_width 6 should produce a string with display width <= 6.
    let truncated = truncate_to_width("마법소녀", 6);
    let width = UnicodeWidthStr::width(truncated.as_str());
    assert!(
        width <= 6,
        "Truncated CJK string display width {} exceeded max_width 6: '{}'",
        width,
        truncated
    );
}

#[test]
fn test_repro_cjk_modal_row_display_width() {
    let content_width = 30;
    let row = format_modal_row(
        0,
        "魔法少女フィオニー⚡", // CJK wide characters + emoji
        false,
        false,
        true,
        false,
        false,
        content_width,
    );
    let clean = strip_ansi(&row);
    // The formatted row is formatted as "│<content>│"
    assert!(clean.starts_with('│') && clean.ends_with('│'));
    let inner = clean.trim_start_matches('│').trim_end_matches('│');
    let inner_width = UnicodeWidthStr::width(inner);
    assert_eq!(
        inner_width, content_width,
        "Modal row inner display width {} does not match content_width {}: row='{}'",
        inner_width, content_width, clean
    );
}

// 2. Hold `l` past duration crashes player
#[test]
fn test_repro_hold_l_past_duration_error_handling() {
    let (mut app, mut server) = setup_test_app();

    let responder = std::thread::spawn(move || {
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // App sends seek forward 5s
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "seek");
        let req_id = val["request_id"].as_u64().unwrap();

        // MPV returns error because track is at EOF
        server
            .write_all(
                format!(
                    r#"{{"request_id":{},"error":"error running command"}}"#,
                    req_id
                )
                .as_bytes(),
            )
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    // Seeking forward past duration must not return an Err / crash the app
    let result = app.handle_action(KeyAction::SeekForward);
    responder.join().unwrap();
    assert!(
        result.is_ok(),
        "Seeking forward past duration returned error: {:?}",
        result
    );
}

// 3. Hold `h` stutter at 00:00
#[test]
fn test_repro_hold_h_stutter_at_zero() {
    let (mut app, mut server) = setup_test_app();

    let responder = std::thread::spawn(move || {
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // Read command from app
        if reader.read_line(&mut line).is_ok() && !line.is_empty() {
            let val: serde_json::Value = serde_json::from_str(&line).unwrap();
            let req_id = val["request_id"].as_u64().unwrap();

            if val["command"][0] == "get_property" && val["command"][1] == "time-pos" {
                // Return 0.0 (already at start)
                server
                    .write_all(format!(r#"{{"request_id":{},"error":"success","data":0.0}}"#, req_id).as_bytes())
                    .unwrap();
                server.write_all(b"\n").unwrap();

                // Check if app sends another command (e.g. seek)
                line.clear();
                if let Ok(n) = reader.read_line(&mut line) {
                    if n > 0 {
                        let val2: serde_json::Value = serde_json::from_str(&line).unwrap();
                        if val2["command"][0] == "seek" {
                            panic!("Redundant seek command was sent when position is 00:00: {}", line);
                        }
                    }
                }
            } else if val["command"][0] == "seek" {
                panic!("App sent seek command directly without checking position at 00:00: {}", line);
            }
        }
    });

    let _ = app.handle_action(KeyAction::SeekBackward);
    responder.join().unwrap();
}

// 4. Backspace with `^H`, `\x08`, `\x7f` in search modal
#[test]
fn test_repro_modal_search_backspace_variants() {
    let ctrl_h = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL);
    assert_eq!(
        parse_modal_search_key_event(ctrl_h),
        Some(ModalKeyAction::SearchBackspace),
        "Ctrl+h was not recognized as SearchBackspace"
    );

    let bs_08 = KeyEvent::new(KeyCode::Char('\x08'), KeyModifiers::NONE);
    assert_eq!(
        parse_modal_search_key_event(bs_08),
        Some(ModalKeyAction::SearchBackspace),
        "\\x08 was not recognized as SearchBackspace"
    );

    let bs_7f = KeyEvent::new(KeyCode::Char('\x7f'), KeyModifiers::NONE);
    assert_eq!(
        parse_modal_search_key_event(bs_7f),
        Some(ModalKeyAction::SearchBackspace),
        "\\x7f was not recognized as SearchBackspace"
    );
}

// 5. Playback auto-run continuity
#[test]
fn test_repro_playback_auto_run_continuity() {
    let (mut app, mut server) = setup_test_app();

    let responder = std::thread::spawn(move || {
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // 1. Initial pause
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "set_property");
        assert_eq!(val["command"][1], "pause");
        assert_eq!(val["command"][2], true);
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success"}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // 2. Initial loadfile
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success"}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // 3. User initiates playback (TogglePause)
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "cycle");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success"}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // 4. Advance to next track -> loadfile AND unpause!
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success"}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        line.clear();
        let read_res = reader.read_line(&mut line);
        assert!(
            read_res.is_ok() && !line.is_empty(),
            "Expected unpause command when advancing track after playback was started, but stream timed out or was empty"
        );
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            val["command"][0], "set_property",
            "Expected unpause command when advancing track after playback was started"
        );
        assert_eq!(val["command"][1], "pause");
        assert_eq!(val["command"][2], false);
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success"}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    app.load_initial().unwrap();
    // User starts playback
    app.handle_action(KeyAction::TogglePause).unwrap();

    // Next track advances
    app.handle_action(KeyAction::NextTrack).unwrap();

    responder.join().unwrap();
}

// 6. Play/Pause icon convention: [▶] when paused, [⏸] when playing
#[test]
fn test_repro_play_pause_icon_convention() {
    let paused_line = format_line1(true, "Song Title", Some(0.0), Some(100.0), false);
    assert!(
        paused_line.starts_with("[▶]"),
        "Paused state should display [▶] icon, got: {}",
        paused_line
    );

    let playing_line = format_line1(false, "Song Title", Some(0.0), Some(100.0), false);
    assert!(
        playing_line.starts_with("[⏸]"),
        "Playing state should display [⏸] icon, got: {}",
        playing_line
    );
}

// 7. Queue subsetting and range looping
#[test]
fn test_repro_queue_subsetting_and_range_looping() {
    let tracks = vec![
        Track {
            index: 0,
            id: "id_0".to_string(),
            title: "T0".to_string(),
        },
        Track {
            index: 1,
            id: "id_1".to_string(),
            title: "T1".to_string(),
        },
        Track {
            index: 2,
            id: "id_2".to_string(),
            title: "T2".to_string(),
        },
        Track {
            index: 3,
            id: "id_3".to_string(),
            title: "T3".to_string(),
        },
    ];
    let mut queue = Queue::new(tracks);

    // Cherry pick tracks at index 1 and 3
    queue.reorder_with_selection(&[1, 3]);

    // Active isolated queue should contain ONLY the 2 selected tracks
    assert_eq!(
        queue.playback_order.len(),
        2,
        "Isolated queue should have 2 tracks, got: {:?}",
        queue.playback_order
    );
    assert_eq!(queue.current_track().unwrap().id, "id_1");

    // Advancing loops within the subset
    queue.next();
    assert_eq!(queue.current_track().unwrap().id, "id_3");
    queue.next();
    assert_eq!(queue.current_track().unwrap().id, "id_1");

    // Iceberg reset (`r`) restores full playlist
    queue.reset_to_top();
    assert_eq!(
        queue.playback_order.len(),
        4,
        "Reset to top should restore all 4 tracks"
    );
}
