use std::env;
use std::process;
use ytm::app::App;
use ytm::cache::AudioCache;
use ytm::mpv::MpvController;
use ytm::playlist::{parse_cli_args, PlaylistManager};
use ytm::prefetch::AudioPrefetcher;
use ytm::queue::Queue;

fn main() {
    let args: Vec<String> = env::args().collect();
    let (url, force_sync) = parse_cli_args(args);

    let manager = PlaylistManager::default();
    let target_url = url.clone().unwrap_or_else(|| manager.get_active_url());

    if force_sync || url.is_some() || manager.cache_needs_sync() {
        println!("Syncing playlist metadata from {}...", target_url);
    }

    let tracks = match manager.load_or_sync(url.as_deref(), force_sync) {
        Ok(tracks) => tracks,
        Err(e) => {
            eprintln!("Error loading playlist: {}", e);
            process::exit(1);
        }
    };

    if tracks.is_empty() {
        eprintln!("Playlist contains no tracks: {}", target_url);
        process::exit(1);
    }

    let queue = Queue::new(tracks);
    let cache = AudioCache::default();
    if let Err(e) = cache.ensure_audio_dir() {
        eprintln!("Warning: could not ensure cache directory: {}", e);
    }
    let prefetcher = AudioPrefetcher::new(cache.clone());

    let mpv = match MpvController::start() {
        Ok(mpv) => mpv,
        Err(e) => {
            eprintln!("Error starting mpv: {}", e);
            process::exit(1);
        }
    };

    let mut app = App::new(queue, cache, prefetcher, mpv);
    if let Err(e) = app.run() {
        eprintln!("Player error: {}", e);
        process::exit(1);
    }
}
