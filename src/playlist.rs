use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

use std::time::{Duration, SystemTime};

pub const DEFAULT_PLAYLIST_URL: &str =
    "https://music.youtube.com/playlist?list=PLxKYvEjnAdk4JEdUsTSYO7RQlzU6WRNY_";

pub const CACHE_TTL_SECS: u64 = 15 * 60; // 15 minutes

#[derive(Debug, Clone)]
pub struct PlaylistManager {
    pub cache_dir: PathBuf,
}

impl PlaylistManager {
    pub fn default_cache_dir() -> PathBuf {
        crate::cache::default_cache_dir()
    }

    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }
}

impl Default for PlaylistManager {
    fn default() -> Self {
        Self::new(Self::default_cache_dir())
    }
}

impl PlaylistManager {

    pub fn url_file_path(&self) -> PathBuf {
        self.cache_dir.join("playlist.url")
    }

    pub fn tracks_file_path(&self) -> PathBuf {
        self.cache_dir.join("tracks.tsv")
    }

    pub fn get_active_url(&self) -> String {
        let path = self.url_file_path();
        if let Ok(content) = fs::read_to_string(path) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        DEFAULT_PLAYLIST_URL.to_string()
    }

    pub fn set_active_url(&self, url: &str) -> io::Result<()> {
        fs::create_dir_all(&self.cache_dir)?;
        fs::write(self.url_file_path(), format!("{}\n", url.trim()))
    }

    pub fn cache_needs_sync(&self) -> bool {
        self.cache_needs_sync_at(SystemTime::now())
    }

    pub fn cache_needs_sync_at(&self, now: SystemTime) -> bool {
        let tracks_path = self.tracks_file_path();
        let metadata = match fs::metadata(&tracks_path) {
            Ok(meta) => meta,
            Err(_) => return true,
        };

        let modified = match metadata.modified() {
            Ok(mtime) => mtime,
            Err(_) => return true,
        };

        match now.duration_since(modified) {
            Ok(age) => age > Duration::from_secs(CACHE_TTL_SECS),
            Err(_) => false,
        }
    }

    pub fn save_tracks(&self, tracks: &[Track]) -> io::Result<()> {
        fs::create_dir_all(&self.cache_dir)?;
        let tsv = format_tracks_tsv(tracks);
        fs::write(self.tracks_file_path(), tsv)
    }

    pub fn load_tracks(&self) -> io::Result<Vec<Track>> {
        let content = fs::read_to_string(self.tracks_file_path())?;
        Ok(parse_tracks_tsv(&content))
    }

    pub fn sync_with_extractor<F>(
        &self,
        url: &str,
        extractor: F,
    ) -> Result<Vec<Track>, Box<dyn std::error::Error>>
    where
        F: FnOnce(&str) -> Result<String, String>,
    {
        let raw_output = extractor(url).map_err(io::Error::other)?;
        let tracks = parse_ytdlp_output(&raw_output);
        self.save_tracks(&tracks)?;
        Ok(tracks)
    }

    pub fn sync_playlist(&self, url: &str) -> Result<Vec<Track>, Box<dyn std::error::Error>> {
        self.sync_with_extractor(url, run_ytdlp_extraction)
    }

    pub fn load_or_sync_with_extractor<F>(
        &self,
        new_url: Option<&str>,
        force_sync: bool,
        extractor: F,
    ) -> Result<Vec<Track>, Box<dyn std::error::Error>>
    where
        F: FnOnce(&str) -> Result<String, String>,
    {
        if let Some(url) = new_url {
            let active = self.get_active_url();
            let url_changed = url != active;
            self.set_active_url(url)?;
            if url_changed || force_sync || self.cache_needs_sync() {
                return self.sync_with_extractor(url, extractor);
            }
        } else if force_sync || self.cache_needs_sync() {
            let active = self.get_active_url();
            return self.sync_with_extractor(&active, extractor);
        }

        match self.load_tracks() {
            Ok(tracks) if !tracks.is_empty() => Ok(tracks),
            _ => {
                let active = self.get_active_url();
                self.sync_with_extractor(&active, extractor)
            }
        }
    }

    pub fn load_or_sync(
        &self,
        new_url: Option<&str>,
        force_sync: bool,
    ) -> Result<Vec<Track>, Box<dyn std::error::Error>> {
        self.load_or_sync_with_extractor(new_url, force_sync, run_ytdlp_extraction)
    }
}

/// Executes yt-dlp to extract track IDs and titles from a playlist URL.
pub fn run_ytdlp_extraction(url: &str) -> Result<String, String> {
    let mut cmd = std::process::Command::new("yt-dlp");
    cmd.args([
        "--no-warnings",
        "--flat-playlist",
        "--print",
        "%(id)s\t%(title)s",
    ]);
    if let Some(cookies) = crate::cache::CookieSource::resolve() {
        cmd.args(cookies.ytdlp_args());
    }
    cmd.arg(url);
    let output = cmd
        .output()
        .map_err(|e| format!("Failed to execute yt-dlp: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("yt-dlp exited with {}: {}", output.status, stderr));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Parses CLI arguments into optional (playlist_url, force_sync).
pub fn parse_cli_args<I: IntoIterator<Item = String>>(args: I) -> (Option<String>, bool) {
    let mut url: Option<String> = None;
    let mut force_sync = false;

    // Skip the first arg (binary name)
    for arg in args.into_iter().skip(1) {
        if arg == "--sync" {
            force_sync = true;
        } else if !arg.starts_with("--") && url.is_none() {
            url = Some(arg);
        }
    }

    (url, force_sync)
}



#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub index: usize,
    pub id: String,
    pub title: String,
}

/// Parses the output from `yt-dlp --no-warnings --flat-playlist --print "%(id)s\t%(title)s"`
/// into a Vec of `Track`s with 0-based indices.
pub fn parse_ytdlp_output(output: &str) -> Vec<Track> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .filter_map(|(idx, line)| {
            let mut parts = line.splitn(2, '\t');
            let id = parts.next()?.trim();
            let title = parts.next().unwrap_or("").trim();
            if id.is_empty() {
                None
            } else {
                Some(Track {
                    index: idx,
                    id: id.to_string(),
                    title: title.to_string(),
                })
            }
        })
        .collect()
}

/// Formats a slice of `Track`s into `<index>\t<id>\t<title>\n` format.
pub fn format_tracks_tsv(tracks: &[Track]) -> String {
    let mut out = String::new();
    for track in tracks {
        out.push_str(&format!("{}\t{}\t{}\n", track.index, track.id, track.title));
    }
    out
}

/// Parses a TSV file formatted as `<index>\t<id>\t<title>`.
/// If index is missing or malformed, it falls back to 0-based line numbering.
pub fn parse_tracks_tsv(content: &str) -> Vec<Track> {
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .filter_map(|(line_idx, line)| {
            let parts: Vec<&str> = line.split('\t').collect();
            match parts.len() {
                len if len >= 3 => {
                    let index = parts[0].trim().parse::<usize>().unwrap_or(line_idx);
                    let id = parts[1].trim();
                    let title = parts[2..].join("\t");
                    if id.is_empty() {
                        None
                    } else {
                        Some(Track {
                            index,
                            id: id.to_string(),
                            title: title.trim().to_string(),
                        })
                    }
                }
                2 => {
                    let id = parts[0].trim();
                    let title = parts[1].trim();
                    if id.is_empty() {
                        None
                    } else {
                        Some(Track {
                            index: line_idx,
                            id: id.to_string(),
                            title: title.to_string(),
                        })
                    }
                }
                _ => None,
            }
        })
        .collect()
}
