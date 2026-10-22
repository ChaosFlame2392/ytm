use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::ops::Deref;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;


#[derive(Debug)]
pub enum MpvError {
    Io(std::io::Error),
    Json(serde_json::Error),
    IpcError(String),
    CommandTimeout,
    Disconnected,
    ProcessExited(Option<i32>),
}

impl std::fmt::Display for MpvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MpvError::Io(e) => write!(f, "IO error: {}", e),
            MpvError::Json(e) => write!(f, "JSON error: {}", e),
            MpvError::IpcError(msg) => write!(f, "MPV IPC error: {}", msg),
            MpvError::CommandTimeout => write!(f, "MPV command timed out"),
            MpvError::Disconnected => write!(f, "MPV IPC disconnected"),
            MpvError::ProcessExited(code) => write!(f, "MPV process exited with code {:?}", code),
        }
    }
}

impl std::error::Error for MpvError {}

impl From<std::io::Error> for MpvError {
    fn from(e: std::io::Error) -> Self {
        MpvError::Io(e)
    }
}

impl From<serde_json::Error> for MpvError {
    fn from(e: serde_json::Error) -> Self {
        MpvError::Json(e)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MpvResponse {
    Success {
        request_id: u64,
        data: Option<Value>,
    },
    PropertyUnavailable {
        request_id: u64,
    },
    Error {
        request_id: u64,
        message: String,
    },
}

#[derive(Serialize)]
struct CommandValuesPayload<'a> {
    command: &'a [Value],
    request_id: u64,
}

#[derive(Deserialize)]
struct RawResponse {
    #[serde(default)]
    request_id: Option<u64>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    data: Option<Value>,
}

pub fn format_command_values(request_id: u64, command: &[Value]) -> String {
    let payload = CommandValuesPayload {
        command,
        request_id,
    };
    let mut s = serde_json::to_string(&payload).expect("Serialization failed");
    s.push('\n');
    s
}

pub fn format_command(request_id: u64, command: &[&str]) -> String {
    let vals: Vec<Value> = command.iter().map(|&s| Value::String(s.to_string())).collect();
    format_command_values(request_id, &vals)
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndFileReason {
    Eof,
    Stop,
    Quit,
    Error,
    Other(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum MpvEvent {
    EndFile { reason: EndFileReason },
    StartFile,
    PlaybackRestart,
    Idle,
    Other(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum IncomingMessage {
    Response(MpvResponse),
    Event(MpvEvent),
}

pub fn parse_response(line: &str) -> Result<MpvResponse, String> {
    let raw: RawResponse = serde_json::from_str(line).map_err(|e| e.to_string())?;
    let request_id = raw.request_id.unwrap_or(0);
    match raw.error.as_deref() {
        Some("success") => Ok(MpvResponse::Success {
            request_id,
            data: raw.data,
        }),
        Some("property unavailable") => Ok(MpvResponse::PropertyUnavailable { request_id }),
        Some(err) => Ok(MpvResponse::Error {
            request_id,
            message: err.to_string(),
        }),
        None => Err("Missing error field in response".to_string()),
    }
}

pub fn parse_incoming(line: &str) -> Result<IncomingMessage, String> {
    let v: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
    if let Some(event_str) = v.get("event").and_then(|e| e.as_str()) {
        let event = match event_str {
            "end-file" => {
                let reason = match v.get("reason").and_then(|r| r.as_str()) {
                    Some("eof") => EndFileReason::Eof,
                    Some("stop") => EndFileReason::Stop,
                    Some("quit") => EndFileReason::Quit,
                    Some("error") => EndFileReason::Error,
                    Some(other) => EndFileReason::Other(other.to_string()),
                    None => EndFileReason::Other(String::new()),
                };
                MpvEvent::EndFile { reason }
            }
            "start-file" => MpvEvent::StartFile,
            "playback-restart" => MpvEvent::PlaybackRestart,
            "idle" => MpvEvent::Idle,
            other => MpvEvent::Other(other.to_string()),
        };
        Ok(IncomingMessage::Event(event))
    } else {
        let resp = parse_response(line)?;
        Ok(IncomingMessage::Response(resp))
    }
}

pub struct MpvIpcClient {
    writer: Arc<Mutex<UnixStream>>,
    pending: Arc<Mutex<HashMap<u64, Sender<MpvResponse>>>>,
    event_rx: Mutex<Receiver<MpvEvent>>,
    next_req_id: AtomicU64,
    _reader_handle: std::thread::JoinHandle<()>,
}

impl MpvIpcClient {
    pub fn new(stream: UnixStream) -> Self {
        let read_stream = stream.try_clone().expect("Failed to clone UnixStream");
        let writer = Arc::new(Mutex::new(stream));
        let pending: Arc<Mutex<HashMap<u64, Sender<MpvResponse>>>> = Arc::new(Mutex::new(HashMap::new()));
        let (event_tx, event_rx) = mpsc::channel();

        let pending_clone = Arc::clone(&pending);
        let reader_handle = std::thread::spawn(move || {
            let mut reader = BufReader::new(read_stream);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if let Ok(incoming) = parse_incoming(trimmed) {
                            match incoming {
                                IncomingMessage::Event(ev) => {
                                    let _ = event_tx.send(ev);
                                }
                                IncomingMessage::Response(resp) => {
                                    let req_id = match &resp {
                                        MpvResponse::Success { request_id, .. } => *request_id,
                                        MpvResponse::PropertyUnavailable { request_id } => *request_id,
                                        MpvResponse::Error { request_id, .. } => *request_id,
                                    };
                                    let mut map = pending_clone.lock().unwrap();
                                    if let Some(tx) = map.remove(&req_id) {
                                        let _ = tx.send(resp);
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            writer,
            pending,
            event_rx: Mutex::new(event_rx),
            next_req_id: AtomicU64::new(1),
            _reader_handle: reader_handle,
        }
    }

    pub fn send_command_values(&self, command: &[Value]) -> Result<MpvResponse, MpvError> {
        let req_id = self.next_req_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        {
            let mut map = self.pending.lock().unwrap();
            map.insert(req_id, tx);
        }

        let payload = format_command_values(req_id, command);
        {
            let mut w = self.writer.lock().unwrap();
            w.write_all(payload.as_bytes())?;
            w.flush()?;
        }

        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(resp) => Ok(resp),
            Err(RecvTimeoutError::Timeout) => {
                let mut map = self.pending.lock().unwrap();
                map.remove(&req_id);
                Err(MpvError::CommandTimeout)
            }
            Err(RecvTimeoutError::Disconnected) => Err(MpvError::Disconnected),
        }
    }

    pub fn send_command(&self, command: &[&str]) -> Result<MpvResponse, MpvError> {
        let vals: Vec<Value> = command.iter().map(|&s| Value::String(s.to_string())).collect();
        self.send_command_values(&vals)
    }

    pub fn loadfile(&self, path_or_url: &str) -> Result<(), MpvError> {
        let resp = self.send_command(&["loadfile", path_or_url, "replace"])?;
        match resp {
            MpvResponse::Success { .. } => Ok(()),
            MpvResponse::Error { message, .. } => Err(MpvError::IpcError(message)),
            MpvResponse::PropertyUnavailable { .. } => Ok(()),
        }
    }

    pub fn toggle_pause(&self) -> Result<(), MpvError> {
        let resp = self.send_command(&["cycle", "pause"])?;
        match resp {
            MpvResponse::Success { .. } => Ok(()),
            MpvResponse::Error { message, .. } => Err(MpvError::IpcError(message)),
            MpvResponse::PropertyUnavailable { .. } => Ok(()),
        }
    }

    pub fn pause(&self) -> Result<(), MpvError> {
        self.send_command_values(&[
            Value::String("set_property".to_string()),
            Value::String("pause".to_string()),
            Value::Bool(true),
        ])?;
        Ok(())
    }

    pub fn resume(&self) -> Result<(), MpvError> {
        self.send_command_values(&[
            Value::String("set_property".to_string()),
            Value::String("pause".to_string()),
            Value::Bool(false),
        ])?;
        Ok(())
    }

    pub fn is_paused(&self) -> Result<bool, MpvError> {
        let resp = self.send_command(&["get_property", "pause"])?;
        match resp {
            MpvResponse::Success { data, .. } => Ok(data.and_then(|v| v.as_bool()).unwrap_or(false)),
            _ => Ok(false),
        }
    }

    pub fn get_position(&self) -> Result<Option<f64>, MpvError> {
        let resp = self.send_command(&["get_property", "time-pos"])?;
        match resp {
            MpvResponse::Success { data, .. } => Ok(data.and_then(|v| v.as_f64())),
            MpvResponse::PropertyUnavailable { .. } => Ok(None),
            MpvResponse::Error { message, .. } => Err(MpvError::IpcError(message)),
        }
    }

    pub fn get_duration(&self) -> Result<Option<f64>, MpvError> {
        let resp = self.send_command(&["get_property", "duration"])?;
        match resp {
            MpvResponse::Success { data, .. } => Ok(data.and_then(|v| v.as_f64())),
            MpvResponse::PropertyUnavailable { .. } => Ok(None),
            MpvResponse::Error { message, .. } => Err(MpvError::IpcError(message)),
        }
    }

    /// Performs relative in-track seeking by `offset_seconds`.
    pub fn seek(&self, offset_seconds: f64) -> Result<(), MpvError> {
        let offset_str = format!("{}", offset_seconds);
        let resp = self.send_command(&["seek", &offset_str, "relative"])?;
        match resp {
            MpvResponse::Success { .. } => Ok(()),
            MpvResponse::Error { message, .. } => Err(MpvError::IpcError(message)),
            MpvResponse::PropertyUnavailable { .. } => Ok(()),
        }
    }

    pub fn poll_event(&self) -> Option<MpvEvent> {
        self.event_rx.lock().unwrap().try_recv().ok()
    }

    pub fn recv_event(&self) -> Result<MpvEvent, mpsc::RecvError> {
        self.event_rx.lock().unwrap().recv()
    }

    pub fn recv_event_timeout(&self, timeout: Duration) -> Result<MpvEvent, mpsc::RecvTimeoutError> {
        self.event_rx.lock().unwrap().recv_timeout(timeout)
    }
}

pub const DEFAULT_SOCKET_PATH: &str = "/tmp/ytm-mpv.sock";

pub struct MpvController {
    client: MpvIpcClient,
    child: Option<Child>,
    socket_path: Option<PathBuf>,
}

impl MpvController {
    /// Spawns a headless mpv process using the default socket path `/tmp/ytm-mpv.sock`.
    pub fn start() -> Result<Self, MpvError> {
        Self::start_with_socket(DEFAULT_SOCKET_PATH)
    }

    /// Spawns a headless mpv process using the specified socket path.
    pub fn start_with_socket<P: AsRef<Path>>(socket_path: P) -> Result<Self, MpvError> {
        let path = socket_path.as_ref().to_path_buf();
        // Remove stale socket if it exists
        let _ = std::fs::remove_file(&path);

        let mut cmd = Command::new("mpv");
        cmd.arg("--no-video")
            .arg("--really-quiet")
            .arg("--idle")
            .arg(format!("--input-ipc-server={}", path.display()));
        if cfg!(test) {
            cmd.arg("--ao=null");
        }
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        // Wait for socket to become available
        let mut stream = None;
        for _ in 0..100 {
            if let Some(status) = child.try_wait()? {
                return Err(MpvError::ProcessExited(status.code()));
            }
            if let Ok(s) = UnixStream::connect(&path) {
                stream = Some(s);
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }

        let stream = match stream {
            Some(s) => s,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&path);
                return Err(MpvError::CommandTimeout);
            }
        };

        let client = MpvIpcClient::new(stream);
        Ok(Self {
            client,
            child: Some(child),
            socket_path: Some(path),
        })
    }

    /// Wraps an existing UnixStream without managing a child process.
    pub fn from_stream(stream: UnixStream) -> Self {
        Self {
            client: MpvIpcClient::new(stream),
            child: None,
            socket_path: None,
        }
    }

    /// Access the underlying IPC client.
    pub fn client(&self) -> &MpvIpcClient {
        &self.client
    }

    /// Performs relative in-track seeking by `offset_seconds`.
    pub fn seek(&self, offset_seconds: f64) -> Result<(), MpvError> {
        self.client.seek(offset_seconds)
    }


    /// Gracefully terminate mpv and wait for process exit.
    pub fn quit(&mut self) -> Result<(), MpvError> {
        let _ = self.client.send_command(&["quit"]);

        if let Some(mut child) = self.child.take() {
            // Wait up to 1 second for graceful exit
            let mut exited = false;
            for _ in 0..20 {
                if child.try_wait()?.is_some() {
                    exited = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if !exited {
                let _ = child.kill();
            }
            let _ = child.wait();
        }

        if let Some(ref path) = self.socket_path.take() {
            let _ = std::fs::remove_file(path);
        }

        Ok(())
    }
}

impl Deref for MpvController {
    type Target = MpvIpcClient;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl Drop for MpvController {
    fn drop(&mut self) {
        let _ = self.quit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_command_and_parse_response() {
        let cmd = format_command(1, &["loadfile", "test.opus", "replace"]);
        assert_eq!(
            cmd.trim(),
            r#"{"command":["loadfile","test.opus","replace"],"request_id":1}"#
        );

        let resp_success: MpvResponse = parse_response(r#"{"request_id":1,"error":"success","data":42.5}"#).unwrap();
        match resp_success {
            MpvResponse::Success { request_id, data } => {
                assert_eq!(request_id, 1);
                assert_eq!(data.and_then(|v| v.as_f64()), Some(42.5));
            }
            _ => panic!("Expected Success"),
        }

        let resp_unavailable: MpvResponse = parse_response(r#"{"request_id":1,"error":"property unavailable"}"#).unwrap();
        match resp_unavailable {
            MpvResponse::PropertyUnavailable { request_id } => {
                assert_eq!(request_id, 1);
            }
            _ => panic!("Expected PropertyUnavailable"),
        }

        let resp_err: MpvResponse = parse_response(r#"{"request_id":1,"error":"invalid parameter"}"#).unwrap();
        match resp_err {
            MpvResponse::Error { request_id, message } => {
                assert_eq!(request_id, 1);
                assert_eq!(message, "invalid parameter");
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_parse_events_and_distinguish_from_responses() {
        let msg_eof = parse_incoming(r#"{"event":"end-file","reason":"eof"}"#).unwrap();
        assert_eq!(
            msg_eof,
            IncomingMessage::Event(MpvEvent::EndFile {
                reason: EndFileReason::Eof
            })
        );

        let msg_stop = parse_incoming(r#"{"event":"end-file","reason":"stop"}"#).unwrap();
        assert_eq!(
            msg_stop,
            IncomingMessage::Event(MpvEvent::EndFile {
                reason: EndFileReason::Stop
            })
        );

        let msg_idle = parse_incoming(r#"{"event":"idle"}"#).unwrap();
        assert_eq!(msg_idle, IncomingMessage::Event(MpvEvent::Idle));

        let msg_restart = parse_incoming(r#"{"event":"playback-restart"}"#).unwrap();
        assert_eq!(msg_restart, IncomingMessage::Event(MpvEvent::PlaybackRestart));

        let msg_resp = parse_incoming(r#"{"request_id":42,"error":"success","data":"ok"}"#).unwrap();
        match msg_resp {
            IncomingMessage::Response(MpvResponse::Success { request_id, data }) => {
                assert_eq!(request_id, 42);
                assert_eq!(data.and_then(|v| v.as_str().map(String::from)), Some("ok".to_string()));
            }
            _ => panic!("Expected Response::Success"),
        }
    }

    #[test]
    fn test_mpv_ipc_client_commands_and_events() {
        let (client_stream, mut server_stream) = std::os::unix::net::UnixStream::pair().unwrap();
        let client = MpvIpcClient::new(client_stream);

        let server_thread = std::thread::spawn(move || {
            use std::io::{BufRead, BufReader, Write};
            let mut reader = BufReader::new(server_stream.try_clone().unwrap());

            // 1. Expect loadfile
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let parsed: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(parsed["command"][0], "loadfile");
            assert_eq!(parsed["command"][1], "song.opus");
            let req_id = parsed["request_id"].as_u64().unwrap();
            server_stream
                .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
                .unwrap();
            server_stream.write_all(b"\n").unwrap();

            // 2. Send an event asynchronously
            server_stream
                .write_all(b"{\"event\":\"end-file\",\"reason\":\"eof\"}\n")
                .unwrap();

            // 3. Expect get_property time-pos
            line.clear();
            reader.read_line(&mut line).unwrap();
            let parsed: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(parsed["command"][0], "get_property");
            assert_eq!(parsed["command"][1], "time-pos");
            let req_id = parsed["request_id"].as_u64().unwrap();
            server_stream
                .write_all(format!(r#"{{"request_id":{},"error":"success","data":15.75}}"#, req_id).as_bytes())
                .unwrap();
            server_stream.write_all(b"\n").unwrap();

            // 4. Expect get_property duration when unavailable
            line.clear();
            reader.read_line(&mut line).unwrap();
            let parsed: Value = serde_json::from_str(&line).unwrap();
            let req_id = parsed["request_id"].as_u64().unwrap();
            server_stream
                .write_all(format!(r#"{{"request_id":{},"error":"property unavailable"}}"#, req_id).as_bytes())
                .unwrap();
            server_stream.write_all(b"\n").unwrap();

            // 5. Expect toggle_pause
            line.clear();
            reader.read_line(&mut line).unwrap();
            let parsed: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(parsed["command"][0], "cycle");
            assert_eq!(parsed["command"][1], "pause");
            let req_id = parsed["request_id"].as_u64().unwrap();
            server_stream
                .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
                .unwrap();
            server_stream.write_all(b"\n").unwrap();
        });

        client.loadfile("song.opus").unwrap();

        // Check event received
        let ev = client.recv_event_timeout(std::time::Duration::from_millis(500)).unwrap();
        assert_eq!(ev, MpvEvent::EndFile { reason: EndFileReason::Eof });

        let pos = client.get_position().unwrap();
        assert_eq!(pos, Some(15.75));

        let dur = client.get_duration().unwrap();
        assert_eq!(dur, None);

        client.toggle_pause().unwrap();

        server_thread.join().unwrap();
    }

    #[test]
    fn test_mpv_ipc_client_seek_command() {
        let (client_stream, mut server_stream) = std::os::unix::net::UnixStream::pair().unwrap();
        let client = MpvIpcClient::new(client_stream);

        let server_thread = std::thread::spawn(move || {
            use std::io::{BufRead, BufReader, Write};
            let mut reader = BufReader::new(server_stream.try_clone().unwrap());

            // 1. seek +5 relative
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let parsed: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(parsed["command"][0], "seek");
            assert_eq!(parsed["command"][1], "5");
            assert_eq!(parsed["command"][2], "relative");
            let req_id = parsed["request_id"].as_u64().unwrap();
            server_stream
                .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
                .unwrap();
            server_stream.write_all(b"\n").unwrap();

            // 2. seek -5 relative
            line.clear();
            reader.read_line(&mut line).unwrap();
            let parsed: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(parsed["command"][0], "seek");
            assert_eq!(parsed["command"][1], "-5");
            assert_eq!(parsed["command"][2], "relative");
            let req_id = parsed["request_id"].as_u64().unwrap();
            server_stream
                .write_all(format!(r#"{{"request_id":{},"error":"success","data":null}}"#, req_id).as_bytes())
                .unwrap();
            server_stream.write_all(b"\n").unwrap();
        });

        client.seek(5.0).unwrap();
        client.seek(-5.0).unwrap();

        server_thread.join().unwrap();
    }


    #[test]
    fn test_mpv_controller_real_process_lifecycle() {
        let socket = format!("/tmp/ytm-test-{}.sock", std::process::id());
        let mut controller = MpvController::start_with_socket(&socket).expect("Failed to start mpv");

        // Check position when idle (should be None)
        let pos = controller.get_position().expect("Failed to get position");
        assert_eq!(pos, None);

        // Check pause state
        let paused = controller.is_paused().expect("Failed to get pause");
        assert!(!paused);

        // Toggle pause
        controller.toggle_pause().expect("Failed to toggle pause");
        let paused = controller.is_paused().expect("Failed to get pause");
        assert!(paused);

        // Quit cleanly
        controller.quit().expect("Failed to quit mpv");

        // Verify socket was removed
        assert!(!std::path::Path::new(&socket).exists());
    }

    #[test]
    fn test_mpv_controller_drop_cleanup() {
        let socket = format!("/tmp/ytm-test-drop-{}.sock", std::process::id());
        {
            let _controller = MpvController::start_with_socket(&socket).expect("Failed to start mpv");
            assert!(std::path::Path::new(&socket).exists());
        }
        // Give mpv a brief moment to exit and cleanup on drop
        std::thread::sleep(Duration::from_millis(100));
        assert!(!std::path::Path::new(&socket).exists());
    }

    fn create_test_wav(path: &Path, num_samples: usize) {
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
    fn test_mpv_real_playback_eof_event() {
        let wav_path = std::env::temp_dir().join(format!("ytm-test-{}.wav", std::process::id()));
        create_test_wav(&wav_path, 2205); // 0.05 seconds of silence

        let socket = format!("/tmp/ytm-test-eof-{}.sock", std::process::id());
        let mut controller = MpvController::start_with_socket(&socket).expect("Failed to start mpv");

        controller.loadfile(wav_path.to_str().unwrap()).expect("Failed to loadfile");

        let mut received_eof = false;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            if let Ok(MpvEvent::EndFile { reason: EndFileReason::Eof }) =
                controller.recv_event_timeout(Duration::from_millis(100))
            {
                received_eof = true;
                break;
            }
        }

        assert!(received_eof, "Did not receive EndFile Eof event");

        let _ = controller.quit();
        let _ = std::fs::remove_file(&wav_path);
    }
}



