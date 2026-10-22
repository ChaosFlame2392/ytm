use std::fs;
use std::io;
use std::path::PathBuf;

pub const DEFAULT_MAX_CACHE_BYTES: u64 = 500 * 1024 * 1024; // 500 MB
pub const DEFAULT_TARGET_CACHE_BYTES: u64 = 400 * 1024 * 1024; // 400 MB

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioSource {
    Local(PathBuf),
    Remote(String),
}

impl AudioSource {
    pub fn as_str(&self) -> &str {
        match self {
            AudioSource::Local(path) => path.to_str().unwrap_or(""),
            AudioSource::Remote(url) => url.as_str(),
        }
    }

    pub fn into_string(self) -> String {
        match self {
            AudioSource::Local(path) => path.to_string_lossy().into_owned(),
            AudioSource::Remote(url) => url,
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self, AudioSource::Local(_))
    }
}

#[derive(Debug, Clone)]
pub struct AudioCache {
    pub audio_dir: PathBuf,
    pub max_bytes: u64,
    pub target_bytes: u64,
}

impl Default for AudioCache {
    fn default() -> Self {
        Self::new(Self::default_audio_dir())
    }
}

impl AudioCache {
    pub fn default_audio_dir() -> PathBuf {
        if let Ok(dir) = std::env::var("YTM_CACHE_DIR") {
            PathBuf::from(dir).join("audio")
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join("Library/Caches/ytm/audio")
        } else {
            PathBuf::from("./cache/audio")
        }
    }

    pub fn new(audio_dir: PathBuf) -> Self {
        Self::with_limits(audio_dir, DEFAULT_MAX_CACHE_BYTES, DEFAULT_TARGET_CACHE_BYTES)
    }

    pub fn with_limits(audio_dir: PathBuf, max_bytes: u64, target_bytes: u64) -> Self {
        Self {
            audio_dir,
            max_bytes,
            target_bytes,
        }
    }

    pub fn cache_file_path(&self, video_id: &str) -> PathBuf {
        self.audio_dir.join(format!("{}.opus", video_id))
    }


    pub fn is_cached(&self, video_id: &str) -> bool {
        self.cache_file_path(video_id).is_file()
    }

    pub fn resolve_source(&self, video_id: &str) -> AudioSource {
        let path = self.cache_file_path(video_id);
        if path.is_file() {
            AudioSource::Local(path)
        } else {
            AudioSource::Remote(format!("https://www.youtube.com/watch?v={}", video_id))
        }
    }

    pub fn get_audio_source(&self, video_id: &str) -> String {
        self.resolve_source(video_id).as_str().to_string()
    }

    pub fn ensure_audio_dir(&self) -> io::Result<()> {
        fs::create_dir_all(&self.audio_dir)
    }

    pub fn total_cache_size(&self) -> io::Result<u64> {
        if !self.audio_dir.exists() {
            return Ok(0);
        }

        let mut total: u64 = 0;
        for entry in fs::read_dir(&self.audio_dir)? {
            let entry = entry?;
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    total += meta.len();
                }
            }
        }
        Ok(total)
    }

    pub fn prune_lru(&self) -> io::Result<PruneReport> {
        if !self.audio_dir.exists() {
            return Ok(PruneReport::default());
        }

        let mut total_size: u64 = 0;
        let mut candidates = Vec::new();

        for entry in fs::read_dir(&self.audio_dir)? {
            let entry = entry?;
            let path = entry.path();
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    let size = meta.len();
                    total_size += size;

                    let is_opus = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|ext| ext.eq_ignore_ascii_case("opus"))
                        .unwrap_or(false);

                    if is_opus {
                        let timestamp = meta
                            .accessed()
                            .or_else(|_| meta.modified())
                            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                        candidates.push((path, size, timestamp));
                    }
                }
            }
        }

        let mut current_size = total_size;
        let mut bytes_deleted = 0;
        let mut files_deleted = 0;

        if current_size > self.max_bytes {
            // Sort ascending by timestamp (oldest first), breaking ties by path
            candidates.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0)));

            for (path, size, _) in candidates {
                if current_size <= self.target_bytes {
                    break;
                }
                if fs::remove_file(&path).is_ok() {
                    current_size = current_size.saturating_sub(size);
                    bytes_deleted += size;
                    files_deleted += 1;
                }
            }
        }

        Ok(PruneReport {
            initial_bytes: total_size,
            target_bytes: self.target_bytes,
            bytes_deleted,
            files_deleted,
            remaining_bytes: current_size,
        })
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PruneReport {
    pub initial_bytes: u64,
    pub target_bytes: u64,
    pub bytes_deleted: u64,
    pub files_deleted: usize,
    pub remaining_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::FileTimes;
    use std::time::{Duration, SystemTime};

    #[test]
    fn test_cache_file_path_and_source_resolution() {
        let temp_dir = std::env::temp_dir().join("ytm_test_cache_cycle1");
        let _ = fs::remove_dir_all(&temp_dir);
        let cache = AudioCache::new(temp_dir.clone());

        let video_id = "abc123xyz";
        let expected_path = temp_dir.join("abc123xyz.opus");
        assert_eq!(cache.cache_file_path(video_id), expected_path);

        // Before file exists: is_cached is false, source is remote URL
        assert!(!cache.is_cached(video_id));
        assert_eq!(
            cache.get_audio_source(video_id),
            "https://www.youtube.com/watch?v=abc123xyz"
        );
        assert_eq!(
            cache.resolve_source(video_id),
            AudioSource::Remote("https://www.youtube.com/watch?v=abc123xyz".to_string())
        );

        // Create the file: is_cached becomes true, source is local path
        fs::create_dir_all(&temp_dir).unwrap();
        fs::write(&expected_path, b"test opus content").unwrap();

        assert!(cache.is_cached(video_id));
        assert_eq!(
            cache.get_audio_source(video_id),
            expected_path.to_string_lossy().to_string()
        );
        assert_eq!(
            cache.resolve_source(video_id),
            AudioSource::Local(expected_path.clone())
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_lru_pruning_order_and_size_calculation() {
        let temp_dir = std::env::temp_dir().join("ytm_test_cache_cycle2_lru");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // 1000 max bytes, 600 target bytes
        let cache = AudioCache::with_limits(temp_dir.clone(), 1000, 600);

        let base_time = SystemTime::now() - Duration::from_secs(3600);

        let create_dummy = |name: &str, size: usize, age_offset_secs: u64| -> PathBuf {
            let path = temp_dir.join(name);
            fs::write(&path, vec![b'x'; size]).unwrap();
            let file = fs::File::open(&path).unwrap();
            let t = base_time + Duration::from_secs(age_offset_secs);
            let times = FileTimes::new().set_accessed(t).set_modified(t);
            file.set_times(times).unwrap();
            path
        };

        let f1 = create_dummy("f1.opus", 300, 10); // oldest
        let f2 = create_dummy("f2.opus", 400, 20); // second oldest
        let f3 = create_dummy("f3.opus", 250, 30); // third oldest
        let f4 = create_dummy("f4.opus", 200, 40); // newest

        // Total size should be 300 + 400 + 250 + 200 = 1150
        assert_eq!(cache.total_cache_size().unwrap(), 1150);

        // Pruning should delete f1 (300 bytes -> 850), then f2 (400 bytes -> 450 <= 600 target)
        let report = cache.prune_lru().unwrap();
        assert_eq!(report.files_deleted, 2);
        assert_eq!(report.bytes_deleted, 700);
        assert_eq!(report.remaining_bytes, 450);

        assert!(!f1.exists(), "f1 (oldest) should have been pruned");
        assert!(!f2.exists(), "f2 (second oldest) should have been pruned");
        assert!(f3.exists(), "f3 should remain");
        assert!(f4.exists(), "f4 should remain");
        assert_eq!(cache.total_cache_size().unwrap(), 450);

        // Calling prune_lru when below max_bytes (450 <= 1000) does nothing
        let report2 = cache.prune_lru().unwrap();
        assert_eq!(report2.files_deleted, 0);
        assert_eq!(report2.bytes_deleted, 0);
        assert_eq!(report2.remaining_bytes, 450);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_non_opus_files_preserved_during_prune() {
        let temp_dir = std::env::temp_dir().join("ytm_test_cache_preserve_non_opus");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let cache = AudioCache::with_limits(temp_dir.clone(), 500, 300);

        // Create a non-opus file (e.g. metadata or readme)
        let other_file = temp_dir.join("metadata.json");
        fs::write(&other_file, vec![b'm'; 200]).unwrap();

        // Create opus files that exceed max limit: 200 + 400 = 600 > 500
        let opus_file = temp_dir.join("song.opus");
        fs::write(&opus_file, vec![b'o'; 400]).unwrap();

        assert_eq!(cache.total_cache_size().unwrap(), 600);

        let report = cache.prune_lru().unwrap();
        assert_eq!(report.files_deleted, 1);
        assert!(!opus_file.exists(), "Opus file should be pruned");
        assert!(other_file.exists(), "Non-opus file must be preserved");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_non_existent_directory_behavior() {
        let temp_dir = std::env::temp_dir().join("ytm_test_cache_non_existent");
        let _ = fs::remove_dir_all(&temp_dir);

        let cache = AudioCache::new(temp_dir.clone());
        assert_eq!(cache.total_cache_size().unwrap(), 0);

        let report = cache.prune_lru().unwrap();
        assert_eq!(report.files_deleted, 0);
        assert_eq!(report.bytes_deleted, 0);

        // ensure_audio_dir creates directory
        assert!(!temp_dir.exists());
        cache.ensure_audio_dir().unwrap();
        assert!(temp_dir.exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_audio_source_methods() {
        let local_path = PathBuf::from("/test/audio.opus");
        let source_local = AudioSource::Local(local_path.clone());
        assert!(source_local.is_local());
        assert_eq!(source_local.as_str(), "/test/audio.opus");
        assert_eq!(source_local.into_string(), "/test/audio.opus");

        let remote_url = "https://www.youtube.com/watch?v=123".to_string();
        let source_remote = AudioSource::Remote(remote_url.clone());
        assert!(!source_remote.is_local());
        assert_eq!(source_remote.as_str(), &remote_url);
        assert_eq!(source_remote.into_string(), remote_url);
    }
}

