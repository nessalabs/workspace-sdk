//! Pinned SPACE wire simulator. Session attachment executes the real installed
//! supervisor; this fixture supplies transport, never native protocol answers.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::{MetadataExt, PermissionsExt},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub const SANDBOX: &str = "sbx_00000000-0000-0000-0000-000000000001";
pub const MANAGEMENT: &str = "fixture-management-private";
pub const ATTACH: &str = "fixture-attach-private";
pub const USER: &str = "fixture-user";

#[derive(Clone, Copy, Debug, Default)]
pub enum Fault {
    #[default]
    None,
    Eof,
    Gap,
    InvalidBase64,
    WrongProcess,
    MissingStarted,
    ReorderedStarted,
    ClosedOnly,
    WrongClosedReason,
    DuplicateClosed,
    OutputAfterExit,
    Resync,
    Oversized,
    AttachOrigin,
    AttachPath,
    Redirect,
    WrongSandbox,
    WrongSession,
    ShortUpload,
    UploadAckLoss,
    LateUpload,
    LaunchAckLoss,
    Delay,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}
struct Shared {
    requests: Mutex<Vec<Request>>,
    commands: Mutex<BTreeMap<String, Value>>,
    fault: Mutex<Fault>,
    violations: Mutex<Vec<String>>,
    stop: AtomicBool,
    data_url: String,
}
pub struct Fixture {
    pub api_url: String,
    pub data_url: String,
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
}
impl Fixture {
    pub fn new() -> Self {
        let api = TcpListener::bind("127.0.0.1:0").unwrap();
        let data = TcpListener::bind("127.0.0.1:0").unwrap();
        let api_url = format!("http://{}", api.local_addr().unwrap());
        let data_url = format!("http://{}", data.local_addr().unwrap());
        let shared = Arc::new(Shared {
            requests: Mutex::new(Vec::new()),
            commands: Mutex::new(BTreeMap::new()),
            fault: Mutex::new(Fault::None),
            violations: Mutex::new(Vec::new()),
            stop: AtomicBool::new(false),
            data_url: data_url.clone(),
        });
        let workers =
            [(api, false), (data, true)]
                .into_iter()
                .map(|(listener, data)| {
                    listener.set_nonblocking(true).unwrap();
                    let state = shared.clone();
                    thread::spawn(move || {
                        let mut handlers: Vec<JoinHandle<()>> = Vec::new();
                        while !state.stop.load(Ordering::Acquire) {
                            match listener.accept() {
                                Ok((stream, _)) => {
                                    let mut index = 0;
                                    while index < handlers.len() {
                                        if handlers[index].is_finished() {
                                            if handlers.swap_remove(index).join().is_err() {
                                                state.violations.lock().unwrap().push(
                                                    "fixture connection handler panicked".into(),
                                                );
                                            }
                                        } else {
                                            index += 1;
                                        }
                                    }
                                    if handlers.len() >= 32 {
                                        state.violations.lock().unwrap().push(
                                            "fixture active connection budget exceeded".into(),
                                        );
                                        drop(stream);
                                        continue;
                                    }
                                    if let Err(error) = stream.set_nonblocking(false) {
                                        state.violations.lock().unwrap().push(format!(
                                            "fixture blocking socket setup failed: {error}"
                                        ));
                                        drop(stream);
                                        continue;
                                    }
                                    let handler_state = state.clone();
                                    match thread::Builder::new()
                                        .spawn(move || handle(stream, &handler_state, data))
                                    {
                                        Ok(handler) => handlers.push(handler),
                                        Err(error) => {
                                            state.violations.lock().unwrap().push(format!(
                                                "fixture handler spawn failed: {error}"
                                            ))
                                        }
                                    }
                                }
                                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                    thread::sleep(Duration::from_millis(2))
                                }
                                Err(error) => {
                                    state
                                        .violations
                                        .lock()
                                        .unwrap()
                                        .push(format!("fixture accept failed: {error}"));
                                    break;
                                }
                            }
                        }
                        for handler in handlers {
                            if handler.join().is_err() {
                                state
                                    .violations
                                    .lock()
                                    .unwrap()
                                    .push("fixture connection handler panicked".into());
                            }
                        }
                    })
                })
                .collect();
        Self {
            api_url,
            data_url,
            shared,
            workers,
        }
    }
    pub fn fault(&self, fault: Fault) {
        *self.shared.fault.lock().unwrap() = fault;
    }
    pub fn requests(&self) -> Vec<Request> {
        self.shared.requests.lock().unwrap().clone()
    }
    pub fn assert_valid(&self) {
        assert_eq!(
            *self.shared.violations.lock().unwrap(),
            Vec::<String>::new()
        );
        assert!(self
            .requests()
            .iter()
            .all(|request| request.method != "DELETE"));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        for worker in self.workers.drain(..) {
            if worker.join().is_err() {
                self.shared
                    .violations
                    .lock()
                    .unwrap()
                    .push("fixture listener panicked".into());
            }
        }
        let errors = self.shared.violations.lock().unwrap();
        if !errors.is_empty() {
            if thread::panicking() {
                eprintln!("fixture failure during unwind: {errors:?}");
            } else {
                panic!("fixture transport failure: {errors:?}");
            }
        }
    }
}
fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<Request>> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut bytes = Vec::new();
    let header_end = loop {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "fixture request deadline")
            })?;
        stream.set_read_timeout(Some(remaining))?;
        let mut byte = [0];
        match stream.read_exact(&mut byte) {
            Ok(()) => {}
            Err(error)
                if bytes.is_empty()
                    && matches!(
                        error.kind(),
                        std::io::ErrorKind::UnexpectedEof
                            | std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                    ) =>
            {
                return Ok(None)
            }
            Err(error) => return Err(error),
        }
        bytes.push(byte[0]);
        assert!(bytes.len() <= 64 * 1024);
        if bytes.ends_with(b"\r\n\r\n") {
            break bytes.len();
        }
    };
    let header = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
    let mut lines = header.split("\r\n");
    let mut first = lines.next().unwrap().split_whitespace();
    let method = first.next().unwrap().to_owned();
    let path = first.next().unwrap().to_owned();
    let headers: BTreeMap<_, _> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length = headers
        .get("content-length")
        .map(|value| value.parse::<usize>().unwrap())
        .unwrap_or(0);
    assert!(length <= 20 * 1024 * 1024);
    let mut body = vec![0; length];
    let mut received = 0;
    while received < body.len() {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "fixture request body deadline",
                )
            })?;
        stream.set_read_timeout(Some(remaining))?;
        match stream.read(&mut body[received..]) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "partial fixture request body",
                ))
            }
            Ok(count) => received += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(Some(Request {
        method,
        path,
        headers,
        body: if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).unwrap()
        },
    }))
}
fn write_before(
    stream: &mut TcpStream,
    mut bytes: &[u8],
    deadline: std::time::Instant,
) -> std::io::Result<()> {
    while !bytes.is_empty() {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "fixture reply deadline")
            })?;
        stream.set_write_timeout(Some(remaining))?;
        match stream.write(bytes) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "fixture reply stopped",
                ))
            }
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
fn reply(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let header=format!("HTTP/1.1 {status} Fixture\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
    if write_before(stream, header.as_bytes(), deadline).is_err() {
        return;
    }
    // Tiny writes exercise incremental SSE/UTF-8/base64 frame boundaries.
    let fragmented = body.len().min(224);
    for chunk in body[..fragmented]
        .chunks(7)
        .chain(body[fragmented..].chunks(16 * 1024))
    {
        if write_before(stream, chunk, deadline).is_err() {
            break;
        }
    }
}
fn json_reply(stream: &mut TcpStream, status: u16, value: Value) {
    reply(
        stream,
        status,
        "application/json",
        &serde_json::to_vec(&value).unwrap(),
    );
}
fn consume_fault(state: &Shared) -> Fault {
    std::mem::take(&mut *state.fault.lock().unwrap())
}
fn handle(mut stream: TcpStream, state: &Shared, data: bool) {
    let request = match read_request(&mut stream) {
        Ok(Some(request)) => request,
        Ok(None) => return, // A canceled speculative connection carried no request.
        Err(error) => {
            state
                .violations
                .lock()
                .unwrap()
                .push(format!("partial or malformed request: {error}"));
            return;
        }
    };
    state.requests.lock().unwrap().push(request.clone());
    let expected = format!("Bearer {}", if data { ATTACH } else { MANAGEMENT });
    if request.headers.get("authorization") != Some(&expected) {
        state
            .violations
            .lock()
            .unwrap()
            .push("wrong transport bearer".into());
        json_reply(&mut stream, 401, json!({"error":"wrong bearer"}));
        return;
    }
    if data {
        let session = request
            .path
            .strip_prefix("/v1/sessions/")
            .and_then(|rest| rest.strip_suffix("/events"))
            .unwrap_or("");
        let command = state.commands.lock().unwrap().remove(session);
        if request.method != "GET" || command.is_none() {
            json_reply(&mut stream, 404, json!({}));
            return;
        }
        let command = command.unwrap();
        let fault = {
            let pending = *state.fault.lock().unwrap();
            if matches!(
                pending,
                Fault::ShortUpload | Fault::UploadAckLoss | Fault::LateUpload
            ) || (matches!(pending, Fault::LaunchAckLoss)
                && command["command"][1] != "--request-file")
            {
                Fault::None
            } else {
                consume_fault(state)
            }
        };
        if matches!(fault, Fault::Redirect) {
            let header=format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {}/credential-leak\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",state.data_url);
            let _ = write_before(
                &mut stream,
                header.as_bytes(),
                std::time::Instant::now() + Duration::from_secs(5),
            );
            return;
        }
        let argv = command["command"].as_array().unwrap();
        let mut child = Command::new(argv[0].as_str().unwrap());
        child.args(argv[1..].iter().map(|arg| arg.as_str().unwrap()));
        child.current_dir(command["cwd"].as_str().unwrap());
        let stdout = tempfile::tempfile().unwrap();
        let stderr = tempfile::tempfile().unwrap();
        child.stdout(Stdio::from(stdout.try_clone().unwrap()));
        child.stderr(Stdio::from(stderr.try_clone().unwrap()));
        let mut process = child.spawn().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let status = loop {
            if stdout.metadata().unwrap().len() > 2 * 1024 * 1024
                || stderr.metadata().unwrap().len() > 2 * 1024 * 1024
            {
                let _ = process.kill();
                process.wait().unwrap();
                panic!("native RPC exceeded sampled fixture output bound; child killed and reaped");
            }
            if let Some(status) = process.try_wait().unwrap() {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                let _ = process.kill();
                process.wait().unwrap();
                panic!("native RPC exceeded fixture deadline; owned child killed and reaped");
            }
            thread::sleep(Duration::from_millis(5));
        };
        fn bounded_output(mut file: std::fs::File) -> Vec<u8> {
            use std::io::{Seek, SeekFrom};
            assert!(
                file.metadata().unwrap().len() <= 2 * 1024 * 1024,
                "native RPC output exceeded fixture bound"
            );
            file.seek(SeekFrom::Start(0)).unwrap();
            let mut bytes = Vec::new();
            file.take(2 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .unwrap();
            bytes
        }
        let stdout = bounded_output(stdout);
        let stderr = bounded_output(stderr);
        let mut events = format!(
            ": heartbeat\n\nevent: attached\ndata: {}\n\n",
            json!({"type":"attached","session_id":session,"route_generation":1,"current_node_id":"fixture"})
        );
        if matches!(fault, Fault::Resync) {
            events.push_str("event: resync_required\ndata: {\"type\":\"resync_required\",\"reason\":\"cursor_replay_unavailable\"}\n\n");
        }
        let started = format!(
            "event: process_started\ndata: {}\n\n",
            json!({"type":"process_started","process_id":session,"pid":process.id()})
        );
        if !matches!(fault, Fault::MissingStarted | Fault::ReorderedStarted) {
            events.push_str(&started);
        }
        let mut sequence = 1;
        for (kind, bytes) in [("stdout", &stdout), ("stderr", &stderr)] {
            for chunk in bytes.chunks(512 * 1024) {
                let encoded = if matches!(fault, Fault::InvalidBase64) {
                    "!invalid!".into()
                } else {
                    STANDARD.encode(chunk)
                };
                let seq = if matches!(fault, Fault::Gap) {
                    sequence + 1
                } else {
                    sequence
                };
                events.push_str(&format!(
                    "event: {kind}\ndata: {}\n\n",
                    json!({"type":kind,"seq":seq,"data_base64":encoded})
                ));
                sequence += 1;
            }
        }
        if matches!(fault, Fault::ReorderedStarted) {
            events.push_str(&started);
        }
        if matches!(fault, Fault::Oversized) {
            events.push_str(&format!("data: {}\n\n", "x".repeat(5 * 1024 * 1024)));
        }
        if !matches!(fault, Fault::Eof | Fault::LaunchAckLoss | Fault::ClosedOnly) {
            events.push_str(&format!("event: process_exited\ndata: {}\n\n", json!({"type":"process_exited","process_id":if matches!(fault,Fault::WrongProcess) {"proc_wrong"} else {session},"exit_code":status.code().unwrap_or(1),"status":"exited"})));
        }
        if matches!(fault, Fault::OutputAfterExit) {
            events.push_str(&format!(
                "event: stdout\ndata: {}\n\n",
                json!({"type":"stdout","seq":sequence,"data_base64":"eA=="})
            ));
        }
        if !matches!(fault, Fault::Eof | Fault::LaunchAckLoss) {
            events.push_str(&format!("event: closed\ndata: {}\n\n", json!({"type":"closed","reason":if matches!(fault,Fault::WrongClosedReason) {"unknown"} else {"backend_closed"}})));
            if matches!(fault, Fault::DuplicateClosed) {
                events.push_str(
                    "event: closed\ndata: {\"type\":\"closed\",\"reason\":\"backend_closed\"}\n\n",
                );
            }
        }
        if matches!(fault, Fault::Delay) {
            let heartbeat = b": still-running\n\n";
            let body = events.as_bytes();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let header=format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()+heartbeat.len()*25);
            if write_before(&mut stream, header.as_bytes(), deadline).is_err() {
                return;
            }
            for _ in 0..25 {
                if write_before(&mut stream, heartbeat, deadline).is_err() {
                    return;
                }
                thread::sleep(Duration::from_millis(10));
            }
            let _ = write_before(&mut stream, body, deadline);
        } else {
            reply(&mut stream, 200, "text/event-stream", events.as_bytes());
        }
        return;
    }
    if request.path == format!("/v1/sandboxes/{SANDBOX}") && request.method == "GET" {
        json_reply(
            &mut stream,
            200,
            json!({"id":SANDBOX,"name":null,"state":"running","failure_code":null,"conditions":[],"template":"seed:latest","template_digest":null,"resources":{"vcpu":null,"memory_mib":null,"disk_mib":null},"metadata":{},"expires_at":null,"idle_timeout":null,"idle_action":null,"created_at":"2026-10-18T00:00:00Z","updated_at":"2026-10-18T00:00:00Z"}),
        );
        return;
    }
    if request.path == format!("/v1/sandboxes/{SANDBOX}/commands") && request.method == "POST" {
        if request.body["user"] != USER
            || request.body["env"] != json!({})
            || request.body["stdin"] != false
            || request.body["timeout"] != "30s"
            || request.body["cwd"] != "/"
            || !request.headers.contains_key("idempotency-key")
        {
            state
                .violations
                .lock()
                .unwrap()
                .push("invalid command context".into());
        }
        let session = format!("proc_{}", uuid::Uuid::new_v4());
        state
            .commands
            .lock()
            .unwrap()
            .insert(session.clone(), request.body.clone());
        let fault = *state.fault.lock().unwrap();
        let returned_session = if matches!(fault, Fault::WrongSession) {
            "wrong-session"
        } else {
            &session
        };
        let events_url = if matches!(fault, Fault::AttachOrigin) {
            "http://127.0.0.1:1/steal".into()
        } else if matches!(fault, Fault::AttachPath) {
            format!("{}/v1/sessions/{session}/attach", state.data_url)
        } else {
            format!("{}/v1/sessions/{session}/events", state.data_url)
        };
        json_reply(
            &mut stream,
            201,
            json!({"session_id":returned_session,"process_id":session,"sandbox_id":if matches!(fault,Fault::WrongSandbox) {"sbx_wrong"} else {SANDBOX},"state":"pending","attach":{"url":format!("{}/v1/sessions/{session}/attach",state.data_url),"events_url":events_url,"token":ATTACH,"expires_at":"2099-01-01T00:00:00Z","token_transport":["authorization_header"]},"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","expires_at":"2099-01-01T00:00:00Z"}),
        );
        return;
    }
    if request.path == format!("/v1/sandboxes/{SANDBOX}/files:write") && request.method == "POST" {
        assert_eq!(request.body["user"], USER);
        assert_eq!(request.body["append"], true);
        assert!(request.headers.contains_key("idempotency-key"));
        let path = request.body["path"].as_str().unwrap();
        let fault = *state.fault.lock().unwrap();
        if matches!(fault, Fault::LateUpload) {
            thread::sleep(Duration::from_millis(500));
            consume_fault(state);
        }
        let metadata = fs::symlink_metadata(path).unwrap();
        assert!(metadata.is_file() && metadata.nlink() == 1);
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(
            metadata.len(),
            0,
            "append transport must own a fresh request file"
        );
        let bytes = STANDARD
            .decode(request.body["data_base64"].as_str().unwrap())
            .unwrap();
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .unwrap();
        file.write_all(&bytes).unwrap();
        file.sync_all().unwrap();
        let fault = *state.fault.lock().unwrap();
        if matches!(fault, Fault::UploadAckLoss) {
            consume_fault(state);
            return;
        }
        let length = if matches!(fault, Fault::ShortUpload) {
            consume_fault(state);
            bytes.len().saturating_sub(1)
        } else {
            bytes.len()
        };
        json_reply(&mut stream, 200, json!({"bytes_written":length}));
        return;
    }
    state
        .violations
        .lock()
        .unwrap()
        .push(format!("unexpected {} {}", request.method, request.path));
    json_reply(
        &mut stream,
        404,
        json!({"error":"unsupported fixture route"}),
    );
}

/// Retain evidence instead of deleting a fixture with uncertain native owners.
/// This guard runs before TempDir deletion, including failed test unwinding.
pub struct NativeDirectory(Option<tempfile::TempDir>);
impl NativeDirectory {
    pub fn new() -> Self {
        Self(Some(
            tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap(),
        ))
    }
    pub fn path(&self) -> &std::path::Path {
        self.0.as_ref().unwrap().path()
    }
    fn quiescent(&self) -> Result<(), String> {
        use fs2::FileExt;
        use std::os::unix::fs::OpenOptionsExt;
        let namespace = self.path().join(".workspace-sdk");
        if !namespace.exists() {
            return Ok(());
        }
        let entries = fs::read_dir(&namespace).map_err(|error| error.to_string())?;
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut records = Vec::new();
        for (index, entry) in entries.take(1025).enumerate() {
            if index >= 1024 {
                return Err("native control inventory exceeds fixture bound".into());
            }
            let entry = entry.map_err(|error| error.to_string())?;
            if records.len() >= 1024 {
                return Err("native inventory exceeds fixture bound".into());
            }
            if !entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_dir()
            {
                return Err("unexpected control entry".into());
            }
            let control = entry.path();
            let owner_file = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(control.join("owner.json"))
                .map_err(|error| error.to_string())?;
            let metadata = owner_file.metadata().map_err(|error| error.to_string())?;
            if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > 16 * 1024 {
                return Err("invalid bounded owner receipt".into());
            }
            let mut bytes = Vec::new();
            owner_file
                .take(16 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            let owner: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            if owner["control"].as_str() != control.to_str()
                || owner["workspace_id"].as_str() != entry.file_name().to_str()
            {
                return Err("native fixture ownership differs".into());
            }
            let project = std::path::Path::new(owner["root"].as_str().ok_or("missing owner root")?);
            if !project.starts_with(self.path()) || owner["ownership"] != "attached" {
                return Err("owner root or attachment is outside fixture scope".into());
            }
            let runs = control.join("runs");
            if !runs.exists() {
                continue;
            }
            for run in fs::read_dir(runs).map_err(|error| error.to_string())? {
                if records.len() >= 1024 {
                    return Err("native run inventory exceeds fixture bound".into());
                }
                let run = run.map_err(|error| error.to_string())?;
                if !run.file_type().map_err(|error| error.to_string())?.is_dir() {
                    return Err("unexpected native run entry".into());
                }
                workspace_sdk::RunId::new(
                    run.file_name().to_str().ok_or("nonUTF8 native identity")?,
                )
                .map_err(|error| error.to_string())?;
                records.push(run.path());
            }
        }
        for directory in records {
            loop {
                let read = |name: &str| -> Result<std::fs::File, String> {
                    let file = OpenOptions::new()
                        .read(true)
                        .write(name == "active.lock")
                        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                        .open(directory.join(name))
                        .map_err(|error| error.to_string())?;
                    let metadata = file.metadata().map_err(|error| error.to_string())?;
                    if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > 16 * 1024 {
                        return Err("invalid bounded native receipt".into());
                    }
                    Ok(file)
                };
                let mut bytes = Vec::new();
                read("status.json")?
                    .take(16 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| error.to_string())?;
                let status: workspace_sdk::RunState =
                    serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
                if matches!(status, workspace_sdk::RunState::Lost { .. }) {
                    return Err("native fixture ownership remains Lost".into());
                }
                if status.is_terminal() {
                    let lock = read("active.lock")?;
                    match lock.try_lock_exclusive() {
                        Ok(()) => {
                            let mut bytes = Vec::new();
                            read("status.json")?
                                .take(16 * 1024 + 1)
                                .read_to_end(&mut bytes)
                                .map_err(|error| error.to_string())?;
                            let confirmed: workspace_sdk::RunState = serde_json::from_slice(&bytes)
                                .map_err(|error| error.to_string())?;
                            FileExt::unlock(&lock).map_err(|error| error.to_string())?;
                            if !confirmed.is_terminal() {
                                return Err("native receipt changed during ownership check".into());
                            }
                            break;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                        Err(error) => return Err(error.to_string()),
                    }
                }
                if std::time::Instant::now() >= deadline {
                    return Err(
                        "native fixture owner did not become terminal and release lock".into(),
                    );
                }
                thread::sleep(Duration::from_millis(5));
            }
        }
        Ok(())
    }
}
impl Drop for NativeDirectory {
    fn drop(&mut self) {
        if thread::panicking() {
            let retained = self.0.take().unwrap().keep();
            eprintln!(
                "retained native fixture during test/handler uncertainty: {}",
                retained.display()
            );
            return;
        }
        if let Err(error) = self.quiescent() {
            let retained = self.0.take().unwrap().keep();
            let message = format!(
                "native fixture cleanup unresolved: {error}; retained {}",
                retained.display()
            );
            if thread::panicking() {
                eprintln!("{message}");
            } else {
                panic!("{message}");
            }
        }
    }
}
