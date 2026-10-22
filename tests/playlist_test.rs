use ytm::playlist::{format_tracks_tsv, parse_tracks_tsv, parse_ytdlp_output, Track};

#[test]
fn test_parse_ytdlp_output_and_format_tsv() {
    let raw_ytdlp = "dQw4w9WgXcQ\tRick Astley - Never Gonna Give You Up\n9bZkp7q19f0\tPSY - GANGNAM STYLE\n";
    let tracks = parse_ytdlp_output(raw_ytdlp);

    assert_eq!(tracks.len(), 2);
    assert_eq!(
        tracks[0],
        Track {
            index: 0,
            id: "dQw4w9WgXcQ".to_string(),
            title: "Rick Astley - Never Gonna Give You Up".to_string(),
        }
    );
    assert_eq!(
        tracks[1],
        Track {
            index: 1,
            id: "9bZkp7q19f0".to_string(),
            title: "PSY - GANGNAM STYLE".to_string(),
        }
    );

    let tsv_output = format_tracks_tsv(&tracks);
    assert_eq!(
        tsv_output,
        "0\tdQw4w9WgXcQ\tRick Astley - Never Gonna Give You Up\n1\t9bZkp7q19f0\tPSY - GANGNAM STYLE\n"
    );

    let parsed_back = parse_tracks_tsv(&tsv_output);
    assert_eq!(parsed_back, tracks);
}

#[test]
fn test_playlist_manager_active_url_default_and_update() {
    let temp_dir = std::env::temp_dir().join(format!("ytm_test_url_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let manager = ytm::playlist::PlaylistManager::new(temp_dir.clone());

    // Should return default URL when file does not exist
    assert_eq!(
        manager.get_active_url(),
        ytm::playlist::DEFAULT_PLAYLIST_URL
    );

    // Should save new URL and read it back
    let new_url = "https://music.youtube.com/playlist?list=PLcustom_playlist_id";
    manager.set_active_url(new_url).expect("failed to save active url");

    assert_eq!(manager.get_active_url(), new_url);

    // Verify file actually contains the URL
    let saved_url = std::fs::read_to_string(temp_dir.join("playlist.url")).unwrap();
    assert_eq!(saved_url.trim(), new_url);

    // Cleanup
    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_playlist_cache_freshness() {
    let temp_dir = std::env::temp_dir().join(format!("ytm_test_fresh_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let manager = ytm::playlist::PlaylistManager::new(temp_dir.clone());

    // When tracks.tsv does not exist, cache needs sync
    assert!(manager.cache_needs_sync());

    // Create a tracks.tsv file
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::fs::write(manager.tracks_file_path(), "0\ttest\tTrack\n").unwrap();

    // Now cache is fresh
    assert!(!manager.cache_needs_sync());

    // Test with simulated future time: 10 minutes later -> still fresh
    let now = std::time::SystemTime::now();
    let ten_mins_later = now + std::time::Duration::from_secs(10 * 60);
    assert!(!manager.cache_needs_sync_at(ten_mins_later));

    // 16 minutes later -> needs sync
    let sixteen_mins_later = now + std::time::Duration::from_secs(16 * 60);
    assert!(manager.cache_needs_sync_at(sixteen_mins_later));

    // Cleanup
    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_playlist_save_load_and_sync_orchestration() {
    let temp_dir = std::env::temp_dir().join(format!("ytm_test_sync_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let manager = ytm::playlist::PlaylistManager::new(temp_dir.clone());

    // 1. Initial load with missing cache triggers sync
    let mut extractor_calls = 0;
    let mock_extractor = |_url: &str| -> Result<String, String> {
        extractor_calls += 1;
        Ok("id1\tSong One\nid2\tSong Two\n".to_string())
    };

    let tracks = manager
        .load_or_sync_with_extractor(None, false, mock_extractor)
        .expect("initial sync should succeed");

    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].title, "Song One");
    assert_eq!(tracks[1].title, "Song Two");
    assert_eq!(extractor_calls, 1);

    // Verify tracks.tsv was saved to disk
    assert!(manager.tracks_file_path().exists());
    let loaded = manager.load_tracks().expect("should load tracks from disk");
    assert_eq!(loaded, tracks);

    // 2. Second load without force_sync and fresh cache should NOT call extractor
    let mut extractor_calls_2 = 0;
    let mock_extractor_2 = |_url: &str| -> Result<String, String> {
        extractor_calls_2 += 1;
        Ok("".to_string())
    };

    let cached_tracks = manager
        .load_or_sync_with_extractor(None, false, mock_extractor_2)
        .expect("should read from cache");
    assert_eq!(cached_tracks, tracks);
    assert_eq!(extractor_calls_2, 0, "should not call extractor when cache is fresh");

    // 3. Passing --sync (force_sync = true) should trigger extractor even if cache is fresh
    let mut extractor_calls_3 = 0;
    let mock_extractor_3 = |_url: &str| -> Result<String, String> {
        extractor_calls_3 += 1;
        Ok("id3\tSong Three\n".to_string())
    };

    let refreshed_tracks = manager
        .load_or_sync_with_extractor(None, true, mock_extractor_3)
        .expect("force sync should succeed");
    assert_eq!(refreshed_tracks.len(), 1);
    assert_eq!(refreshed_tracks[0].id, "id3");
    assert_eq!(extractor_calls_3, 1, "force sync must call extractor");

    // 4. Passing a new URL should update active url and trigger sync
    let new_url = "https://music.youtube.com/playlist?list=PLnew_playlist";
    let mut extractor_calls_4 = 0;
    let mock_extractor_4 = |url: &str| -> Result<String, String> {
        extractor_calls_4 += 1;
        assert_eq!(url, new_url);
        Ok("id4\tSong Four\n".to_string())
    };

    let new_url_tracks = manager
        .load_or_sync_with_extractor(Some(new_url), false, mock_extractor_4)
        .expect("new url sync should succeed");
    assert_eq!(new_url_tracks.len(), 1);
    assert_eq!(new_url_tracks[0].id, "id4");
    assert_eq!(extractor_calls_4, 1);
    assert_eq!(manager.get_active_url(), new_url);

    // Cleanup
    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_parse_cli_args() {
    use ytm::playlist::parse_cli_args;

    // No args (just binary name)
    let (url, sync) = parse_cli_args(vec!["ytm".to_string()]);
    assert_eq!(url, None);
    assert!(!sync);

    // Only --sync
    let (url, sync) = parse_cli_args(vec!["ytm".to_string(), "--sync".to_string()]);
    assert_eq!(url, None);
    assert!(sync);

    // Only URL
    let test_url = "https://music.youtube.com/playlist?list=PL123";
    let (url, sync) = parse_cli_args(vec!["ytm".to_string(), test_url.to_string()]);
    assert_eq!(url, Some(test_url.to_string()));
    assert!(!sync);

    // URL and --sync
    let (url, sync) = parse_cli_args(vec![
        "ytm".to_string(),
        "--sync".to_string(),
        test_url.to_string(),
    ]);
    assert_eq!(url, Some(test_url.to_string()));
    assert!(sync);
}

#[test]
fn test_parse_tracks_tsv_edge_cases() {
    let tsv_content = r#"
0	id_one	First Track with regular title
1	id_two	Second Track	with extra tabs	in title

2	id_three	Third Track
"#;
    let tracks = ytm::playlist::parse_tracks_tsv(tsv_content);
    assert_eq!(tracks.len(), 3);
    assert_eq!(tracks[0].id, "id_one");
    assert_eq!(tracks[0].title, "First Track with regular title");
    assert_eq!(tracks[1].id, "id_two");
    assert_eq!(tracks[1].title, "Second Track\twith extra tabs\tin title");
    assert_eq!(tracks[2].id, "id_three");
    assert_eq!(tracks[2].title, "Third Track");
}




