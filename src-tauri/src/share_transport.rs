//! Bounded, cancellable HTTP transport for the single registered Quick Share file.
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const MAX_CLIENTS: usize = 4;
const HEADER_LIMIT: usize = 8192;

struct Clients {
    stopping: bool,
    sockets: HashMap<u64, TcpStream>,
    active: u64,
    last_activity: Instant,
}

pub struct Control {
    clients: Mutex<Clients>,
    pub completed: AtomicU64,
}

impl Control {
    pub fn new() -> Self {
        Self {
            clients: Mutex::new(Clients {
                stopping: false,
                sockets: HashMap::new(),
                active: 0,
                last_activity: Instant::now(),
            }),
            completed: AtomicU64::new(0),
        }
    }
    pub fn active(&self) -> u64 {
        self.clients
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .active
    }
    fn stopping(&self) -> bool {
        self.clients
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stopping
    }
    /// The same lock admits a download and authorizes stopping: no check/stop gap.
    pub fn stop(&self, force: bool) -> bool {
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        if !force && clients.active > 0 {
            return false;
        }
        clients.stopping = true;
        for socket in clients.sockets.values() {
            let _ = socket.shutdown(Shutdown::Both);
        }
        true
    }
    fn begin(&self) -> bool {
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        if clients.stopping {
            return false;
        }
        clients.active += 1;
        clients.last_activity = Instant::now();
        true
    }
    fn end(&self) {
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        clients.active = clients.active.saturating_sub(1);
        clients.last_activity = Instant::now();
    }
}

pub struct Config {
    pub path: PathBuf,
    pub token: String,
    pub lan_ip: Ipv4Addr,
    pub port: u16,
    pub mime: &'static str,
    pub disposition: String,
    pub idle_timeout: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    Stopped,
    IdleTimeout,
}
impl StopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::IdleTimeout => "idle_timeout",
        }
    }
}

pub enum Event {
    Activity(u64),
    Completed { count: u64, bytes: u64 },
    Aborted(String),
}
type Notify = Arc<dyn Fn(Event) + Send + Sync>;

/// Owns the listener directly; returning drops it before the caller's join completes.
pub fn run(
    listener: TcpListener,
    config: Config,
    control: Arc<Control>,
    notify: Notify,
) -> Result<StopReason, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let config = Arc::new(config);
    let mut workers: Vec<thread::JoinHandle<()>> = Vec::new();
    let mut serial = 0;
    let result = loop {
        workers.retain(|worker| !worker.is_finished());
        {
            let clients = control.clients.lock().unwrap_or_else(|e| e.into_inner());
            if clients.stopping {
                break Ok(StopReason::Stopped);
            }
            if clients.active == 0 && clients.last_activity.elapsed() >= config.idle_timeout {
                break Ok(StopReason::IdleTimeout);
            }
        }
        match listener.accept() {
            Ok((mut socket, _)) => {
                if socket.set_nonblocking(true).is_err() {
                    continue;
                }
                let mut clients = control.clients.lock().unwrap_or_else(|e| e.into_inner());
                if clients.stopping || workers.len() >= MAX_CLIENTS {
                    let _ = socket.shutdown(Shutdown::Both);
                    continue;
                }
                serial += 1;
                let id = serial;
                let duplicate = match socket.try_clone() {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                clients.sockets.insert(id, duplicate);
                drop(clients);
                let control = control.clone();
                let config = config.clone();
                let notify = notify.clone();
                workers.push(thread::spawn(move || {
                    serve(&mut socket, &config, &control, &notify);
                    let _ = socket.shutdown(Shutdown::Both);
                    control
                        .clients
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .sockets
                        .remove(&id);
                }));
            }
            Err(e) if retryable_accept_error(e.kind()) => thread::sleep(Duration::from_millis(15)),
            Err(e) => break Err(format!("accept: {e}")),
        }
    };
    control.stop(true);
    // The listener is released even if a client was stalled reading the video.
    drop(listener);
    for worker in workers {
        let _ = worker.join();
    }
    result
}

fn retryable_accept_error(kind: std::io::ErrorKind) -> bool {
    matches!(
        kind,
        std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::TimedOut
    )
}

fn reject(socket: &mut TcpStream, code: &str) {
    let _ = write!(
        socket,
        "HTTP/1.1 {code}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
}

fn write_cancellable(
    socket: &mut TcpStream,
    mut bytes: &[u8],
    control: &Control,
) -> std::io::Result<()> {
    let mut deadline = Instant::now() + Duration::from_secs(15);
    while !bytes.is_empty() {
        if control.stopping() {
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        match socket.write(bytes) {
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(n) => {
                bytes = &bytes[n..];
                deadline = Instant::now() + Duration::from_secs(15);
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(10))
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn serve(socket: &mut TcpStream, config: &Config, control: &Control, notify: &Notify) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut header = Vec::new();
    let mut bytes = [0; 1024];
    while !header.windows(4).any(|v| v == b"\r\n\r\n") {
        if control.stopping() {
            return;
        }
        if header.len() >= HEADER_LIMIT || Instant::now() >= deadline {
            reject(socket, "431 Request Header Fields Too Large");
            return;
        }
        match socket.read(&mut bytes) {
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10))
            }
            Ok(0) | Err(_) => return,
            Ok(n) => header.extend_from_slice(&bytes[..n]),
        }
    }
    if header.len() > HEADER_LIMIT {
        reject(socket, "431 Request Header Fields Too Large");
        return;
    }
    let Ok(header) = std::str::from_utf8(&header) else {
        reject(socket, "400 Bad Request");
        return;
    };
    let mut lines = header.split("\r\n");
    let request: Vec<_> = lines
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    if request.len() != 3 || !matches!(request[2], "HTTP/1.1" | "HTTP/1.0") {
        reject(socket, "400 Bad Request");
        return;
    }
    if request[0] != "GET" {
        reject(socket, "405 Method Not Allowed");
        return;
    }
    if !super::share_server::request_path_matches(request[1], &format!("/w/{}", config.token)) {
        reject(socket, "404 Not Found");
        return;
    }
    let hosts: Vec<_> = lines
        .filter_map(|line| line.split_once(':'))
        .filter(|(key, _)| key.eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.trim())
        .collect();
    if hosts.len() != 1
        || !super::share_server::host_header_matches(hosts[0], config.lan_ip, config.port)
    {
        reject(socket, "403 Forbidden");
        return;
    }
    let Ok(mut file) = File::open(&config.path) else {
        reject(socket, "404 Not Found");
        return;
    };
    let Ok(metadata) = file.metadata() else {
        reject(socket, "500 Internal Server Error");
        return;
    };
    if !control.begin() {
        return;
    }
    notify(Event::Activity(control.active()));
    let result = (|| -> std::io::Result<()> {
        let header = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: {}\r\nContent-Disposition: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", metadata.len(), config.mime, config.disposition);
        write_cancellable(socket, header.as_bytes(), control)?;
        let mut buffer = [0; 65536];
        let mut sent = 0u64;
        while sent < metadata.len() {
            if control.stopping() {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            let limit = (metadata.len() - sent).min(buffer.len() as u64) as usize;
            let n = file.read(&mut buffer[..limit])?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "video changed during transfer",
                ));
            }
            write_cancellable(socket, &buffer[..n], control)?;
            sent += n as u64;
        }
        Ok(())
    })();
    control.end();
    notify(Event::Activity(control.active()));
    if result.is_ok() {
        notify(Event::Completed {
            count: control.completed.fetch_add(1, Ordering::SeqCst) + 1,
            bytes: metadata.len(),
        });
    } else if let Err(error) = result {
        notify(Event::Aborted(error.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accept_retries_connection_failures_but_not_listener_failures() {
        for kind in [
            std::io::ErrorKind::ConnectionAborted,
            std::io::ErrorKind::ConnectionReset,
            std::io::ErrorKind::Interrupted,
            std::io::ErrorKind::WouldBlock,
        ] {
            assert!(retryable_accept_error(kind));
        }
        assert!(!retryable_accept_error(
            std::io::ErrorKind::PermissionDenied
        ));
        assert!(!retryable_accept_error(std::io::ErrorKind::InvalidInput));
    }
    struct Running {
        control: Arc<Control>,
        worker: Option<thread::JoinHandle<Result<StopReason, String>>>,
        port: u16,
        path: PathBuf,
    }
    impl Running {
        fn start(port: u16, bytes: u64, idle: Duration) -> Self {
            let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let path =
                std::env::temp_dir().join(format!("wui-share-test-{}.mp4", uuid::Uuid::new_v4()));
            let file = File::create(&path).unwrap();
            file.set_len(bytes).unwrap();
            drop(file);
            let control = Arc::new(Control::new());
            let config = Config {
                path: path.clone(),
                token: "fixture-token".into(),
                lan_ip: Ipv4Addr::LOCALHOST,
                port,
                mime: "video/mp4",
                disposition: "attachment; filename=clip.mp4".into(),
                idle_timeout: idle,
            };
            let owned_control = control.clone();
            let worker = thread::spawn(move || {
                run(
                    listener,
                    config,
                    owned_control,
                    Arc::new(|event| {
                        if let Event::Aborted(error) = event {
                            eprintln!("fixture download aborted: {error}");
                        }
                    }),
                )
            });
            Self {
                control,
                worker: Some(worker),
                port,
                path,
            }
        }
        fn connect(&self, request: &str) -> TcpStream {
            let mut socket = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            socket.write_all(request.as_bytes()).unwrap();
            socket
        }
        fn get(&self) -> TcpStream {
            self.connect(&format!(
                "GET /w/fixture-token HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
                self.port
            ))
        }
        fn stop(&mut self) -> StopReason {
            self.control.stop(true);
            self.worker.take().unwrap().join().unwrap().unwrap()
        }
    }
    impl Drop for Running {
        fn drop(&mut self) {
            self.control.stop(true);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
            let _ = std::fs::remove_file(&self.path);
        }
    }
    #[test]
    fn repeated_downloads_and_twenty_restarts_release_the_same_port() {
        let mut port = 0;
        for _ in 0..20 {
            let mut server = Running::start(port, 1024, Duration::from_secs(180));
            port = server.port;
            for _ in 0..2 {
                let mut response = Vec::new();
                server.get().read_to_end(&mut response).unwrap();
                assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
                let body = response.windows(4).position(|v| v == b"\r\n\r\n").unwrap() + 4;
                assert_eq!(response.len() - body, 1024);
            }
            assert_eq!(server.control.completed.load(Ordering::SeqCst), 2);
            assert_eq!(server.stop(), StopReason::Stopped);
        }
        let _listener =
            TcpListener::bind(("127.0.0.1", port)).expect("immediate reuse after repeated close");
    }
    #[test]
    fn closing_active_download_requires_confirmation_and_force_interrupts_stalled_client() {
        let mut server = Running::start(0, 256 * 1024 * 1024, Duration::from_millis(40));
        let _client = server.get(); // Deliberately never consume the response.
        let deadline = Instant::now() + Duration::from_secs(3);
        while server.control.active() == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(server.control.active(), 1);
        assert!(!server.control.stop(false));
        thread::sleep(Duration::from_millis(80));
        assert!(
            !server.worker.as_ref().unwrap().is_finished(),
            "active download is not idle: active={} completed={}",
            server.control.active(),
            server.control.completed.load(Ordering::SeqCst)
        );
        let started = Instant::now();
        assert_eq!(server.stop(), StopReason::Stopped);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(server.control.completed.load(Ordering::SeqCst), 0);
        let _listener = TcpListener::bind(("127.0.0.1", server.port)).unwrap();
    }
    #[test]
    fn incomplete_headers_do_not_prevent_shutdown() {
        let mut server = Running::start(0, 1, Duration::from_secs(180));
        let _client = server.connect("GET /w/fixture-token HTTP/1.1\r\n");
        thread::sleep(Duration::from_millis(40));
        let started = Instant::now();
        server.stop();
        assert!(started.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn token_host_and_method_checks_are_enforced_over_tcp() {
        let mut server = Running::start(0, 5, Duration::from_secs(180));
        for (request, status) in [
            (
                format!(
                    "GET /w/wrong HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
                    server.port
                ),
                "404",
            ),
            (
                "GET /w/fixture-token HTTP/1.1\r\nHost: attacker.invalid\r\n\r\n".into(),
                "403",
            ),
            (
                "GET /w/fixture-token HTTP/1.1\r\nHost: localhost\r\nHost: localhost\r\n\r\n"
                    .into(),
                "403",
            ),
            (
                "POST /w/fixture-token HTTP/1.1\r\nHost: localhost\r\n\r\n".into(),
                "405",
            ),
        ] {
            let mut response = String::new();
            server
                .connect(&request)
                .read_to_string(&mut response)
                .unwrap();
            assert!(response.starts_with(&format!("HTTP/1.1 {status}")));
        }
        assert_eq!(server.control.completed.load(Ordering::SeqCst), 0);
        server.stop();
    }
    #[test]
    fn idle_exit_releases_listener() {
        let mut server = Running::start(0, 5, Duration::from_millis(40));
        assert_eq!(
            server.worker.take().unwrap().join().unwrap().unwrap(),
            StopReason::IdleTimeout
        );
        let _listener = TcpListener::bind(("127.0.0.1", server.port)).unwrap();
    }
}
