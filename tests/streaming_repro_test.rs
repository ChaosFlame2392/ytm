use std::time::Duration;
use ytm::mpv::{EndFileReason, MpvController, MpvEvent};
use ytm::prefetch::{AudioDownloader, YtDlpDownloader};

#[test]
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
