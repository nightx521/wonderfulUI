//! Quick Share session ownership, QR rendering and synchronous shutdown.

use std::collections::HashMap;
use std::net::{Ipv4Addr, TcpListener};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use rand::RngCore;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::firewall;
use crate::lan_ip;
use crate::share_policy::{ipc_error, ShareErrorCode, QUICK_SHARE_PORT};
use crate::share_transport::{self, Control, Event, StopReason as ServerStopReason};

const CANCELLED_SESSION_TTL: Duration = Duration::from_secs(60);
const MAX_CANCELLED_SESSIONS: usize = 128;

/// 启动 server 后返回给前端的"拨号盘"信息。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareServerInfo {
    /// Frontend-generated UUID identity. Events and stop requests carry it so
    /// an old server cannot mutate a replacement server's UI, including after
    /// a WebView reload inside the same Tauri process.
    pub session_id: String,
    pub port: u16,
    pub token: String,
    pub url: String,
    pub lan_ip: String,
    /// QR 内容（直接是 URL，手机扫码即用）。前端不用自己再生成。
    pub qr_svg: String,
    pub video_name: String,
    pub video_size: u64,
    /// Unix epoch 秒。给前端用作倒计时的起点；Rust 端 3 分钟空闲
    /// 超时基于 `last_request` 而非这个时间。
    pub started_at_unix: u64,
}

/// 启动失败 / 停止原因。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareServerStatus {
    pub running: bool,
    pub info: Option<ShareServerInfo>,
    pub download_count: u64,
    pub active_downloads: u64,
    pub last_error: Option<String>,
}

/// 内部状态：要么有 server 句柄，要么有错误。
struct Inner {
    info: ShareServerInfo,
    control: Arc<Control>,
    worker: Option<thread::JoinHandle<Result<ServerStopReason, String>>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopOutcome {
    pub stopped: bool,
    pub active_downloads: u64,
}

pub struct ShareServerState {
    inner: Mutex<Option<Inner>>,
    lifecycle: Mutex<()>,
    /// A stop can arrive before its start command publishes `inner`. Remember
    /// that session so the late start closes only itself instead of replacing
    /// a newer server.
    cancelled_sessions: Mutex<HashMap<String, Instant>>,
    /// 空闲超时
    idle_timeout: Duration,
}

impl ShareServerState {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
            lifecycle: Mutex::new(()),
            cancelled_sessions: Mutex::new(HashMap::new()),
            idle_timeout: Duration::from_secs(3 * 60), // 3 分钟
        }
    }
}

impl Default for ShareServerState {
    fn default() -> Self {
        Self::new()
    }
}

/// Bind once to the documented Quick Share port. Returning the live server
/// avoids the check-then-bind race of probing a port and reopening it in
/// another thread.
fn bind_server() -> Result<(TcpListener, u16), String> {
    let server = TcpListener::bind(("0.0.0.0", QUICK_SHARE_PORT)).map_err(|error| {
        let in_use = error.kind() == std::io::ErrorKind::AddrInUse;
        crate::app_log::write(
            crate::app_log::LogLevel::Error,
            "share",
            format!("bind 0.0.0.0:{QUICK_SHARE_PORT} failed: {error}"),
        );
        if in_use {
            ipc_error(ShareErrorCode::PortInUse)
        } else {
            ipc_error(ShareErrorCode::PortBindFailed)
        }
    })?;
    let port = server
        .local_addr()
        .map_err(|_| ipc_error(ShareErrorCode::PortBindFailed))?
        .port();
    if port != QUICK_SHARE_PORT {
        crate::app_log::write(
            crate::app_log::LogLevel::Error,
            "share",
            format!("server returned unexpected port {port}"),
        );
        return Err(ipc_error(ShareErrorCode::PortBindFailed));
    }
    Ok((server, port))
}

/// 生成 256-bit base64url token（URL 安全，43 字符）。
fn make_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// 生成圆点 QR SVG 字符串（v0.14 API）。错误返回空字符串让前端兜底。
///
/// **圆点风格**：qrcode 0.14 默认用 `<rect>` 画每个 module，看起来像工业
/// 二维码。手动遍历 QrCode 矩阵 + 用 `<circle r=0.5>` 画每个 dark module，
/// 立刻有"产品感"——圆点之间的留白比方块呼吸感强得多。Rust 端直接输出
/// 圆点 SVG，前端不用 post-process。
///
/// **容错 + 静默区**：`EcLevel::H` (30% 容错) + 4-module 静默区。
/// iPhone / Android 原生扫码在镜头抖动 / 屏幕划痕 / 偏光贴膜等场景下
/// 都能可靠识别。
fn make_qr_svg(content: &str) -> String {
    use qrcode::EcLevel;
    use qrcode::QrCode;

    let code = match QrCode::with_error_correction_level(content, EcLevel::H) {
        Ok(c) => c,
        Err(_) => match QrCode::new(content) {
            Ok(c) => c,
            Err(_) => return String::new(),
        },
    };

    let width = code.width() as i32; // module 数（不含静默区）
    let quiet: i32 = 4; // 标准 4-module 静默区
    let total = width + quiet * 2; // 完整 viewBox 边长

    let mut svg = String::with_capacity((width * width * 32) as usize);
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" \
         viewBox=\"0 0 {total} {total}\" width=\"280\" height=\"280\" \
         shape-rendering=\"geometricPrecision\">"
    ));
    // 白色背景
    svg.push_str(&format!(
        "<rect width=\"{total}\" height=\"{total}\" fill=\"#fff\"/>"
    ));

    // 画每个 dark module 为圆点（r=0.45，半径 0.45 留 0.05 间隙 — 视觉上
    // 是"圆点"而不是"圆盘"）。色值用主题主色 —— 前端用 --accent 覆盖。
    // Rust 端给一个 fallback hex，前端 inject 真实颜色。
    let module_color = "#d23a3a"; // 与前端 var(--accent, #d04040) 同步
    let radius = 0.45_f64;
    for y in 0..width {
        for x in 0..width {
            if code[(x as usize, y as usize)] == qrcode::Color::Dark {
                let cx = (x + quiet) as f64 + 0.5;
                let cy = (y + quiet) as f64 + 0.5;
                svg.push_str(&format!(
                    "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{:.2}\" fill=\"{}\"/>",
                    cx, cy, radius, module_color
                ));
            }
        }
    }
    svg.push_str("</svg>");
    svg
}

/// 启动 server。结果返回 `ShareServerInfo` 给前端展示。
pub fn start_server(
    app: &AppHandle,
    state: &ShareServerState,
    video_path: PathBuf,
    session_id: String,
) -> Result<ShareServerInfo, String> {
    let _lifecycle = state.lifecycle.lock().map_err(|_| "快传状态不可用")?;
    if state.inner.lock().map_err(|_| "快传状态不可用")?.is_some() {
        return Err(ipc_error(ShareErrorCode::PortInUse));
    }
    crate::app_log::write(
        crate::app_log::LogLevel::Info,
        "share",
        format!("start requested: path={}", video_path.display()),
    );
    if !video_path.exists() {
        crate::app_log::write(
            crate::app_log::LogLevel::Error,
            "share",
            format!("source missing: {}", video_path.display()),
        );
        return Err(ipc_error(ShareErrorCode::SourceUnavailable));
    }
    let video_size = std::fs::metadata(&video_path)
        .map_err(|error| {
            crate::app_log::write(
                crate::app_log::LogLevel::Error,
                "share",
                format!("stat video failed: {error}"),
            );
            ipc_error(ShareErrorCode::SourceUnavailable)
        })?
        .len();
    let video_name = video_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("video.mp4")
        .to_string();

    // Firewall readiness is checked before binding so a missing rule cannot
    // leave a live server behind while UAC is being requested.
    firewall::ensure_ready().map_err(|error| {
        crate::app_log::write(
            crate::app_log::LogLevel::Error,
            "share",
            format!("firewall readiness failed: {error}"),
        );
        error
    })?;

    let (server, port) = bind_server()?;
    let token = make_token();
    let lan_ip =
        lan_ip::detect_lan_ipv4().ok_or_else(|| ipc_error(ShareErrorCode::LanIpUnavailable))?;
    let lan_ip_v4: Ipv4Addr = lan_ip
        .parse()
        .map_err(|_| ipc_error(ShareErrorCode::LanIpUnavailable))?;
    let url = format!("http://{}:{}/w/{}", lan_ip, port, token);

    let qr_svg = make_qr_svg(&url);
    crate::app_log::write(
        crate::app_log::LogLevel::Info,
        "share",
        format!("listening on {lan_ip}:{port} size={video_size} name={video_name}"),
    );

    let control = Arc::new(Control::new());
    let config = share_transport::Config {
        path: video_path,
        token: token.clone(),
        lan_ip: lan_ip_v4,
        port,
        mime: guess_mime(&video_name),
        disposition: content_disposition_attachment(&video_name),
        idle_timeout: state.idle_timeout,
    };
    let app_for_thread = app.clone();
    let session_for_thread = session_id.clone();
    let name_for_thread = video_name.clone();
    let thread_control = control.clone();
    let worker = thread::Builder::new().name("wui-share-server".into()).spawn(move || {
        let event_app = app_for_thread.clone();
        let event_session = session_for_thread.clone();
        let result = share_transport::run(server, config, thread_control, Arc::new(move |event| {
            match event {
                Event::Activity(active) => { let _ = event_app.emit("wui://share_activity", serde_json::json!({ "sessionId": event_session, "activeDownloads": active })); }
                Event::Completed { count, bytes } => { let _ = event_app.emit("wui://share_downloaded", serde_json::json!({ "sessionId": event_session, "count": count, "filename": name_for_thread, "sizeBytes": bytes })); }
                Event::Aborted(error) => crate::app_log::write(crate::app_log::LogLevel::Warn, "share", format!("download aborted: {error}")),
            }
        }));
        clear_server_if_session(&app_for_thread.state::<ShareServerState>(), &session_for_thread);
        let (reason, message) = match &result { Ok(reason) => (reason.as_str(), None), Err(message) => ("error", Some(message.as_str())) };
        crate::app_log::write(crate::app_log::LogLevel::Info, "share", format!("server thread exited: {reason}"));
        let _ = app_for_thread.emit("wui://share_server_stopped", serde_json::json!({ "sessionId": session_for_thread, "reason": reason, "message": message }));
        result
    }).map_err(|_| ipc_error(ShareErrorCode::ServerStartFailed))?;
    let info = ShareServerInfo {
        session_id: session_id.clone(),
        port,
        token,
        url,
        lan_ip,
        qr_svg,
        video_name,
        video_size,
        started_at_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    };
    // Publication and cancellation share a lock order. stop(session_id) can
    // arrive while bind/thread startup is in flight; if so, this new thread is
    // stopped without ever replacing the current server.
    let mut cancelled = state
        .cancelled_sessions
        .lock()
        .map_err(|e| format!("lock cancelled sessions: {e}"))?;
    prune_cancelled_sessions(&mut cancelled);
    if cancelled.remove(&session_id).is_some() {
        control.stop(true);
        drop(cancelled);
        let _ = worker.join();
        return Err(ipc_error(ShareErrorCode::StartCancelled));
    }
    let mut guard = state.inner.lock().map_err(|e| format!("lock: {e}"))?;
    *guard = Some(Inner {
        info: info.clone(),
        control,
        worker: Some(worker),
    });
    Ok(info)
}

pub(crate) fn request_path_matches(url: &str, expected_path: &str) -> bool {
    url.split_once('?').map_or(url, |(path, _)| path) == expected_path
}

pub(crate) fn host_header_matches(value: &str, lan_ip: Ipv4Addr, port: u16) -> bool {
    let authority = value.trim();
    if authority.is_empty() || authority.contains(['/', '@']) || authority.contains("://") {
        return false;
    }
    let mut parts = authority.split(':');
    let host = parts.next().unwrap_or_default();
    let header_port = match parts.next() {
        Some(raw) => match raw.parse::<u16>() {
            Ok(value) => Some(value),
            Err(_) => return false,
        },
        None => None,
    };
    if parts.next().is_some() || header_port.is_some_and(|value| value != port) {
        return false;
    }
    host == "localhost" || host == "127.0.0.1" || host == lan_ip.to_string()
}

fn guess_mime(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".mp4") {
        "video/mp4"
    } else if lower.ends_with(".webm") {
        "video/webm"
    } else if lower.ends_with(".mkv") {
        "video/x-matroska"
    } else {
        "application/octet-stream"
    }
}

/// RFC 6266 / 5987 attachment header. ASCII `filename=` fallback + UTF-8
/// `filename*=` so Chinese highlight names download correctly on browsers.
fn content_disposition_attachment(filename: &str) -> String {
    let ascii_fallback: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else if c.is_ascii() && c != '"' && c != '\\' && c != ';' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded = percent_encode_rfc5987(filename);
    format!(
        "attachment; filename=\"{}\"; filename*=UTF-8''{}",
        ascii_fallback, encoded
    )
}

fn percent_encode_rfc5987(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn stop_server(
    state: &ShareServerState,
    session_id: Option<String>,
    force: bool,
) -> Result<StopOutcome, String> {
    let _lifecycle = state.lifecycle.lock().map_err(|_| "快传状态不可用")?;
    let mut cancelled = state
        .cancelled_sessions
        .lock()
        .map_err(|_| "快传状态不可用")?;
    prune_cancelled_sessions(&mut cancelled);
    let mut guard = state.inner.lock().map_err(|_| "快传状态不可用")?;
    let matches = guard.as_ref().is_some_and(|inner| {
        session_id
            .as_ref()
            .is_none_or(|id| *id == inner.info.session_id)
    });
    if matches {
        let inner = guard.as_ref().unwrap();
        if !inner.control.stop(force) {
            return Ok(StopOutcome {
                stopped: false,
                active_downloads: inner.control.active(),
            });
        }
        let inner = guard.take().unwrap();
        cancelled.remove(&inner.info.session_id);
        drop(guard);
        drop(cancelled);
        // Joining outside the state lock allows the server's terminal event to finish.
        if let Some(worker) = inner.worker {
            worker
                .join()
                .map_err(|_| "快传关闭失败，请重试")?
                .map_err(|_| "快传关闭失败，请重试")?;
        }
    } else if let Some(expected) = session_id {
        if !cancelled.contains_key(&expected) && cancelled.len() >= MAX_CANCELLED_SESSIONS {
            return Err("待处理的快传取消请求过多".into());
        }
        cancelled.insert(expected, Instant::now());
    }
    Ok(StopOutcome {
        stopped: true,
        active_downloads: 0,
    })
}

fn prune_cancelled_sessions(cancelled: &mut HashMap<String, Instant>) {
    cancelled.retain(|_, recorded_at| recorded_at.elapsed() <= CANCELLED_SESSION_TTL);
}

fn clear_server_if_session(state: &ShareServerState, session_id: &str) {
    let Ok(mut guard) = state.inner.lock() else {
        return;
    };
    if guard
        .as_ref()
        .is_some_and(|inner| inner.info.session_id == session_id)
    {
        guard.take();
    }
}

pub fn status(state: &ShareServerState) -> ShareServerStatus {
    let guard = match state.inner.lock() {
        Ok(g) => g,
        Err(_) => {
            return ShareServerStatus {
                running: false,
                info: None,
                download_count: 0,
                active_downloads: 0,
                last_error: Some("state poisoned".into()),
            };
        }
    };
    match guard.as_ref() {
        Some(inner) => ShareServerStatus {
            running: true,
            info: Some(inner.info.clone()),
            download_count: inner.control.completed.load(Ordering::SeqCst),
            active_downloads: inner.control.active(),
            last_error: None,
        },
        None => ShareServerStatus {
            running: false,
            info: None,
            download_count: 0,
            active_downloads: 0,
            last_error: None,
        },
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn test_info(session_id: &str) -> ShareServerInfo {
        ShareServerInfo {
            session_id: session_id.into(),
            port: QUICK_SHARE_PORT,
            token: format!("token-{session_id}"),
            url: format!("http://192.168.1.42:{QUICK_SHARE_PORT}/w/token-{session_id}"),
            lan_ip: "192.168.1.42".into(),
            qr_svg: "<svg/>".into(),
            video_name: "clip.mp4".into(),
            video_size: 1,
            started_at_unix: 1,
        }
    }

    #[test]
    fn make_token_is_unique_and_url_safe() {
        let t1 = make_token();
        let t2 = make_token();
        assert_ne!(t1, t2, "token must be unique per call");
        // 32 bytes base64url-no-pad = 43 chars, no padding
        assert_eq!(t1.len(), 43);
        assert!(
            t1.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "token must be base64url charset"
        );
    }

    #[test]
    fn make_qr_svg_returns_dotted_svg() {
        // Rust 端用 <circle> 画每个 dark module（圆点 QR），不用 rect。
        let svg = make_qr_svg(&format!("http://192.168.1.42:{QUICK_SHARE_PORT}/w/abc123"));
        assert!(svg.contains("<svg"), "got: {svg}");
        assert!(svg.contains("</svg>"));
        // 圆点风格：每 dark module 是 <circle cx=.. cy=.. r=0.45/>
        assert!(svg.contains("<circle"), "expected circle-based modules");
        // 包含白色背景 rect（让前端 QR 框有底色，iPhone 扫码可靠）
        assert!(svg.contains("fill=\"#fff\""), "expected white background");
        assert!(!svg.is_empty());
    }

    #[test]
    fn bind_server_reports_the_fixed_port() {
        let _lock = bind_test_lock().lock().unwrap();
        let (server, port) = bind_server().expect("fixed port is available");
        assert_eq!(port, QUICK_SHARE_PORT);
        assert_eq!(
            server.local_addr().ok().map(|addr| addr.port()),
            Some(QUICK_SHARE_PORT)
        );
    }

    #[test]
    fn bind_server_reports_port_in_use_without_falling_back() {
        let _lock = bind_test_lock().lock().unwrap();
        // Another process may already own the fixed port in a developer or CI
        // environment. That is still a valid setup for this test; unrelated
        // bind errors must fail loudly instead of silently skipping the setup.
        let listener = match std::net::TcpListener::bind(("0.0.0.0", QUICK_SHARE_PORT)) {
            Ok(listener) => Some(listener),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => None,
            Err(error) => panic!("failed to reserve fixed share port: {error}"),
        };
        let result = bind_server();
        let error = match result {
            Ok(_) => panic!("occupied fixed port must fail"),
            Err(error) => error,
        };
        assert!(
            error.starts_with("WUI_SHARE_PORT_IN_USE|"),
            "unexpected error: {error}"
        );
        drop(listener);
    }

    pub(crate) fn bind_test_lock() -> &'static Mutex<()> {
        static LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn session_scoped_stop_cannot_stop_a_replacement_server() {
        let state = ShareServerState::new();
        let control = Arc::new(Control::new());
        *state.inner.lock().unwrap() = Some(Inner {
            info: test_info("session-22"),
            control: control.clone(),
            worker: None,
        });

        stop_server(&state, Some("session-21".into()), true).expect("stale stop is harmless");
        assert!(status(&state).running);
        stop_server(&state, Some("session-22".into()), true).expect("current session stops");
        assert!(!status(&state).running);
    }

    #[test]
    fn stop_before_publish_records_the_pending_session_cancellation() {
        let state = ShareServerState::new();
        stop_server(&state, Some("session-31".into()), true).expect("pending stop is recorded");
        assert!(state
            .cancelled_sessions
            .lock()
            .unwrap()
            .contains_key("session-31"));
        assert!(!status(&state).running);
    }

    #[test]
    fn pending_session_cancellations_are_memory_bounded() {
        let state = ShareServerState::new();
        for index in 0..MAX_CANCELLED_SESSIONS {
            stop_server(&state, Some(format!("pending-{index}")), true)
                .expect("bounded cancellation accepted");
        }

        assert!(stop_server(&state, Some("one-too-many".into()), true).is_err());
        assert_eq!(
            state.cancelled_sessions.lock().unwrap().len(),
            MAX_CANCELLED_SESSIONS
        );

        *state.inner.lock().unwrap() = Some(Inner {
            info: test_info("current-session"),
            control: Arc::new(Control::new()),
            worker: None,
        });
        stop_server(&state, Some("current-session".into()), true)
            .expect("the current server always remains stoppable");
        assert!(!status(&state).running);
    }

    #[test]
    fn stop_reasons_preserve_idle_timeout_semantics() {
        assert_eq!(ServerStopReason::Stopped.as_str(), "stopped");
        assert_eq!(ServerStopReason::IdleTimeout.as_str(), "idle_timeout");
    }

    #[test]
    fn token_path_match_is_exact_but_allows_query_strings() {
        let expected = "/w/abc123";
        assert!(request_path_matches("/w/abc123", expected));
        assert!(request_path_matches("/w/abc123?download=1", expected));
        assert!(!request_path_matches("/w/abc123-extra", expected));
        assert!(!request_path_matches("/w/abc123/child", expected));
    }

    #[test]
    fn guess_mime_for_video_files() {
        assert_eq!(guess_mime("highlight.mp4"), "video/mp4");
        assert_eq!(guess_mime("clip.WebM"), "video/webm");
        assert_eq!(guess_mime("recording.MKV"), "video/x-matroska");
        assert_eq!(guess_mime("unknown.xyz"), "application/octet-stream");
    }

    #[test]
    fn content_disposition_encodes_non_ascii_filename() {
        let header = content_disposition_attachment("击杀集锦.mp4");
        assert!(
            header.contains("filename*=UTF-8''"),
            "expected RFC 5987 filename*: {header}"
        );
        assert!(
            header.contains("%E5%87%BB"),
            "expected percent-encoded Chinese: {header}"
        );
        // ASCII fallback must stay quoted-string safe (no raw non-ascii quotes issues).
        assert!(header.starts_with("attachment; filename=\""));
    }

    #[test]
    fn content_disposition_keeps_ascii_filename() {
        let header = content_disposition_attachment("clip.mp4");
        assert!(header.contains("filename=\"clip.mp4\""));
        assert!(header.contains("filename*=UTF-8''clip.mp4"));
    }

    #[test]
    fn host_header_requires_the_started_address_and_port() {
        let lan_ip = "192.168.1.42".parse().unwrap();
        for accepted in ["192.168.1.42:22357", "localhost:22357", "127.0.0.1:22357"] {
            assert!(
                host_header_matches(accepted, lan_ip, QUICK_SHARE_PORT),
                "{accepted}"
            );
        }
        for rejected in [
            "192.168.1.99:22357",
            "192.168.1.42:80",
            "192.168.1.42:invalid",
            "192.168.1.42:22357:extra",
            "http://192.168.1.42:22357",
            "user@192.168.1.42:22357",
        ] {
            assert!(
                !host_header_matches(rejected, lan_ip, QUICK_SHARE_PORT),
                "{rejected}"
            );
        }
    }
}
