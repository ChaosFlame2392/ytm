use std::env;
use std::process;
use ytm::playlist::{parse_cli_args, PlaylistManager};

fn main() {
    let args: Vec<String> = env::args().collect();
    let (url, force_sync) = parse_cli_args(args);

    let manager = PlaylistManager::default();
    let target_url = url.clone().unwrap_or_else(|| manager.get_active_url());

    if force_sync || url.is_some() || manager.cache_needs_sync() {
        println!("Syncing playlist metadata from {}...", target_url);
    }

    match manager.load_or_sync(url.as_deref(), force_sync) {
        Ok(tracks) => {
            println!("Loaded {} tracks from playlist: {}", tracks.len(), target_url);
        }
        Err(e) => {
            eprintln!("Error loading playlist: {}", e);
            process::exit(1);
        }
    }
}
