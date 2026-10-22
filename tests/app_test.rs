use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use ytm::app::App;
use ytm::cache::AudioCache;
use ytm::mpv::{EndFileReason, MpvController, MpvEvent};
use ytm::playlist::Track;
use ytm::prefetch::{AudioDownloader, AudioPrefetcher};
use ytm::queue::Queue;
use ytm::ui::KeyAction;

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
    let dir = std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()));
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

    assert!(rendered.contains("[▶] Track Zero (00:45 / 03:30)"));
    assert!(rendered.contains("[Shuffle: OFF] [Loop: OFF]  Next: Track One"));

    responder.join().unwrap();
}

