use crate::auth::build_ytdlp_command;
use crate::cache::AudioCache;
use crate::queue::Queue;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};

pub trait AudioDownloader: Send + Sync {
    fn download(&self, video_id: &str, tmp_path: &Path) -> Result<(), String>;
}

pub struct YtDlpDownloader;

impl AudioDownloader for YtDlpDownloader {
    fn download(&self, video_id: &str, tmp_path: &Path) -> Result<(), String> {
        let url = format!("https://www.youtube.com/watch?v={}", video_id);
        let tmp_path_str = tmp_path
            .to_str()
            .ok_or_else(|| "Invalid UTF-8 in temp path".to_string())?;

        let mut cmd = build_ytdlp_command();
        cmd.args([
            "-x",
            "--audio-format",
            "opus",
            "-o",
            tmp_path_str,
        ]);
        cmd.arg(&url);

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to execute yt-dlp: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("yt-dlp exited with {}: {}", output.status, stderr));
        }

        Ok(())
    }
}

enum PrefetchTask {
    Download(String),
    Stop,
}

struct PrefetcherInner {
    cache: AudioCache,
    in_flight: Arc<Mutex<HashSet<String>>>,
    sender: mpsc::Sender<PrefetchTask>,
    worker_handle: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for PrefetcherInner {
    fn drop(&mut self) {
        let _ = self.sender.send(PrefetchTask::Stop);
        if let Ok(mut guard) = self.worker_handle.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
    }
}

#[derive(Clone)]
pub struct AudioPrefetcher {
    inner: Arc<PrefetcherInner>,
}

impl AudioPrefetcher {
    pub fn new(cache: AudioCache) -> Self {
        Self::with_downloader(cache, YtDlpDownloader)
    }

    pub fn with_downloader<D: AudioDownloader + 'static>(
        cache: AudioCache,
        downloader: D,
    ) -> Self {
        let (sender, receiver) = mpsc::channel::<PrefetchTask>();
        let in_flight = Arc::new(Mutex::new(HashSet::<String>::new()));
        let downloader = Arc::new(downloader);

        let cache_worker = cache.clone();
        let in_flight_worker = in_flight.clone();

        let handle = thread::spawn(move || {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);

            while let Ok(task) = receiver.recv() {
                match task {
                    PrefetchTask::Download(video_id) => {
                        // If file is already cached, skip download
                        if cache_worker.is_cached(&video_id) {
                            if let Ok(mut set) = in_flight_worker.lock() {
                                set.remove(&video_id);
                            }
                            continue;
                        }

                        let _ = cache_worker.ensure_audio_dir();

                        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
                        let tmp_file_name = format!(
                            "{}.part-{}-{}.opus",
                            video_id,
                            std::process::id(),
                            seq
                        );
                        let tmp_path = cache_worker.audio_dir.join(&tmp_file_name);
                        let final_path = cache_worker.cache_file_path(&video_id);

                        let res = downloader.download(&video_id, &tmp_path);
                        let is_raw = crossterm::terminal::is_raw_mode_enabled().unwrap_or(false);
                        match res {
                            Ok(()) => {
                                if tmp_path.exists() {
                                    if let Err(e) = fs::rename(&tmp_path, &final_path) {
                                        if !is_raw {
                                            eprintln!(
                                                "Error renaming prefetch temp {:?} to {:?}: {}",
                                                tmp_path, final_path, e
                                            );
                                        }
                                        let _ = fs::remove_file(&tmp_path);
                                    } else {
                                        let _ = cache_worker.prune_lru();
                                    }
                                }
                            }
                            Err(e) => {
                                if !is_raw {
                                    eprintln!("Prefetch error for {}: {}", video_id, e);
                                }
                                if tmp_path.exists() {
                                    let _ = fs::remove_file(&tmp_path);
                                }
                            }
                        }

                        if let Ok(mut set) = in_flight_worker.lock() {
                            set.remove(&video_id);
                        }
                    }
                    PrefetchTask::Stop => break,
                }
            }
        });

        Self {
            inner: Arc::new(PrefetcherInner {
                cache,
                in_flight,
                sender,
                worker_handle: Mutex::new(Some(handle)),
            }),
        }
    }


    pub fn cache(&self) -> &AudioCache {
        &self.inner.cache
    }

    pub fn is_cached(&self, video_id: &str) -> bool {
        self.inner.cache.is_cached(video_id)
    }

    pub fn is_in_flight(&self, video_id: &str) -> bool {
        let guard = match self.inner.in_flight.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.contains(video_id)
    }

    pub fn in_flight_count(&self) -> usize {
        let guard = match self.inner.in_flight.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.len()
    }

    pub fn prefetch(&self, video_id: &str) -> bool {
        if self.inner.cache.is_cached(video_id) {
            return false;
        }

        let mut guard = match self.inner.in_flight.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };

        if guard.contains(video_id) {
            return false;
        }

        guard.insert(video_id.to_string());
        if self
            .inner
            .sender
            .send(PrefetchTask::Download(video_id.to_string()))
            .is_err()
        {
            guard.remove(video_id);
            return false;
        }

        true
    }

    pub fn prefetch_next(&self, queue: &Queue) -> bool {
        if let Some(track) = queue.peek_next() {
            self.prefetch(&track.id)
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct MockDownloader {
        download_calls: Arc<AtomicUsize>,
        simulate_delay_ms: u64,
        fail: bool,
    }

    impl AudioDownloader for MockDownloader {
        fn download(&self, video_id: &str, tmp_path: &Path) -> Result<(), String> {
            self.download_calls.fetch_add(1, Ordering::SeqCst);
            if self.simulate_delay_ms > 0 {
                thread::sleep(Duration::from_millis(self.simulate_delay_ms));
            }
            if self.fail {
                return Err("Simulated download failure".to_string());
            }
            // Write dummy audio to tmp_path
            fs::write(tmp_path, format!("mock opus audio for {}", video_id))
                .map_err(|e| e.to_string())?;
            Ok(())
        }
    }

    #[test]
    fn test_prefetch_deduplication_and_completion() {
        let temp_dir = std::env::temp_dir().join("ytm_test_prefetch_dedup");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let cache = AudioCache::new(temp_dir.clone());
        let calls = Arc::new(AtomicUsize::new(0));
        let mock = MockDownloader {
            download_calls: calls.clone(),
            simulate_delay_ms: 50,
            fail: false,
        };

        let prefetcher = AudioPrefetcher::with_downloader(cache.clone(), mock);

        let video_id = "test_vid_1";
        // 1. Initial prefetch should return true and mark as in-flight
        let enqueued1 = prefetcher.prefetch(video_id);
        assert!(enqueued1, "First prefetch call should succeed");
        assert!(prefetcher.is_in_flight(video_id), "Should be in-flight");

        // 2. Duplicate prefetch call while in-flight should return false (deduplicated)
        let enqueued2 = prefetcher.prefetch(video_id);
        assert!(!enqueued2, "Duplicate prefetch should be deduplicated");

        // Wait for download to complete
        for _ in 0..50 {
            if !prefetcher.is_in_flight(video_id) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(!prefetcher.is_in_flight(video_id), "Should no longer be in-flight");
        assert!(cache.is_cached(video_id), "File should now be cached on disk");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "Downloader should only be called once");

        // 3. Prefetch after it is already cached should return false
        let enqueued3 = prefetcher.prefetch(video_id);
        assert!(!enqueued3, "Already cached video should not be prefetch-enqueued");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "Downloader should not be called again");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_prefetch_next_with_queue() {
        use crate::playlist::Track;

        let temp_dir = std::env::temp_dir().join("ytm_test_prefetch_queue");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let cache = AudioCache::new(temp_dir.clone());
        let calls = Arc::new(AtomicUsize::new(0));
        let mock = MockDownloader {
            download_calls: calls.clone(),
            simulate_delay_ms: 10,
            fail: false,
        };

        let prefetcher = AudioPrefetcher::with_downloader(cache.clone(), mock);

        // Empty queue
        let empty_queue = Queue::new(vec![]);
        assert!(!prefetcher.prefetch_next(&empty_queue));

        // Queue with 3 tracks: t0 (currently playing), t1 (next), t2
        let tracks = vec![
            Track {
                index: 0,
                id: "track_0".to_string(),
                title: "Track 0".to_string(),
            },
            Track {
                index: 1,
                id: "track_1".to_string(),
                title: "Track 1".to_string(),
            },
            Track {
                index: 2,
                id: "track_2".to_string(),
                title: "Track 2".to_string(),
            },
        ];
        let queue = Queue::new(tracks);

        // Current is track_0, peek_next is track_1
        assert_eq!(queue.current_track().unwrap().id, "track_0");
        assert_eq!(queue.peek_next().unwrap().id, "track_1");

        let queued = prefetcher.prefetch_next(&queue);
        assert!(queued, "Should prefetch track_1");
        assert!(prefetcher.is_in_flight("track_1"));

        // Wait for completion
        for _ in 0..50 {
            if !prefetcher.is_in_flight("track_1") {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(cache.is_cached("track_1"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Calling prefetch_next again when track_1 is already cached
        assert!(!prefetcher.prefetch_next(&queue));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_prefetch_failure_cleanup_and_retry() {
        let temp_dir = std::env::temp_dir().join("ytm_test_prefetch_failure");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let cache = AudioCache::new(temp_dir.clone());
        let calls = Arc::new(AtomicUsize::new(0));
        let mock = MockDownloader {
            download_calls: calls.clone(),
            simulate_delay_ms: 5,
            fail: true, // Will fail
        };

        let prefetcher = AudioPrefetcher::with_downloader(cache.clone(), mock);

        let video_id = "failing_video";
        assert!(prefetcher.prefetch(video_id));

        // Wait for failure to process
        for _ in 0..50 {
            if !prefetcher.is_in_flight(video_id) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(!prefetcher.is_in_flight(video_id));
        assert!(!cache.is_cached(video_id));

        // Ensure no leftover temp files exist
        let entries = fs::read_dir(&temp_dir).unwrap();
        let count = entries.count();
        assert_eq!(count, 0, "No orphaned temporary files should remain");

        // Can retry now that it's no longer in-flight
        assert!(prefetcher.prefetch(video_id));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_prefetch_triggers_lru_prune() {
        use std::fs::FileTimes;
        use std::time::SystemTime;

        let temp_dir = std::env::temp_dir().join("ytm_test_prefetch_prune");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // 100 max bytes, 50 target bytes
        let cache = AudioCache::with_limits(temp_dir.clone(), 100, 50);

        // Existing old file with 60 bytes
        let old_file = cache.cache_file_path("old_song");
        fs::write(&old_file, vec![b'o'; 60]).unwrap();
        let f = fs::File::open(&old_file).unwrap();
        let past = SystemTime::now() - Duration::from_secs(3600);
        f.set_times(FileTimes::new().set_accessed(past).set_modified(past)).unwrap();

        // Downloader writes 50 bytes for new track -> total = 110 > 100 max bytes
        struct LargeDownloader;
        impl AudioDownloader for LargeDownloader {
            fn download(&self, _video_id: &str, tmp_path: &Path) -> Result<(), String> {
                fs::write(tmp_path, vec![b'n'; 50]).map_err(|e| e.to_string())
            }
        }

        let prefetcher = AudioPrefetcher::with_downloader(cache.clone(), LargeDownloader);
        assert!(prefetcher.prefetch("new_song"));

        for _ in 0..50 {
            if !prefetcher.is_in_flight("new_song") {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(cache.is_cached("new_song"), "New song should be cached");
        assert!(!old_file.exists(), "Old song should have been pruned by LRU");
        assert!(cache.total_cache_size().unwrap() <= 50, "Cache should be <= target_bytes");

        let _ = fs::remove_dir_all(&temp_dir);
    }
}

