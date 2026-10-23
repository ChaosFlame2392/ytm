use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ytm::app::App;
use ytm::auth::{
    build_ytdlp_command_with_cookies, configure_mpv_command_with_cookies, Browser, CookieSource,
};
use ytm::cache::{AudioCache, AudioSource};
use ytm::mpv::{EndFileReason, MpvController, MpvEvent};
use ytm::playlist::Track;
use ytm::prefetch::{AudioDownloader, AudioPrefetcher, YtDlpDownloader};
use ytm::queue::Queue;

#[test]
#[ignore = "requires live internet connection and YouTube access"]
fn test_repro_prefetch_download_uncached_track() {
    let video_id = "5MT2Hsfs4Rw";
    let tmp_dir = std::env::temp_dir().join(format!("ytm-dl-test-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).unwrap();
    let tmp_file = tmp_dir.join("test.opus");

    let downloader = YtDlpDownloader;
    let download_result = downloader.download(video_id, &tmp_file);
    let _ = std::fs::remove_dir_all(&tmp_dir);

    assert!(
        download_result.is_ok(),
        "Prefetch download failed: {:?}",
        download_result.err()
    );
}

#[test]
#[ignore = "requires live internet connection and YouTube access"]
fn test_repro_mpv_stream_uncached_track() {
    let video_id = "5MT2Hsfs4Rw";
    let url = format!("https://www.youtube.com/watch?v={}", video_id);

    let socket = format!("/tmp/ytm-stream-test-{}.sock", std::process::id());
    let mut controller = MpvController::start_with_socket(&socket).expect("Failed to start mpv");

    controller.loadfile(&url).expect("loadfile command failed");

    let mut got_error = false;
    let mut got_playback_restart = false;
    for _ in 0..40 {
        if let Ok(event) = controller.recv_event_timeout(Duration::from_millis(250)) {
            match event {
                MpvEvent::PlaybackRestart => {
                    got_playback_restart = true;
                    break;
                }
                MpvEvent::EndFile { reason } => {
                    if matches!(reason, EndFileReason::Error | EndFileReason::Other(_)) {
                        got_error = true;
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    let _ = controller.quit();

    assert!(
        got_playback_restart && !got_error,
        "MPV failed to stream track URL (got_error={}, got_playback_restart={})",
        got_error,
        got_playback_restart
    );
}

// ============================================================================
// Hermetic Offline Mock Tests (08-3: Deterministic, fast, zero network access)
// ============================================================================

struct MockFailureDownloader {
    error_msg: String,
    attempts: Arc<Mutex<Vec<String>>>,
}

impl AudioDownloader for MockFailureDownloader {
    fn download(&self, video_id: &str, tmp_path: &Path) -> Result<(), String> {
        self.attempts.lock().unwrap().push(video_id.to_string());
        // Simulate writing partial/corrupted data before failing
        let _ = std::fs::write(tmp_path, b"PARTIAL_BOT_CHALLENGE_HTML");
        Err(self.error_msg.clone())
    }
}

fn temp_test_dir(prefix: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("{}-{}-{}", prefix, std::process::id(), count));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn create_hermetic_test_app() -> (App, UnixStream) {
    let (client_stream, server_stream) = UnixStream::pair().unwrap();
    let mpv = MpvController::from_stream(client_stream);

    let temp_dir = temp_test_dir("ytm-hermetic-test");
    let cache = AudioCache::new(temp_dir);

    struct NoOpDownloader;
    impl AudioDownloader for NoOpDownloader {
        fn download(&self, _video_id: &str, _tmp_path: &Path) -> Result<(), String> {
            Ok(())
        }
    }

    let prefetcher = AudioPrefetcher::with_downloader(cache.clone(), NoOpDownloader);
    let tracks = vec![
        Track {
            index: 0,
            id: "stream_fail_0".to_string(),
            title: "Failing Stream Track".to_string(),
        },
        Track {
            index: 1,
            id: "stream_ok_1".to_string(),
            title: "Backup Stream Track".to_string(),
        },
        Track {
            index: 2,
            id: "stream_ok_2".to_string(),
            title: "Final Stream Track".to_string(),
        },
    ];
    let queue = Queue::new(tracks);
    let app = App::new(queue, cache, prefetcher, mpv);
    (app, server_stream)
}

#[test]
fn test_hermetic_stream_error_handling_and_auto_advance() {
    let (mut app, mut server) = create_hermetic_test_app();

    // Background thread to handle mpv IPC commands generated when stream error triggers advancement
    let responder = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut line = String::new();

        // Expect loadfile command for the next track ("stream_ok_1")
        reader.read_line(&mut line).unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["command"][0], "loadfile");
        assert_eq!(
            val["command"][1],
            "https://www.youtube.com/watch?v=stream_ok_1"
        );
        let req_id = val["request_id"].as_u64().unwrap();
        server
            .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
            .unwrap();
        server.write_all(b"\n").unwrap();
    });

    assert_eq!(app.queue.cursor, 0);
    assert!(app.error_message.is_none());

    // Simulate backend player reporting error (e.g., bot detection challenge or 403 Forbidden)
    app.handle_mpv_event(MpvEvent::EndFile {
        reason: EndFileReason::Error,
    })
    .expect("handle_mpv_event should process EndFileReason::Error");

    // Queue must automatically advance to index 1
    assert_eq!(app.queue.cursor, 1);
    assert_eq!(app.queue.current_track().unwrap().id, "stream_ok_1");

    // Error badge must be surfaced in app status
    assert_eq!(app.error_message.as_deref(), Some("[!] Stream Error"));
    let status = app.current_status_state();
    assert_eq!(status.error_message, Some("[!] Stream Error"));
    let (_l1, l2) = status.format_lines();
    assert!(l2.contains("[!] Stream Error"));

    responder.join().unwrap();
}

#[test]
fn test_hermetic_prefetch_bot_verification_and_http_403_resilience() {
    let temp_dir = temp_test_dir("ytm-hermetic-prefetch");

    let cache = AudioCache::new(temp_dir.clone());
    let attempts = Arc::new(Mutex::new(Vec::new()));
    let bot_error = "Sign in to confirm you're not a bot. This helps protect our community. [HTTP 403]";

    let downloader = MockFailureDownloader {
        error_msg: bot_error.to_string(),
        attempts: attempts.clone(),
    };

    let prefetcher = AudioPrefetcher::with_downloader(cache.clone(), downloader);
    let video_id = "bot_challenged_vid";

    // Request prefetch for blocked track
    assert!(prefetcher.prefetch(video_id));

    // Wait for worker to finish processing failure
    for _ in 0..50 {
        if !prefetcher.is_in_flight(video_id) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    // Verify worker recovered cleanly:
    // 1. In-flight marker cleared
    assert!(!prefetcher.is_in_flight(video_id));
    // 2. Track not marked as cached
    assert!(!cache.is_cached(video_id));
    // 3. Temporary partial file pruned/removed, no orphan garbage left on disk
    let leftover_files = std::fs::read_dir(&temp_dir).unwrap().count();
    assert_eq!(leftover_files, 0, "Prefetch failure must clean up partial temp files");
    // 4. Downloader was invoked
    assert_eq!(attempts.lock().unwrap().as_slice(), &[video_id]);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

fn assert_command_args(
    source: Option<&CookieSource>,
    expected_ytdlp: &[&str],
    expected_mpv: &[&str],
) {
    use std::process::Command;

    let yt_cmd = build_ytdlp_command_with_cookies(source);
    let yt_args: Vec<String> = yt_cmd
        .get_args()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    assert_eq!(yt_args, expected_ytdlp);

    let mut mpv_cmd = Command::new("mpv");
    configure_mpv_command_with_cookies(&mut mpv_cmd, source);
    let mpv_args: Vec<String> = mpv_cmd
        .get_args()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    assert_eq!(mpv_args, expected_mpv);
}

#[test]
fn test_hermetic_cookie_resolution_and_command_configuration() {
    // 1. File cookie source formatting
    let cookie_file = PathBuf::from("/etc/ytm/cookies.txt");
    let file_src = CookieSource::File(cookie_file);
    assert_eq!(file_src.ytdlp_args(), vec!["--cookies", "/etc/ytm/cookies.txt"]);
    assert_eq!(
        file_src.mpv_arg(),
        "--ytdl-raw-options=cookies=/etc/ytm/cookies.txt"
    );
    assert_command_args(
        Some(&file_src),
        &["--no-warnings", "--cookies", "/etc/ytm/cookies.txt"],
        &["--ytdl-raw-options=cookies=/etc/ytm/cookies.txt"],
    );

    // 2. Browser cookie source formatting
    let browser_src = CookieSource::Browser(Browser::Chrome);
    assert_eq!(browser_src.ytdlp_args(), vec!["--cookies-from-browser", "chrome"]);
    assert_eq!(
        browser_src.mpv_arg(),
        "--ytdl-raw-options=cookies-from-browser=chrome"
    );
    assert_command_args(
        Some(&browser_src),
        &["--no-warnings", "--cookies-from-browser", "chrome"],
        &["--ytdl-raw-options=cookies-from-browser=chrome"],
    );

    // 3. None cookie source
    assert_command_args(None, &["--no-warnings"], &[]);

    // 4. Hierarchy resolution verification:
    // Env file override takes precedence over env browser, config path, and auto-detect
    let res = CookieSource::resolve_hierarchy(
        Some("/env/override/cookies.txt"),
        Some("brave"),
        Some(PathBuf::from("/config/cookies.txt")),
        Some(Browser::Firefox),
        |p| p == Path::new("/env/override/cookies.txt"),
    );
    assert_eq!(res, Some(CookieSource::File(PathBuf::from("/env/override/cookies.txt"))));

    // Env browser takes precedence over config path and auto-detect
    let res_browser = CookieSource::resolve_hierarchy(
        None,
        Some("brave"),
        Some(PathBuf::from("/config/cookies.txt")),
        Some(Browser::Firefox),
        |_| true,
    );
    assert_eq!(res_browser, Some(CookieSource::Browser(Browser::Brave)));

    // User config file takes precedence over auto-detect if file exists
    let res_config = CookieSource::resolve_hierarchy(
        None,
        None,
        Some(PathBuf::from("/config/cookies.txt")),
        Some(Browser::Firefox),
        |p| p == Path::new("/config/cookies.txt"),
    );
    assert_eq!(res_config, Some(CookieSource::File(PathBuf::from("/config/cookies.txt"))));

    // Auto-detect is used if no user config file exists
    let res_autodetect = CookieSource::resolve_hierarchy(
        None,
        None,
        Some(PathBuf::from("/config/cookies.txt")),
        Some(Browser::Firefox),
        |_| false,
    );
    assert_eq!(res_autodetect, Some(CookieSource::Browser(Browser::Firefox)));

    // None if nothing configured and auto-detect returns None
    let res_empty = CookieSource::resolve_hierarchy(
        None,
        None,
        None,
        None,
        |_| false,
    );
    assert_eq!(res_empty, None);
}

#[test]
fn test_hermetic_stream_resolution_cached_vs_remote() {
    let temp_dir = temp_test_dir("ytm-hermetic-source");

    let cache = AudioCache::new(temp_dir.clone());

    // Uncached track resolves to YouTube watch URL
    let uncached = cache.resolve_source("uncached_stream_test");
    assert!(!uncached.is_local());
    assert_eq!(
        uncached,
        AudioSource::Remote("https://www.youtube.com/watch?v=uncached_stream_test".to_string())
    );
    assert_eq!(
        uncached.as_str(),
        "https://www.youtube.com/watch?v=uncached_stream_test"
    );

    // Cached track resolves to local file path
    let local_file = cache.cache_file_path("cached_local_test");
    std::fs::write(&local_file, b"MOCK_OPUS_AUDIO_STREAM").unwrap();

    let cached = cache.resolve_source("cached_local_test");
    assert!(cached.is_local());
    assert_eq!(cached, AudioSource::Local(local_file.clone()));
    assert_eq!(cached.as_str(), local_file.to_str().unwrap());

    let _ = std::fs::remove_dir_all(&temp_dir);
}
