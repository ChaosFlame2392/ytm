use std::fs::{self, FileTimes};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime};
use ytm::{
    build_ytdlp_command_with_cookies, configure_mpv_command_with_cookies, default_cache_dir,
    default_config_dir, default_cookies_file, AudioCache, AudioDownloader, AudioPrefetcher,
    AudioSource, Browser, CookieSource, PlaylistManager, Queue, Track, DEFAULT_MAX_CACHE_BYTES,
    DEFAULT_TARGET_CACHE_BYTES,
};

#[test]
fn test_cache_default_limits_and_env_dir() {
    assert_eq!(DEFAULT_MAX_CACHE_BYTES, 500 * 1024 * 1024);
    assert_eq!(DEFAULT_TARGET_CACHE_BYTES, 400 * 1024 * 1024);

    let base_custom = std::env::temp_dir().join("ytm_custom_env_dir");
    std::env::set_var("YTM_CACHE_DIR", base_custom.to_str().unwrap());

    assert_eq!(default_cache_dir(), base_custom);
    assert_eq!(PlaylistManager::default_cache_dir(), base_custom);
    assert_eq!(AudioCache::default_audio_dir(), base_custom.join("audio"));

    let cache = AudioCache::default();
    assert_eq!(cache.audio_dir, base_custom.join("audio"));
    assert_eq!(cache.max_bytes, DEFAULT_MAX_CACHE_BYTES);
    assert_eq!(cache.target_bytes, DEFAULT_TARGET_CACHE_BYTES);

    std::env::remove_var("YTM_CACHE_DIR");
}

#[test]
fn test_cache_directory_creation_and_resolution() {
    let test_dir = std::env::temp_dir().join("ytm_integration_cache_dir");
    let _ = fs::remove_dir_all(&test_dir);

    let cache = AudioCache::new(test_dir.clone());
    let vid = "dQw4w9WgXcQ";

    // Directory doesn't exist yet
    assert!(!test_dir.exists());
    assert!(!cache.is_cached(vid));
    assert_eq!(
        cache.get_audio_source(vid),
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
    );

    let resolved = cache.resolve_source(vid);
    assert_eq!(
        resolved,
        AudioSource::Remote("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string())
    );
    assert!(!resolved.is_local());
    assert_eq!(resolved.as_str(), "https://www.youtube.com/watch?v=dQw4w9WgXcQ");

    // Create cache directory & local audio file
    cache.ensure_audio_dir().expect("ensure_audio_dir should succeed");
    assert!(test_dir.exists());

    let target_file = cache.cache_file_path(vid);
    fs::write(&target_file, b"OPUS_AUDIO_STREAM_DATA").unwrap();

    assert!(cache.is_cached(vid));
    assert_eq!(
        cache.get_audio_source(vid),
        target_file.to_string_lossy().to_string()
    );
    let resolved_local = cache.resolve_source(vid);
    assert!(resolved_local.is_local());
    assert_eq!(resolved_local, AudioSource::Local(target_file));

    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn test_lru_pruning_calculation_and_order() {
    let test_dir = std::env::temp_dir().join("ytm_integration_lru_prune");
    let _ = fs::remove_dir_all(&test_dir);
    fs::create_dir_all(&test_dir).unwrap();

    // Limit: max 1200 bytes, prune down to 700 bytes
    let cache = AudioCache::with_limits(test_dir.clone(), 1200, 700);

    let base_time = SystemTime::now() - Duration::from_secs(7200);

    let create_track_file = |name: &str, size: usize, age_secs: u64| -> PathBuf {
        let p = test_dir.join(format!("{}.opus", name));
        fs::write(&p, vec![b'a'; size]).unwrap();
        let file = fs::File::open(&p).unwrap();
        let t = base_time + Duration::from_secs(age_secs);
        file.set_times(FileTimes::new().set_accessed(t).set_modified(t)).unwrap();
        p
    };

    let t1 = create_track_file("vid1", 400, 10); // oldest
    let t2 = create_track_file("vid2", 500, 20); // 2nd oldest
    let t3 = create_track_file("vid3", 300, 30); // 3rd oldest
    let t4 = create_track_file("vid4", 300, 40); // newest

    // Total = 400 + 500 + 300 + 300 = 1500 > 1200
    assert_eq!(cache.total_cache_size().unwrap(), 1500);

    // Prune LRU
    // t1 deleted (-400 -> 1100 > 700)
    // t2 deleted (-500 -> 600 <= 700) -> done
    let report = cache.prune_lru().unwrap();
    assert_eq!(report.files_deleted, 2);
    assert_eq!(report.bytes_deleted, 900);
    assert_eq!(report.remaining_bytes, 600);

    assert!(!t1.exists(), "vid1 (oldest) should be deleted");
    assert!(!t2.exists(), "vid2 (second oldest) should be deleted");
    assert!(t3.exists(), "vid3 should be kept");
    assert!(t4.exists(), "vid4 should be kept");
    assert_eq!(cache.total_cache_size().unwrap(), 600);

    let _ = fs::remove_dir_all(&test_dir);
}

struct TestDownloader {
    call_count: Arc<AtomicUsize>,
    download_size: usize,
}

impl AudioDownloader for TestDownloader {
    fn download(&self, _video_id: &str, tmp_path: &Path) -> Result<(), String> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        fs::write(tmp_path, vec![b'x'; self.download_size]).map_err(|e| e.to_string())
    }
}

#[test]
fn test_prefetcher_queueing_and_deduplication() {
    let test_dir = std::env::temp_dir().join("ytm_integration_prefetch_queue");
    let _ = fs::remove_dir_all(&test_dir);
    fs::create_dir_all(&test_dir).unwrap();

    let cache = AudioCache::new(test_dir.clone());
    let counter = Arc::new(AtomicUsize::new(0));
    let downloader = TestDownloader {
        call_count: counter.clone(),
        download_size: 1024,
    };

    let prefetcher = AudioPrefetcher::with_downloader(cache.clone(), downloader);

    let queue = Queue::new(vec![
        Track {
            index: 0,
            id: "current_song".to_string(),
            title: "Current Song".to_string(),
        },
        Track {
            index: 1,
            id: "next_song".to_string(),
            title: "Next Song".to_string(),
        },
    ]);

    // Prefetch next track from queue
    let queued = prefetcher.prefetch_next(&queue);
    assert!(queued, "prefetch_next should queue 'next_song'");

    // Wait for worker to finish
    for _ in 0..50 {
        if !prefetcher.is_in_flight("next_song") {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    assert!(cache.is_cached("next_song"));
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    // Redundant prefetch should return false and not re-run downloader
    assert!(!prefetcher.prefetch_next(&queue));
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn test_auth_resolution_hierarchy_and_argument_formatting_offline() {
    use std::process::Command;

    // Verify all supported browser variants parse properly
    let browsers = [
        ("safari", Browser::Safari),
        ("firefox", Browser::Firefox),
        ("chrome", Browser::Chrome),
        ("brave", Browser::Brave),
        ("edge", Browser::Edge),
        ("chromium", Browser::Chromium),
    ];

    for (name, browser) in browsers {
        assert_eq!(Browser::parse(name), Some(browser));
        let src = CookieSource::Browser(browser);
        assert_eq!(src.ytdlp_args(), vec!["--cookies-from-browser", name]);
        assert_eq!(src.mpv_arg(), format!("--ytdl-raw-options=cookies-from-browser={}", name));

        // Command builders offline formatting check
        let yt_cmd = build_ytdlp_command_with_cookies(Some(&src));
        let yt_args: Vec<String> = yt_cmd.get_args().map(|s| s.to_string_lossy().into_owned()).collect();
        assert_eq!(yt_args, vec!["--no-warnings", "--cookies-from-browser", name]);

        let mut mpv_cmd = Command::new("mpv");
        configure_mpv_command_with_cookies(&mut mpv_cmd, Some(&src));
        let mpv_args: Vec<String> = mpv_cmd.get_args().map(|s| s.to_string_lossy().into_owned()).collect();
        assert_eq!(mpv_args, vec![format!("--ytdl-raw-options=cookies-from-browser={}", name)]);
    }

    // Verify file cookies
    let file_src = CookieSource::File(PathBuf::from("/non-disposable/cookies.txt"));
    assert_eq!(file_src.ytdlp_args(), vec!["--cookies", "/non-disposable/cookies.txt"]);
    assert_eq!(file_src.mpv_arg(), "--ytdl-raw-options=cookies=/non-disposable/cookies.txt");

    let yt_file_cmd = build_ytdlp_command_with_cookies(Some(&file_src));
    let yt_file_args: Vec<String> = yt_file_cmd.get_args().map(|s| s.to_string_lossy().into_owned()).collect();
    assert_eq!(yt_file_args, vec!["--no-warnings", "--cookies", "/non-disposable/cookies.txt"]);

    // Hierarchy test: YTM_COOKIES_FILE overrides browser & config file
    let resolved = CookieSource::resolve_hierarchy(
        Some("/top/priority/cookies.txt"),
        Some("brave"),
        Some(PathBuf::from("/config/cookies.txt")),
        Some(Browser::Firefox),
        |p| p == Path::new("/top/priority/cookies.txt"),
    );
    assert_eq!(resolved, Some(CookieSource::File(PathBuf::from("/top/priority/cookies.txt"))));

    // Config dir lookup
    let _ = default_config_dir();
    let _ = default_cookies_file();
}
