use std::path::Path;
use std::time::Duration;
use ytm::mpv::MpvController;

fn create_test_wav(path: &Path, num_samples: usize) {
    use std::io::Write;
    let mut file = std::fs::File::create(path).unwrap();
    let data_len = (num_samples * 2) as u32;
    let riff_len = data_len + 36;
    file.write_all(b"RIFF").unwrap();
    file.write_all(&riff_len.to_le_bytes()).unwrap();
    file.write_all(b"WAVEfmt ").unwrap();
    file.write_all(&16u32.to_le_bytes()).unwrap();
    file.write_all(&1u16.to_le_bytes()).unwrap();
    file.write_all(&1u16.to_le_bytes()).unwrap();
    file.write_all(&44100u32.to_le_bytes()).unwrap();
    file.write_all(&(44100u32 * 2).to_le_bytes()).unwrap();
    file.write_all(&2u16.to_le_bytes()).unwrap();
    file.write_all(&16u16.to_le_bytes()).unwrap();
    file.write_all(b"data").unwrap();
    file.write_all(&data_len.to_le_bytes()).unwrap();
    file.write_all(&vec![0u8; data_len as usize]).unwrap();
}

#[test]
fn test_mpv_memory_flags_and_repeated_track_switching() {
    let wav1 = std::env::temp_dir().join(format!("ytm-mem-test1-{}.wav", std::process::id()));
    let wav2 = std::env::temp_dir().join(format!("ytm-mem-test2-{}.wav", std::process::id()));
    create_test_wav(&wav1, 2205);
    create_test_wav(&wav2, 2205);

    let socket = format!("/tmp/ytm-mem-test-{}.sock", std::process::id());
    let mut controller = MpvController::start_with_socket(&socket).expect("Failed to start mpv");

    // Rapid track switching simulation (15 switches)
    for i in 0..15 {
        let path = if i % 2 == 0 { &wav1 } else { &wav2 };
        controller.loadfile(path.to_str().unwrap()).expect("loadfile failed");
        std::thread::sleep(Duration::from_millis(20));
    }

    // Verify player is responsive and can toggle pause cleanly
    controller.toggle_pause().expect("Failed to toggle pause");
    let is_paused = controller.is_paused().expect("Failed to query pause");
    assert!(is_paused);

    // Clean exit
    controller.quit().expect("Failed to quit mpv");

    let _ = std::fs::remove_file(&wav1);
    let _ = std::fs::remove_file(&wav2);
}
