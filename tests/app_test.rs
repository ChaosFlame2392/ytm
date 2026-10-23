use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use ytm::app::App;
use ytm::cache::AudioCache;
use ytm::mpv::{EndFileReason, MpvController, MpvEvent};
use ytm::playlist::Track;
use ytm::prefetch::{AudioDownloader, AudioPrefetcher};
use ytm::queue::Queue;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ytm::ui::{parse_key_event, parse_modal_search_key_event, KeyAction, ModalKeyAction};

struct MockDownloader {
    downloaded: Arc<Mutex<Vec<String>>>,
}

impl AudioDownloader for MockDownloader {
    fn download(&self, video_id: &str, _tmp_path: &std::path::Path) -> Result<(), String> {
        self.downloaded.lock().unwrap().push(video_id.to_string());
        Ok(())
    }
}

fn setup_test_app() -> (App, UnixStream, Arc<Mutex<Vec<String>>>) {
    let (client_stream, server_stream) = UnixStream::pair().unwrap();
    let mpv = MpvController::from_stream(client_stream);

    let temp_dir = tempfile_dir("ytm-app-test");
    let cache = AudioCache::new(temp_dir);

    let downloaded = Arc::new(Mutex::new(Vec::new()));
    let prefetcher = AudioPrefetcher::with_downloader(
        cache.clone(),
        MockDownloader {
            downloaded: downloaded.clone(),
        },
    );

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
    ];
    let queue = Queue::new(tracks);

    let app = App::new(queue, cache, prefetcher, mpv);
    (app, server_stream, downloaded)
}

fn tempfile_dir(prefix: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("{}-{}-{}", prefix, std::process::id(), count));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[test]
fn test_app_action_quit_returns_false() {
    let (mut app, _server, _) = setup_test_app();
    let cont = app.handle_action(KeyAction::Quit).unwrap();
    assert!(!cont);
}

#[test]
fn test_app_action_toggle_loop() {
    let (mut app, _server, _) = setup_test_app();
    assert!(!app.queue.single_loop);

    let cont = app.handle_action(KeyAction::ToggleLoop).unwrap();
    assert!(cont);
    assert!(app.queue.single_loop);

    let cont = app.handle_action(KeyAction::ToggleLoop).unwrap();
    assert!(cont);
    assert!(!app.queue.single_loop);
}

#[test]
fn test_app_eof_event_advances_queue_and_plays_next() {
    let (mut app, mut server, _downloaded) = setup_test_app();

    // Spawn mock MPV responder in background
    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // 1. App handles EOF -> advances to track 1 -> sends loadfile for vid_1
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    assert_eq!(app.queue.cursor, 0);

    // Simulate MPV EOF event
    app.handle_mpv_event(MpvEvent::EndFile {
        reason: EndFileReason::Eof,
    })
    .unwrap();

    assert_eq!(app.queue.cursor, 1);
    assert_eq!(app.queue.current_track().unwrap().id, "vid_1");

    responder.join().unwrap();
}

#[test]
fn test_app_stream_error_event_advances_queue_and_sets_error_notification() {
    let (mut app, mut server, _downloaded) = setup_test_app();

    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // Expect loadfile for vid_1 after vid_0 encounters error
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    assert_eq!(app.queue.cursor, 0);
    assert!(app.error_message.is_none());

    // Simulate MPV error event (track failed to stream or decode)
    app.handle_mpv_event(MpvEvent::EndFile {
        reason: EndFileReason::Error,
    })
    .unwrap();

    // Verify queue advanced to track 1
    assert_eq!(app.queue.cursor, 1);
    assert_eq!(app.queue.current_track().unwrap().id, "vid_1");

    // Verify error notification is set on App and reflected in StatusState
    assert_eq!(app.error_message.as_deref(), Some("[!] Stream Error"));
    let status = app.current_status_state();
    assert_eq!(status.error_message, Some("[!] Stream Error"));

    let (_l1, l2) = status.format_lines();
    assert!(l2.contains("[!] Stream Error"), "Line 2 should display error badge: {}", l2);

    responder.join().unwrap();
}

#[test]
fn test_app_stream_error_other_reason_advances_and_sets_message() {
    let (mut app, mut server, _downloaded) = setup_test_app();

    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    app.handle_mpv_event(MpvEvent::EndFile {
        reason: EndFileReason::Other("network timeout".to_string()),
    })
    .unwrap();

    assert_eq!(app.queue.cursor, 1);
    assert!(app.error_message.as_ref().unwrap().contains("[!] Stream Error"));

    responder.join().unwrap();
}

#[test]
fn test_app_stream_error_all_tracks_fail_halts_infinite_loop() {
    let (mut app, mut server, _downloaded) = setup_test_app();

    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());

        // Respond to track 1 loadfile
        let mut line1 = String::new();
        reader.read_line(&mut line1).unwrap();
        let val1: serde_json::Value = serde_json::from_str(&line1).unwrap();
        assert_eq!(val1["command"][0], "loadfile");
        let req_id1 = val1["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id1).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // Respond to track 2 loadfile
        let mut line2 = String::new();
        reader.read_line(&mut line2).unwrap();
        let val2: serde_json::Value = serde_json::from_str(&line2).unwrap();
        assert_eq!(val2["command"][0], "loadfile");
        let req_id2 = val2["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id2).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    // Track 0 fails -> advances to track 1
    app.handle_mpv_event(MpvEvent::EndFile {
        reason: EndFileReason::Error,
    })
    .unwrap();
    assert_eq!(app.queue.cursor, 1);

    // Track 1 fails -> advances to track 2
    app.handle_mpv_event(MpvEvent::EndFile {
        reason: EndFileReason::Error,
    })
    .unwrap();
    assert_eq!(app.queue.cursor, 2);

    // Track 2 fails -> all tracks in queue have now failed
    // Must NOT call loadfile again; loop terminates cleanly without spinning
    app.handle_mpv_event(MpvEvent::EndFile {
        reason: EndFileReason::Error,
    })
    .unwrap();

    assert!(app.error_message.is_some());
    assert_eq!(app.failed_tracks.len(), 3);

    responder.join().unwrap();
}

#[test]
fn test_app_stream_error_cleared_on_user_action_and_natural_eof() {
    let (mut app, mut server, _downloaded) = setup_test_app();

    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());

        // 1. Error on vid_0 -> loadfile for vid_1
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // 2. Normal EOF on vid_1 -> loadfile for vid_2
        let mut line2 = String::new();
        reader.read_line(&mut line2).unwrap();
        let val2: serde_json::Value = serde_json::from_str(&line2).unwrap();
        let req_id2 = val2["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id2).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    // Trigger error on track 0
    app.handle_mpv_event(MpvEvent::EndFile {
        reason: EndFileReason::Error,
    })
    .unwrap();
    assert!(app.error_message.is_some());

    // Trigger natural EOF on track 1
    app.handle_mpv_event(MpvEvent::EndFile {
        reason: EndFileReason::Eof,
    })
    .unwrap();
    // Error notification must be cleared
    assert!(app.error_message.is_none());

    responder.join().unwrap();
}

#[test]
fn test_app_action_next_and_prev_tracks() {
    let (mut app, mut server, _downloaded) = setup_test_app();

    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // 1. NextTrack -> load vid_1
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // 2. PreviousTrack -> load vid_0
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    assert_eq!(app.queue.cursor, 0);
    app.handle_action(KeyAction::NextTrack).unwrap();
    assert_eq!(app.queue.cursor, 1);

    app.handle_action(KeyAction::PreviousTrack).unwrap();
    assert_eq!(app.queue.cursor, 0);

    responder.join().unwrap();
}

#[test]
fn test_app_action_reset_to_top() {
    let (mut app, mut server, _downloaded) = setup_test_app();

    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // Next -> load vid_1
        reader.read_line(&mut line).unwrap();
        let req_id = serde_json::from_str::<serde_json::Value>(&line).unwrap()["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // ResetToTop -> load vid_0
        line.clear();
        reader.read_line(&mut line).unwrap();
        let req_id = serde_json::from_str::<serde_json::Value>(&line).unwrap()["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    app.handle_action(KeyAction::NextTrack).unwrap();
    assert_eq!(app.queue.cursor, 1);

    app.handle_action(KeyAction::ResetToTop).unwrap();
    assert_eq!(app.queue.cursor, 0);
    assert_eq!(app.queue.current_track().unwrap().id, "vid_0");

    responder.join().unwrap();
}

#[test]
fn test_app_cached_vs_streaming_source() {
    let (mut app, mut server, downloaded) = setup_test_app();

    // Make vid_0 cached on disk
    let vid_0_cache = app.cache.cache_file_path("vid_0");
    std::fs::write(&vid_0_cache, b"fake-audio-bytes").unwrap();
    assert!(app.cache.is_cached("vid_0"));
    assert!(!app.cache.is_cached("vid_1"));

    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // play_current for vid_0 -> should load local file path
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        let path = val["command"][1].as_str().unwrap();
        assert!(path.ends_with("vid_0.opus"));
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    app.play_current().unwrap();

    // Prefetcher should have queued vid_1 (peek_next)
    // Wait briefly for prefetcher thread
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(downloaded.lock().unwrap().contains(&"vid_1".to_string()));

    responder.join().unwrap();
}

#[test]
fn test_app_render_status_output() {
    let (app, mut server, _) = setup_test_app();

    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // 1. is_paused -> get_property pause
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":false}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // 2. get_position -> get_property time-pos
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":45.0}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // 3. get_duration -> get_property duration
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":210.0}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    let mut buf = Vec::new();
    app.render_status(&mut buf).unwrap();
    let rendered = String::from_utf8(buf).unwrap();

    assert!(rendered.contains("[⏸] Track Zero (00:45 / 03:30)"));
    assert!(rendered.contains("[Shuffle: OFF] [Loop: OFF]  Next: Track One"));

    responder.join().unwrap();
}

#[test]
fn test_app_current_track_proactive_prefetch_when_not_cached() {
    let (mut app, mut server, downloaded) = setup_test_app();

    assert!(!app.cache.is_cached("vid_0"));
    assert!(!app.cache.is_cached("vid_1"));

    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // play_current for vid_0 -> should load remote streaming URL
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        let path = val["command"][1].as_str().unwrap();
        assert_eq!(path, "https://www.youtube.com/watch?v=vid_0");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    app.play_current().unwrap();

    // Prefetcher should have queued both vid_0 (current track proactive cache) and vid_1 (peek_next)
    std::thread::sleep(std::time::Duration::from_millis(50));
    let list = downloaded.lock().unwrap().clone();
    assert!(list.contains(&"vid_0".to_string()), "current track should be queued for prefetch");
    assert!(list.contains(&"vid_1".to_string()), "next track should be queued for prefetch");

    responder.join().unwrap();
}

#[test]
fn test_app_single_loop_manual_skip_vs_natural_eof() {
    let (mut app, mut server, _) = setup_test_app();

    // Turn single loop ON
    app.handle_action(KeyAction::ToggleLoop).unwrap();
    assert!(app.queue.single_loop);

    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // 1. Manual NextTrack -> should advance to vid_1
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        assert!(val["command"][1].as_str().unwrap().contains("vid_1"));
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // 2. EOF event -> should repeat vid_1 because single_loop is ON
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        assert!(val["command"][1].as_str().unwrap().contains("vid_1"));
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    // 1. Manual NextTrack advances cursor even though loop is ON
    app.handle_action(KeyAction::NextTrack).unwrap();
    assert_eq!(app.queue.cursor, 1);
    assert_eq!(app.queue.current_track().unwrap().id, "vid_1");

    // 2. EOF repeats current track
    app.handle_mpv_event(MpvEvent::EndFile {
        reason: EndFileReason::Eof,
    })
    .unwrap();
    assert_eq!(app.queue.cursor, 1);
    assert_eq!(app.queue.current_track().unwrap().id, "vid_1");

    responder.join().unwrap();
}

#[test]
fn test_app_initialization_prunes_cache() {
    let (client_stream, _server_stream) = UnixStream::pair().unwrap();
    let mpv = MpvController::from_stream(client_stream);

    let temp_dir = tempfile_dir("ytm-prune-test");
    let cache = AudioCache::with_limits(temp_dir.clone(), 100, 50);
    cache.ensure_audio_dir().unwrap();

    let old_file = cache.cache_file_path("old");
    std::fs::write(&old_file, vec![0u8; 150]).unwrap();
    assert!(old_file.exists());

    let queue = Queue::new(vec![]);
    let prefetcher = AudioPrefetcher::new(cache.clone());

    // Creating App should trigger cache.prune_lru()
    let _app = App::new(queue, cache, prefetcher, mpv);

    assert!(!old_file.exists(), "Cache should be pruned on App initialization");
}

#[test]
fn test_app_initial_paused_startup() {
    let (mut app, mut server, _downloaded) = setup_test_app();

    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // 1. Initial pause command: set_property pause true
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "set_property");
        assert_eq!(val["command"][1], "pause");
        assert_eq!(val["command"][2], true);
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // 2. Initial loadfile for track 0 (vid_0)
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        assert_eq!(val["command"][1], "https://www.youtube.com/watch?v=vid_0");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // 3. Status query: get_property pause
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "get_property");
        assert_eq!(val["command"][1], "pause");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":true}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // 4. Status query: get_property time-pos (property unavailable during pause at start)
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"property unavailable"}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // 5. Status query: get_property duration
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":240.0}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // 6. User presses Space (TogglePause): cycle pause
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "cycle");
        assert_eq!(val["command"][1], "pause");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // 7. Status query: get_property pause after unpause
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":false}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // 8. Status query: get_property time-pos
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":0.0}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // 9. Status query: get_property duration
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":240.0}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    app.load_initial().unwrap();
    assert_eq!(app.queue.cursor, 0);

    let state = app.current_status_state();
    assert!(state.is_paused, "App should start in paused state");
    assert!(state.format_line1().starts_with("[▶]"), "Status icon should be [▶]");

    // Press Space to start playback
    app.handle_action(KeyAction::TogglePause).unwrap();
    let state = app.current_status_state();
    assert!(!state.is_paused, "App should unpause after Space");
    assert!(state.format_line1().starts_with("[⏸]"), "Status icon should be [⏸]");

    responder.join().unwrap();
}

#[test]
fn test_app_seek_ipc_commands_and_clamping() {
    let (mut app, mut server, _downloaded) = setup_test_app();

    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // 1. SeekForward (+5s relative): ["seek", "5", "relative"]
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "seek");
        assert_eq!(val["command"][1], "5");
        assert_eq!(val["command"][2], "relative");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // Status queries for render
        line.clear();
        reader.read_line(&mut line).unwrap();
        let req_id = serde_json::from_str::<serde_json::Value>(&line).unwrap()["request_id"].as_u64().unwrap();
        server.write_all(format!(r#"{{"request_id":{},"error":"success","data":false}}"#, req_id).as_bytes()).unwrap();
        server.write_all(b"\n").unwrap();

        line.clear();
        reader.read_line(&mut line).unwrap();
        let req_id = serde_json::from_str::<serde_json::Value>(&line).unwrap()["request_id"].as_u64().unwrap();
        server.write_all(format!(r#"{{"request_id":{},"error":"success","data":25.0}}"#, req_id).as_bytes()).unwrap();
        server.write_all(b"\n").unwrap();

        line.clear();
        reader.read_line(&mut line).unwrap();
        let req_id = serde_json::from_str::<serde_json::Value>(&line).unwrap()["request_id"].as_u64().unwrap();
        server.write_all(format!(r#"{{"request_id":{},"error":"success","data":200.0}}"#, req_id).as_bytes()).unwrap();
        server.write_all(b"\n").unwrap();

        // 2. SeekBackward checks position before seeking: get_property time-pos
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "get_property");
        assert_eq!(val["command"][1], "time-pos");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":25.0}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // 3. Since position was 25.0 (> 0.0), it sends relative seek: ["seek", "-5", "relative"]
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "seek");
        assert_eq!(val["command"][1], "-5");
        assert_eq!(val["command"][2], "relative");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // Status queries for render with position clamped at 0.0
        line.clear();
        reader.read_line(&mut line).unwrap();
        let req_id = serde_json::from_str::<serde_json::Value>(&line).unwrap()["request_id"].as_u64().unwrap();
        server.write_all(format!(r#"{{"request_id":{},"error":"success","data":false}}"#, req_id).as_bytes()).unwrap();
        server.write_all(b"\n").unwrap();

        line.clear();
        reader.read_line(&mut line).unwrap();
        let req_id = serde_json::from_str::<serde_json::Value>(&line).unwrap()["request_id"].as_u64().unwrap();
        server.write_all(format!(r#"{{"request_id":{},"error":"success","data":0.0}}"#, req_id).as_bytes()).unwrap();
        server.write_all(b"\n").unwrap();

        line.clear();
        reader.read_line(&mut line).unwrap();
        let req_id = serde_json::from_str::<serde_json::Value>(&line).unwrap()["request_id"].as_u64().unwrap();
        server.write_all(format!(r#"{{"request_id":{},"error":"success","data":200.0}}"#, req_id).as_bytes()).unwrap();
        server.write_all(b"\n").unwrap();
    });

    // 1. Forward seek
    app.handle_action(KeyAction::SeekForward).unwrap();
    let state = app.current_status_state();
    assert_eq!(state.position, Some(25.0));
    assert!(state.format_line1().contains("(00:25 / 03:20)"));

    // 2. Backward seek clamps to 00:00 without skipping tracks
    assert_eq!(app.queue.cursor, 0);
    app.handle_action(KeyAction::SeekBackward).unwrap();
    assert_eq!(app.queue.cursor, 0, "Seeking backward should not change queue track");
    let state = app.current_status_state();
    assert_eq!(state.position, Some(0.0));
    assert!(state.format_line1().contains("(00:00 / 03:20)"));

    responder.join().unwrap();
}

#[test]
fn test_app_seek_forward_past_duration_triggers_eof_advancement() {
    let (mut app, mut server, _downloaded) = setup_test_app();

    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // 1. App sends seek forward +5
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "seek");
        assert_eq!(val["command"][1], "5");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // 2. Seeking past duration causes MPV to emit EndFile(Eof) event
        server
            .write_all(b"{\"event\":\"end-file\",\"reason\":\"eof\"}\n")
            .unwrap();

        // 3. App handles EOF -> advances to track 1 (vid_1) -> loadfile
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        assert_eq!(val["command"][1], "https://www.youtube.com/watch?v=vid_1");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    assert_eq!(app.queue.cursor, 0);
    app.handle_action(KeyAction::SeekForward).unwrap();

    // Process MPV events received from seek past duration
    let ev = app.mpv.recv_event().unwrap();
    assert_eq!(ev, MpvEvent::EndFile { reason: EndFileReason::Eof });
    app.handle_mpv_event(ev).unwrap();

    assert_eq!(app.queue.cursor, 1, "Seeking past duration should advance to next queue track");
    assert_eq!(app.queue.current_track().unwrap().id, "vid_1");

    responder.join().unwrap();
}

#[test]
fn test_app_status_line1_immediate_update_on_seek() {
    let (mut app, mut server, _) = setup_test_app();

    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // Seek forward
        reader.read_line(&mut line).unwrap();
        let req_id = serde_json::from_str::<serde_json::Value>(&line).unwrap()["request_id"].as_u64().unwrap();
        server.write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes()).unwrap();
        server.write_all(b"\n").unwrap();

        // Status queries immediately after seek
        line.clear();
        reader.read_line(&mut line).unwrap();
        let req_id = serde_json::from_str::<serde_json::Value>(&line).unwrap()["request_id"].as_u64().unwrap();
        server.write_all(format!(r#"{{"request_id":{},"error":"success","data":false}}"#, req_id).as_bytes()).unwrap();
        server.write_all(b"\n").unwrap();

        line.clear();
        reader.read_line(&mut line).unwrap();
        let req_id = serde_json::from_str::<serde_json::Value>(&line).unwrap()["request_id"].as_u64().unwrap();
        server.write_all(format!(r#"{{"request_id":{},"error":"success","data":45.0}}"#, req_id).as_bytes()).unwrap();
        server.write_all(b"\n").unwrap();

        line.clear();
        reader.read_line(&mut line).unwrap();
        let req_id = serde_json::from_str::<serde_json::Value>(&line).unwrap()["request_id"].as_u64().unwrap();
        server.write_all(format!(r#"{{"request_id":{},"error":"success","data":180.0}}"#, req_id).as_bytes()).unwrap();
        server.write_all(b"\n").unwrap();
    });

    app.handle_action(KeyAction::SeekForward).unwrap();
    let mut buf = Vec::new();
    app.render_status(&mut buf).unwrap();
    let rendered = String::from_utf8(buf).unwrap();

    assert!(
        rendered.contains("[⏸] Track Zero (00:45 / 03:00)"),
        "Status should render immediately with new seeked position"
    );

    responder.join().unwrap();
}

#[test]
fn test_app_modal_open_and_close_without_affecting_playback() {
    let (mut app, _server, _) = setup_test_app();
    assert_eq!(app.queue.cursor, 0);
    assert!(!app.modal.is_open);

    // Open modal via OpenModal action
    app.handle_action(KeyAction::OpenModal).unwrap();
    assert!(app.modal.is_open);
    assert_eq!(app.modal.viewer_cursor, 0);

    // Move cursor in modal
    app.handle_modal_action(ModalKeyAction::CursorDown).unwrap();
    assert_eq!(app.modal.viewer_cursor, 1);
    // Active playing track in queue is still 0
    assert_eq!(app.queue.cursor, 0);

    // Close modal via Close
    app.handle_modal_action(ModalKeyAction::Close).unwrap();
    assert!(!app.modal.is_open);
    // Queue cursor remains unaffected
    assert_eq!(app.queue.cursor, 0);
    assert_eq!(app.queue.current_track().unwrap().id, "vid_0");
}

#[test]
fn test_app_modal_navigation_and_track_jump() {
    let (mut app, mut server, downloaded) = setup_test_app();

    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // Commit jump to vid_2 -> loadfile https://www.youtube.com/watch?v=vid_2
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        assert_eq!(val["command"][1], "https://www.youtube.com/watch?v=vid_2");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // Auto-run continuity: unpause on playback start
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "set_property");
        assert_eq!(val["command"][1], "pause");
        assert_eq!(val["command"][2], false);
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    assert_eq!(app.queue.cursor, 0);

    // Open modal
    app.open_modal();
    assert!(app.modal.is_open);
    assert_eq!(app.modal.viewer_cursor, 0);

    // Navigate to vid_2 (cursor index 2)
    app.handle_modal_action(ModalKeyAction::CursorDown).unwrap();
    assert_eq!(app.modal.viewer_cursor, 1);
    app.handle_modal_action(ModalKeyAction::CursorDown).unwrap();
    assert_eq!(app.modal.viewer_cursor, 2);

    // Commit jump with Enter
    app.handle_modal_action(ModalKeyAction::Commit).unwrap();

    // Modal is now closed and playback switched to vid_2
    assert!(!app.modal.is_open);
    assert_eq!(app.queue.cursor, 2);
    assert_eq!(app.queue.current_track().unwrap().id, "vid_2");

    // Next track (vid_0) should be queued for prefetch
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(downloaded.lock().unwrap().contains(&"vid_0".to_string()));

    responder.join().unwrap();
}

#[test]
fn test_app_modal_jump_in_shuffled_queue() {
    let (mut app, mut server, _) = setup_test_app();

    // Toggle shuffle ON
    app.handle_action(KeyAction::ToggleShuffle).unwrap();
    assert!(app.queue.shuffle);

    // Pick target in shuffled order
    let target_idx = 1;
    let expected_track_id = app.queue.tracks[app.queue.playback_order[target_idx]].id.clone();

    let expected_url = format!("https://www.youtube.com/watch?v={}", expected_track_id);
    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        assert_eq!(val["command"][1], expected_url);
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // Auto-run continuity: unpause on playback start
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "set_property");
        assert_eq!(val["command"][1], "pause");
        assert_eq!(val["command"][2], false);
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    app.open_modal();
    assert_eq!(app.modal.viewer_cursor, 0);

    app.handle_modal_action(ModalKeyAction::CursorDown).unwrap();
    assert_eq!(app.modal.viewer_cursor, 1);

    app.handle_modal_action(ModalKeyAction::Commit).unwrap();
    assert!(!app.modal.is_open);
    assert_eq!(app.queue.cursor, 1);
    assert_eq!(app.queue.current_track().unwrap().id, expected_track_id);

    responder.join().unwrap();
}

#[test]
fn test_app_modal_search_flow_live_filtering_navigation_and_resolution() {
    let (mut app, mut server, _) = setup_test_app();

    // App has 3 tracks: vid_0 "Track Zero", vid_1 "Track One", vid_2 "Track Two"
    assert_eq!(app.queue.cursor, 0);

    // Open modal
    app.handle_action(KeyAction::OpenModal).unwrap();
    assert!(app.modal.is_open);
    assert_eq!(app.modal.viewer_cursor, 0);

    // Press '/' to activate search mode
    app.handle_modal_action(ModalKeyAction::SearchActivate).unwrap();
    assert!(app.modal.search_active);
    assert_eq!(app.modal.filtered_indices.len(), 3);

    // Type "Two" -> live filters to vid_2
    for c in "Two".chars() {
        app.handle_modal_action(ModalKeyAction::SearchChar(c)).unwrap();
    }
    assert_eq!(app.modal.search_query, "Two");
    assert_eq!(app.modal.filtered_indices, vec![2]);
    assert_eq!(app.modal.search_cursor, 0);

    // Backspace: delete 'o' and 'w', then type "e" -> "Te" matches "Track Zero" (0) and "Track One" (1)
    app.handle_modal_action(ModalKeyAction::SearchBackspace).unwrap();
    app.handle_modal_action(ModalKeyAction::SearchBackspace).unwrap();
    app.handle_modal_action(ModalKeyAction::SearchBackspace).unwrap(); // empty
    for c in "e".chars() {
        app.handle_modal_action(ModalKeyAction::SearchChar(c)).unwrap();
    }
    // "Track Zero" (idx 0) and "Track One" (idx 1) both contain 'e'
    assert_eq!(app.modal.filtered_indices, vec![0, 1]);
    assert_eq!(app.modal.search_cursor, 0);

    // Navigate with Down / j
    app.handle_modal_action(ModalKeyAction::CursorDown).unwrap();
    assert_eq!(app.modal.search_cursor, 1); // points to idx 1 ("Track One")

    // Press Enter to resolve search
    // This must exit search mode, restore full queue view, and center viewer_cursor on idx 1
    app.handle_modal_action(ModalKeyAction::SearchResolve).unwrap();
    assert!(!app.modal.search_active, "Search mode must be closed");
    assert!(app.modal.is_open, "Modal itself must remain open");
    assert_eq!(app.modal.viewer_cursor, 1, "Viewer cursor must be on resolved track");
    // Notice: playback has NOT changed yet (still on 0)
    assert_eq!(app.queue.cursor, 0);

    // Now press Enter in normal modal to commit playback to hovered track (idx 1)
    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        assert!(val["command"][1].as_str().unwrap().contains("vid_1"));
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // Auto-run continuity: unpause on playback start
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "set_property");
        assert_eq!(val["command"][1], "pause");
        assert_eq!(val["command"][2], false);
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    app.handle_modal_action(ModalKeyAction::Commit).unwrap();
    assert!(!app.modal.is_open, "Commit must close modal");
    assert_eq!(app.queue.cursor, 1, "Playback must switch to track 1");

    responder.join().unwrap();
}

#[test]
fn test_app_modal_search_cancellation_with_esc() {
    let (mut app, _server, _) = setup_test_app();
    app.handle_action(KeyAction::OpenModal).unwrap();
    assert_eq!(app.modal.viewer_cursor, 0);

    // Move viewer cursor to 2
    app.handle_modal_action(ModalKeyAction::CursorDown).unwrap();
    app.handle_modal_action(ModalKeyAction::CursorDown).unwrap();
    assert_eq!(app.modal.viewer_cursor, 2);

    // Enter search mode
    app.handle_modal_action(ModalKeyAction::SearchActivate).unwrap();
    assert!(app.modal.search_active);

    // Type "Zero"
    for c in "Zero".chars() {
        app.handle_modal_action(ModalKeyAction::SearchChar(c)).unwrap();
    }
    assert_eq!(app.modal.filtered_indices, vec![0]);

    // Press Esc to abort search
    app.handle_modal_action(ModalKeyAction::SearchAbort).unwrap();
    assert!(!app.modal.search_active);
    assert!(app.modal.is_open);
    // Cursor restored to prior position 2
    assert_eq!(app.modal.viewer_cursor, 2);
    assert_eq!(app.queue.cursor, 0);
}

#[test]
fn test_app_modal_search_in_shuffled_queue() {
    let (mut app, _server, _) = setup_test_app();

    // Toggle shuffle ON
    app.handle_action(KeyAction::ToggleShuffle).unwrap();
    assert!(app.queue.shuffle);

    // Find the title of track at index 2 in shuffled playback order
    let target_item_idx = 2;
    let target_track_idx = app.queue.playback_order[target_item_idx];
    let target_title = app.queue.tracks[target_track_idx].title.clone();

    app.open_modal();
    app.handle_modal_action(ModalKeyAction::SearchActivate).unwrap();

    // Search for target title
    for c in target_title.chars() {
        app.handle_modal_action(ModalKeyAction::SearchChar(c)).unwrap();
    }
    assert!(app.modal.filtered_indices.contains(&target_item_idx));

    // Resolve search
    app.handle_modal_action(ModalKeyAction::SearchResolve).unwrap();
    assert!(!app.modal.search_active);
    assert_eq!(
        app.modal.viewer_cursor, target_item_idx,
        "Viewer cursor must center on the track in shuffled queue context"
    );
}

#[test]
fn test_app_modal_selection_toggle_and_clear() {
    let (mut app, _server, _) = setup_test_app();
    app.open_modal();
    assert!(app.modal.is_open);
    assert!(app.modal.selection_set.is_empty());
    assert_eq!(app.modal.viewer_cursor, 0);

    // Toggle track 0 with Space
    app.handle_modal_action(ModalKeyAction::ToggleSelection).unwrap();
    assert_eq!(app.modal.selection_set, vec![0]);

    // Move cursor down to track 1
    app.handle_modal_action(ModalKeyAction::CursorDown).unwrap();
    assert_eq!(app.modal.viewer_cursor, 1);

    // Move cursor down to track 2 and toggle with Space
    app.handle_modal_action(ModalKeyAction::CursorDown).unwrap();
    assert_eq!(app.modal.viewer_cursor, 2);
    app.handle_modal_action(ModalKeyAction::ToggleSelection).unwrap();
    assert_eq!(app.modal.selection_set, vec![0, 2]); // Pick order: 0, 2

    // Toggle track 2 again -> unmarks track 2
    app.handle_modal_action(ModalKeyAction::ToggleSelection).unwrap();
    assert_eq!(app.modal.selection_set, vec![0]);

    // Toggle track 2 back on
    app.handle_modal_action(ModalKeyAction::ToggleSelection).unwrap();
    assert_eq!(app.modal.selection_set, vec![0, 2]);

    // Press 'c' -> empties selection set
    app.handle_modal_action(ModalKeyAction::ClearSelection).unwrap();
    assert!(app.modal.selection_set.is_empty());
}

#[test]
fn test_app_modal_commit_with_selections_rebuilds_queue_and_plays() {
    let (mut app, mut server, downloaded) = setup_test_app();
    // Tracks in app: [vid_0, vid_1, vid_2]

    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // When Commit with selections [2, 0] is pressed:
        // First track in new playback order is vid_2.
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        assert_eq!(val["command"][1], "https://www.youtube.com/watch?v=vid_2");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // Auto-run continuity: unpause on playback start
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "set_property");
        assert_eq!(val["command"][1], "pause");
        assert_eq!(val["command"][2], false);
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    app.open_modal();

    // Select track 2 first, then track 0:
    app.modal.viewer_cursor = 2;
    app.handle_modal_action(ModalKeyAction::ToggleSelection).unwrap();

    app.modal.viewer_cursor = 0;
    app.handle_modal_action(ModalKeyAction::ToggleSelection).unwrap();

    assert_eq!(app.modal.selection_set, vec![2, 0]);

    // Commit with Enter
    app.handle_modal_action(ModalKeyAction::Commit).unwrap();

    // 1. Modal closed
    assert!(!app.modal.is_open);

    // 2. Queue isolated to selected subset: [2, 0]
    assert_eq!(app.queue.playback_order, vec![2, 0]);
    assert_eq!(app.queue.cursor, 0);
    assert_eq!(app.queue.current_track().unwrap().id, "vid_2");

    // 3. Next track in queue is vid_0 -> prefetcher should be triggered for vid_0
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(downloaded.lock().unwrap().contains(&"vid_0".to_string()));

    responder.join().unwrap();
}

#[test]
fn test_app_modal_commit_empty_selection_preserves_single_jump() {
    let (mut app, mut server, _downloaded) = setup_test_app();
    // Tracks in app: [vid_0, vid_1, vid_2]

    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // Empty selection set commit on hovered track 1 -> loadfile vid_1
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        assert_eq!(val["command"][1], "https://www.youtube.com/watch?v=vid_1");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();

        // Auto-run continuity: unpause on playback start
        line.clear();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "set_property");
        assert_eq!(val["command"][1], "pause");
        assert_eq!(val["command"][2], false);
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    app.open_modal();
    assert!(app.modal.selection_set.is_empty());

    // Move to track 1 without selecting anything
    app.modal.viewer_cursor = 1;

    // Commit with Enter
    app.handle_modal_action(ModalKeyAction::Commit).unwrap();

    // 1. Modal closed
    assert!(!app.modal.is_open);

    // 2. Queue order preserved: [0, 1, 2], cursor jumped to 1
    assert_eq!(app.queue.playback_order, vec![0, 1, 2]);
    assert_eq!(app.queue.cursor, 1);
    assert_eq!(app.queue.current_track().unwrap().id, "vid_1");

    responder.join().unwrap();
}

#[test]
fn test_app_modal_search_typing_j_and_k_appends_to_query() {
    let (mut app, _server, _) = setup_test_app();
    app.open_modal();

    // Activate search
    app.handle_modal_action(ModalKeyAction::SearchActivate).unwrap();
    assert!(app.modal.search_active);

    fn make_key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }

    // Bare 'j' must parse to SearchChar and append to query
    let act_j = parse_modal_search_key_event(make_key(KeyCode::Char('j'))).unwrap();
    assert_eq!(act_j, ModalKeyAction::SearchChar('j'));
    app.handle_modal_action(act_j).unwrap();
    assert_eq!(app.modal.search_query, "j");

    // Bare 'k' must parse to SearchChar and append to query
    let act_k = parse_modal_search_key_event(make_key(KeyCode::Char('k'))).unwrap();
    assert_eq!(act_k, ModalKeyAction::SearchChar('k'));
    app.handle_modal_action(act_k).unwrap();
    assert_eq!(app.modal.search_query, "jk");
}

#[test]
fn test_app_keybinding_toggle_loop_via_capital_l_and_seek_via_lowercase_l() {
    let (mut app, mut server, _) = setup_test_app();

    fn make_key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }

    // 'L' must parse to ToggleLoop
    let act_loop = parse_key_event(make_key(KeyCode::Char('L'))).unwrap();
    assert_eq!(act_loop, KeyAction::ToggleLoop);
    assert!(!app.queue.single_loop);
    app.handle_action(act_loop).unwrap();
    assert!(app.queue.single_loop);
    app.handle_action(act_loop).unwrap();
    assert!(!app.queue.single_loop);

    // 'l' must parse to SeekForward
    let act_seek = parse_key_event(make_key(KeyCode::Char('l'))).unwrap();
    assert_eq!(act_seek, KeyAction::SeekForward);

    // Handle SeekForward triggers seek IPC
    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(&server);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "seek");
        assert_eq!(val["command"][1], "5");
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    app.handle_action(act_seek).unwrap();
    responder.join().unwrap();
}

#[test]
fn test_app_modal_cache_indicator_rendering() {
    let (mut app, _server, _) = setup_test_app();

    // Cache vid_0
    let cached_file = app.cache.cache_file_path("vid_0");
    std::fs::write(&cached_file, b"test audio content").unwrap();
    assert!(app.cache.is_cached("vid_0"));
    assert!(!app.cache.is_cached("vid_1"));

    app.open_modal();

    let mut buf = Vec::new();
    app.render_modal(&mut buf).unwrap();
    let rendered = String::from_utf8(buf).unwrap();

    // Line with vid_0 (Track Zero) must have [⚡]
    assert!(rendered.contains("[⚡]"), "Modal must render [⚡] for cached track vid_0");
    assert!(rendered.contains("Track Zero"));
    assert!(rendered.contains("Track One"));
}



