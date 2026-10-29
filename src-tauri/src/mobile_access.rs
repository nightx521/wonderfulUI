//! Opt-in, paired, read-only LAN browsing. Separate listener/lifecycle from Quick Share.
use crate::mobile_library::{self, MediaFile, MediaKind, Snapshot};
use crate::mobile_policy::{self, DeviceInfo, Pairing};
use crate::mobile_trust::{SharedTrust, TrustStore};
use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, State},
    http::{header, Method, Request, Response, StatusCode},
    Extension, Router,
};
use hyper::server::conn::http1;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll},
    thread,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncSeekExt, ReadBuf},
    sync::{Notify, Semaphore},
    task::JoinSet,
};
use tokio_util::io::ReaderStream;

mod assets {
    include!(concat!(env!("OUT_DIR"), "/mobile_assets.rs"));
}
type Loader = Arc<dyn Fn() -> Result<Snapshot, String> + Send + Sync>;
struct Cached {
    body: Bytes,
    media: std::collections::HashMap<String, MediaFile>,
    loaded: Instant,
}
struct Core {
    origin: String,
    ip: Ipv4Addr,
    port: u16,
    pairing: Mutex<Pairing>,
    cached: Mutex<Cached>,
    loader: Loader,
    refresh: tokio::sync::Mutex<()>,
    stopped: AtomicBool,
    shutdown: Notify,
    changed: Arc<dyn Fn(bool) + Send + Sync>,
}
impl Core {
    fn new(
        ip: Ipv4Addr,
        port: u16,
        loader: Loader,
        changed: Arc<dyn Fn(bool) + Send + Sync>,
    ) -> Result<Arc<Self>, String> {
        let snapshot = loader()?;
        Ok(Arc::new(Self {
            origin: format!("http://{ip}:{port}"),
            ip,
            port,
            pairing: Mutex::new(Pairing::default()),
            cached: Mutex::new(Cached {
                body: snapshot.json.into(),
                media: snapshot.media,
                loaded: Instant::now(),
            }),
            loader,
            refresh: tokio::sync::Mutex::new(()),
            stopped: AtomicBool::new(false),
            shutdown: Notify::new(),
            changed,
        }))
    }
    fn close(&self) {
        self.stopped.store(true, Ordering::Release);
        self.pairing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .close();
        self.shutdown.notify_one();
    }
    async fn library(&self) -> Result<Bytes, String> {
        let _guard = self.refresh.lock().await;
        if self
            .cached
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .loaded
            .elapsed()
            >= Duration::from_secs(3)
        {
            let loader = self.loader.clone();
            let snapshot = tokio::task::spawn_blocking(move || loader())
                .await
                .map_err(|_| "资料库暂时无法读取")??;
            *self.cached.lock().unwrap_or_else(|e| e.into_inner()) = Cached {
                body: snapshot.json.into(),
                media: snapshot.media,
                loaded: Instant::now(),
            };
        }
        Ok(self
            .cached
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .body
            .clone())
    }
}
struct Server {
    core: Arc<Core>,
    worker: thread::JoinHandle<()>,
    generation: String,
}
#[derive(Default)]
pub struct MobileAccessState {
    inner: Mutex<Option<Server>>,
    lifecycle: Mutex<()>,
    trust: Mutex<Option<SharedTrust>>,
    restore_error: Mutex<Option<String>>,
}
impl Drop for MobileAccessState {
    fn drop(&mut self) {
        if let Ok(inner) = self.inner.get_mut() {
            if let Some(s) = inner.take() {
                s.core.close();
                let _ = s.worker.join();
            }
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessStatus {
    running: bool,
    generation: Option<String>,
    url: Option<String>,
    qr_svg: Option<String>,
    pending: Vec<DeviceInfo>,
    devices: Vec<DeviceInfo>,
    remember_devices: bool,
    restore_error: Option<String>,
}
impl MobileAccessState {
    fn trust(&self) -> Result<SharedTrust, String> {
        let mut cached = self.trust.lock().map_err(|_| "设备设置暂时不可用")?;
        if let Some(trust) = cached.as_ref() {
            return Ok(trust.clone());
        }
        let trust = Arc::new(Mutex::new(TrustStore::load(
            crate::library::db::library_dir()?.join("mobile-access.json"),
        )?));
        *cached = Some(trust.clone());
        Ok(trust)
    }
    fn status(&self) -> Result<AccessStatus, String> {
        let trust = self.trust()?;
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if guard.as_ref().is_some_and(|s| s.worker.is_finished()) {
            if let Some(server) = guard.take() {
                server.core.close();
                let _ = server.worker.join();
            }
        }
        let Some(server) = guard.as_ref() else {
            let trust = trust.lock().unwrap_or_else(|e| e.into_inner());
            return Ok(AccessStatus {
                running: false,
                generation: None,
                url: None,
                qr_svg: None,
                pending: vec![],
                devices: trust
                    .devices()
                    .iter()
                    .map(|d| DeviceInfo {
                        id: d.id.clone(),
                        name: d.name.clone(),
                        ip: String::new(),
                        connected: false,
                        remembered: true,
                        remaining_seconds: None,
                    })
                    .collect(),
                remember_devices: trust.remember(),
                restore_error: self
                    .restore_error
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone(),
            });
        };
        let mut pairing = server
            .core
            .pairing
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (pending, devices) = pairing.infos();
        let url = format!("{}/#pair={}", server.core.origin, pairing.invite);
        let svg = qrcode::QrCode::new(url.as_bytes())
            .expect("bounded LAN address")
            .render::<qrcode::render::svg::Color>()
            .min_dimensions(220, 220)
            .build();
        let remember_devices = trust.lock().unwrap_or_else(|e| e.into_inner()).remember();
        Ok(AccessStatus {
            running: true,
            generation: Some(server.generation.clone()),
            url: Some(url),
            qr_svg: Some(svg),
            pending,
            devices,
            remember_devices,
            restore_error: None,
        })
    }
    fn stop(&self) -> Result<(), String> {
        let _lifecycle = self.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        self.trust()?
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_enabled(false)?;
        if let Some(server) = self.inner.lock().unwrap_or_else(|e| e.into_inner()).take() {
            server.core.close();
            let _ = server.worker.join();
        }
        *self.restore_error.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(())
    }
    fn replace_server(&self, server: Server) {
        let previous = self
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(server);
        if let Some(previous) = previous {
            previous.core.close();
            let _ = previous.worker.join();
        }
    }
    fn action(&self, generation: &str, id: &str, allow: Option<bool>) -> Result<(), String> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let s = guard
            .as_ref()
            .filter(|s| s.generation == generation)
            .ok_or("手机访问会话已结束")?;
        let mut p = s.core.pairing.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(allow) = allow {
            p.decide(id, allow)?
        } else {
            p.revoke(id)?
        };
        Ok(())
    }
}

#[tauri::command]
pub async fn mobile_access_status(app: tauri::AppHandle) -> Result<AccessStatus, String> {
    crate::run_blocking(move || app.state::<MobileAccessState>().status()).await
}
fn detected_ip() -> Result<Ipv4Addr, String> {
    crate::lan_ip::detect_lan_ipv4()
        .ok_or_else(|| "未检测到可用的局域网，请连接 Wi-Fi 或有线网络".to_string())?
        .parse()
        .map_err(|_| "局域网地址不可用".into())
}
fn prepare_server(
    app: &tauri::AppHandle,
    state: &MobileAccessState,
    ip: Ipv4Addr,
) -> Result<Server, String> {
    let listener = TcpListener::bind((ip, mobile_policy::MOBILE_PORT))
        .map_err(|_| "手机访问端口 22358 无法使用，可能被其它程序占用")?;
    listener
        .set_nonblocking(true)
        .map_err(|_| "手机访问服务无法开启")?;
    crate::firewall::ensure_mobile_ready()
        .map_err(|_| "手机访问防火墙授权未完成，请允许系统授权后重试")?;
    let salt = mobile_policy::random_token();
    let loader: Loader = Arc::new(move || mobile_library::load(&salt));
    let callback = app.clone();
    let core = Core::new(
        ip,
        mobile_policy::MOBILE_PORT,
        loader,
        Arc::new(move |pair_request| {
            let _ = callback.emit(
                "wui://mobile_access_changed",
                json!({"pairRequest":pair_request}),
            );
        }),
    )?;
    core.pairing.lock().unwrap_or_else(|e| e.into_inner()).trust = state.trust()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|_| "手机访问服务无法开启")?;
    let worker_core = core.clone();
    let worker = thread::spawn(move || runtime.block_on(serve(listener, worker_core)));
    Ok(Server {
        core,
        worker,
        generation: mobile_policy::random_token(),
    })
}
#[tauri::command]
pub async fn start_mobile_access(app: tauri::AppHandle) -> Result<AccessStatus, String> {
    crate::run_blocking(move || {
        let state = app.state::<MobileAccessState>();
        let _lifecycle = state.lifecycle.lock().map_err(|_| "手机访问暂时无法开启")?;
        if state.status()?.running {
            return state.status();
        }
        let server = prepare_server(&app, &state, detected_ip()?)?;
        if let Err(e) = state
            .trust()?
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_enabled(true)
        {
            server.core.close();
            let _ = server.worker.join();
            return Err(e);
        }
        *state.inner.lock().map_err(|_| "手机访问暂时无法开启")? = Some(server);
        *state
            .restore_error
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        state.status()
    })
    .await
}

/// Restore only a previously enabled service; failures remain visible in settings.
pub fn restore_on_start(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let enabled = app
            .state::<MobileAccessState>()
            .trust()
            .map(|t| t.lock().unwrap_or_else(|e| e.into_inner()).enabled());
        let result = match enabled {
            Ok(true) => start_mobile_access(app.clone()).await.map(|_| ()),
            Ok(false) => Ok(()),
            Err(e) => Err(e),
        };
        if let Err(error) = result {
            *app.state::<MobileAccessState>()
                .restore_error
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(error);
        }
        let _ = app.emit("wui://mobile_access_changed", json!({"pairRequest":false}));
    });
}

#[tauri::command]
pub async fn set_mobile_remember(
    app: tauri::AppHandle,
    remember: bool,
) -> Result<AccessStatus, String> {
    crate::run_blocking(move || {
        let state = app.state::<MobileAccessState>();
        {
            let guard = state.inner.lock().map_err(|_| "手机访问暂时不可用")?;
            if let Some(server) = guard.as_ref() {
                server
                    .core
                    .pairing
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .configure_remember(remember)?;
            } else {
                let trust = state.trust()?;
                let mut trust = trust.lock().unwrap_or_else(|e| e.into_inner());
                let devices = trust.devices().to_vec();
                trust.configure(remember, devices)?;
            }
        }
        state.status()
    })
    .await
}
#[tauri::command]
pub async fn stop_mobile_access(app: tauri::AppHandle) -> Result<AccessStatus, String> {
    crate::run_blocking(move || {
        let s = app.state::<MobileAccessState>();
        s.stop()?;
        *s.restore_error.lock().unwrap_or_else(|e| e.into_inner()) = None;
        s.status()
    })
    .await
}
#[tauri::command]
pub async fn decide_mobile_device(
    app: tauri::AppHandle,
    generation: String,
    id: String,
    allow: bool,
) -> Result<AccessStatus, String> {
    crate::run_blocking(move || {
        let s = app.state::<MobileAccessState>();
        s.action(&generation, &id, Some(allow))?;
        s.status()
    })
    .await
}
#[tauri::command]
pub async fn revoke_mobile_device(
    app: tauri::AppHandle,
    generation: Option<String>,
    id: String,
) -> Result<AccessStatus, String> {
    crate::run_blocking(move || {
        let s = app.state::<MobileAccessState>();
        if s.status()?.running {
            s.action(
                generation.as_deref().ok_or("手机访问会话已结束")?,
                &id,
                None,
            )?;
        } else {
            s.trust()?
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .revoke(&id)?;
        }
        s.status()
    })
    .await
}
#[tauri::command]
pub async fn refresh_mobile_invite(app: tauri::AppHandle) -> Result<AccessStatus, String> {
    crate::run_blocking(move || {
        let state = app.state::<MobileAccessState>();
        let _lifecycle = state.lifecycle.lock().map_err(|_| "手机访问暂时不可用")?;
        let ip = detected_ip()?;
        let changed = state
            .inner
            .lock()
            .map_err(|_| "手机访问暂时不可用")?
            .as_ref()
            .is_some_and(|s| s.core.ip != ip);
        if changed {
            // Prepare the new listener first; a failure keeps the existing service intact.
            let server = prepare_server(&app, &state, ip)?;
            state.replace_server(server);
        } else if let Some(server) = state
            .inner
            .lock()
            .map_err(|_| "手机访问暂时不可用")?
            .as_ref()
        {
            server
                .core
                .pairing
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .rotate();
        }
        state.status()
    })
    .await
}

async fn serve(listener: TcpListener, core: Arc<Core>) {
    let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
        core.close();
        return;
    };
    let router = Router::new()
        .fallback(handle)
        .layer(DefaultBodyLimit::max(2048))
        .with_state(core.clone());
    let slots = Arc::new(Semaphore::new(32));
    let mut workers = JoinSet::new();
    loop {
        tokio::select! {
            _=core.shutdown.notified()=>break,
            Some(_)=workers.join_next(),if !workers.is_empty()=>{},
            connection=listener.accept()=>{
                let Ok((stream,peer))=connection else{tokio::time::sleep(Duration::from_millis(50)).await;continue};
                if !peer.ip().is_loopback()&&!matches!(peer.ip(),IpAddr::V4(ip)if crate::lan_ip::is_lan_ipv4(&ip)){continue}
                let Ok(permit)=slots.clone().try_acquire_owned()else{continue};
                let service=TowerToHyperService::new(router.clone().layer(Extension(peer)));
                workers.spawn(async move{let _permit=permit;let _=http1::Builder::new().timer(TokioTimer::new()).header_read_timeout(Duration::from_secs(5)).max_headers(64).max_buf_size(16*1024).keep_alive(false).serve_connection(TokioIo::new(stream),service).await;});
            }
        }
    }
    core.close();
    drop(listener);
    workers.abort_all();
    while workers.join_next().await.is_some() {}
}

fn response(status: StatusCode, body: impl Into<Body>, mime: &'static str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        .body(body.into())
        .expect("fixed headers")
}
fn message(status: StatusCode, text: &str) -> Response<Body> {
    response(
        status,
        json!({"message":text}).to_string(),
        "application/json; charset=utf-8",
    )
}
fn harden(mut reply: Response<Body>) -> Response<Body> {
    let h = reply.headers_mut();
    h.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    h.insert("x-content-type-options", "nosniff".parse().unwrap());
    h.insert("referrer-policy", "no-referrer".parse().unwrap());
    h.insert("x-frame-options", "DENY".parse().unwrap());
    h.insert("content-security-policy","default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; media-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'".parse().unwrap());
    reply
}
fn cookie_token(headers: &axum::http::HeaderMap) -> Option<&str> {
    let mut found = None;
    for line in headers.get_all(header::COOKIE) {
        for pair in line.to_str().ok()?.split(';') {
            let Some((name, value)) = pair.trim().split_once('=') else {
                continue;
            };
            if name == mobile_policy::COOKIE {
                if found.is_some() {
                    return None;
                }
                found = Some(value)
            }
        }
    }
    found
}
#[derive(Deserialize)]
struct PairRequest {
    invite: String,
    name: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairPoll {
    request_id: String,
    request_token: String,
}
async fn handle(
    State(core): State<Arc<Core>>,
    Extension(peer): Extension<SocketAddr>,
    request: Request<Body>,
) -> Response<Body> {
    harden(route(core, peer, request).await)
}
async fn route(core: Arc<Core>, peer: SocketAddr, request: Request<Body>) -> Response<Body> {
    if core.stopped.load(Ordering::Acquire) {
        return message(StatusCode::GONE, "手机访问已关闭");
    }
    let headers = request.headers();
    let path = request.uri().path().to_string();
    let method = request.method().clone();
    let private_route =
        path.starts_with("/api/") || path.starts_with("/media/") || method == Method::POST;
    if headers.get_all(header::HOST).iter().count() != 1
        || !headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .is_some_and(|h| crate::share_server::host_header_matches(h, core.ip, core.port))
    {
        return message(StatusCode::FORBIDDEN, "访问地址不正确，请重新扫码");
    }
    if headers
        .get(header::ORIGIN)
        .is_some_and(|h| h.to_str().ok() != Some(core.origin.as_str()))
        || private_route
            && headers
                .get("sec-fetch-site")
                .is_some_and(|h| h == "cross-site")
    {
        return message(StatusCode::FORBIDDEN, "请从手机访问页面打开");
    }
    if path == "/api/pair" || path == "/api/pair/status" {
        if method != Method::POST
            || headers.get(header::ORIGIN).and_then(|h| h.to_str().ok()) != Some(&core.origin)
        {
            return message(StatusCode::FORBIDDEN, "连接请求无效");
        }
        let body = match tokio::time::timeout(
            Duration::from_secs(5),
            axum::body::to_bytes(request.into_body(), 2048),
        )
        .await
        {
            Ok(Ok(body)) => body,
            _ => return message(StatusCode::BAD_REQUEST, "连接请求过大或已超时"),
        };
        if path == "/api/pair" {
            let Ok(input) = serde_json::from_slice::<PairRequest>(&body) else {
                return message(StatusCode::BAD_REQUEST, "连接请求无效");
            };
            let result = core
                .pairing
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .begin(&input.invite, &input.name, peer.ip());
            return match result {
                Ok((id, secret)) => {
                    (core.changed)(true);
                    response(
                        StatusCode::OK,
                        json!({"requestId":id,"requestToken":secret}).to_string(),
                        "application/json",
                    )
                }
                Err(e) => message(StatusCode::FORBIDDEN, e),
            };
        }
        let Ok(input) = serde_json::from_slice::<PairPoll>(&body) else {
            return message(StatusCode::BAD_REQUEST, "连接请求无效");
        };
        let result = core.pairing.lock().unwrap_or_else(|e| e.into_inner()).poll(
            &input.request_id,
            &input.request_token,
            peer.ip(),
        );
        return match result {
            Ok(None) => response(
                StatusCode::OK,
                "{\"status\":\"pending\"}",
                "application/json",
            ),
            Ok(Some(token)) => {
                let mut r = response(
                    StatusCode::OK,
                    "{\"status\":\"approved\"}",
                    "application/json",
                );
                r.headers_mut().insert(
                    header::SET_COOKIE,
                    format!(
                        "{}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
                        mobile_policy::COOKIE,
                        if core
                            .pairing
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .trust
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .remember()
                        {
                            31536000
                        } else {
                            43200
                        }
                    )
                    .parse()
                    .unwrap(),
                );
                r
            }
            Err(e) => message(StatusCode::FORBIDDEN, e),
        };
    }
    if path.starts_with("/api/") || path.starts_with("/media/") {
        if method != Method::GET && method != Method::HEAD {
            return message(StatusCode::METHOD_NOT_ALLOWED, "仅支持读取资料");
        }
        let grant = cookie_token(headers).and_then(|token| {
            core.pairing
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .authorize(token, peer.ip())
        });
        let Some(grant) = grant else {
            return message(
                StatusCode::UNAUTHORIZED,
                "连接已失效，请重新扫码并在电脑确认",
            );
        };
        if path == "/api/library" && method == Method::GET {
            return match core.library().await {
                Ok(body) => response(StatusCode::OK, body, "application/json; charset=utf-8"),
                Err(e) => message(StatusCode::SERVICE_UNAVAILABLE, &e),
            };
        }
        if path == "/api/session" && method == Method::GET {
            let mut reply = response(StatusCode::OK, "{\"connected\":true}", "application/json");
            if core
                .pairing
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .trust
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remember()
            {
                if let Some(token) = cookie_token(headers) {
                    reply.headers_mut().insert(
                        header::SET_COOKIE,
                        format!(
                            "{}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=31536000",
                            mobile_policy::COOKIE
                        )
                        .parse()
                        .unwrap(),
                    );
                }
            }
            return reply;
        }
        if let Some(id) = path.strip_prefix("/media/") {
            if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                return message(StatusCode::NOT_FOUND, "视频不存在");
            }
            let file = core
                .cached
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .media
                .get(id)
                .cloned();
            let Some(file) = file else {
                return message(StatusCode::NOT_FOUND, "视频不存在或已移除");
            };
            return media_response(file, grant, request).await;
        }
        return message(StatusCode::NOT_FOUND, "页面不存在");
    }
    if method != Method::GET && method != Method::HEAD {
        return message(StatusCode::METHOD_NOT_ALLOWED, "不支持此操作");
    }
    if let Some((bytes, mime)) = assets::asset(&path) {
        return response(
            StatusCode::OK,
            if method == Method::HEAD {
                Body::empty()
            } else {
                Body::from(bytes)
            },
            mime,
        );
    }
    message(StatusCode::NOT_FOUND, "页面不存在")
}
struct RevocableRead {
    inner: tokio::io::Take<tokio::fs::File>,
    grant: mobile_policy::Grant,
}
impl AsyncRead for RevocableRead {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if !self.grant.active.load(Ordering::Acquire) || Instant::now() >= self.grant.until {
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "session closed",
            )));
        }
        Pin::new(&mut self.inner).poll_read(cx, buffer)
    }
}
async fn media_response(
    file: MediaFile,
    grant: mobile_policy::Grant,
    request: Request<Body>,
) -> Response<Body> {
    let mime = match file
        .path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "video/mp4",
    };
    let opened = tokio::task::spawn_blocking(move || {
        if !mobile_library::allowed_media(&file) {
            return Err(());
        }
        let f = std::fs::File::open(&file.path).map_err(|_| ())?;
        let metadata = f.metadata().map_err(|_| ())?;
        if matches!(file.kind, MediaKind::Poster) && metadata.len() > 16 * 1024 * 1024 {
            return Err(());
        }
        Ok((f, metadata.len()))
    })
    .await;
    let Ok(Ok((file, length))) = opened else {
        return message(StatusCode::NOT_FOUND, "文件未找到，请在电脑上检查视频位置");
    };
    let range_headers = request.headers().get_all(header::RANGE);
    let first_range = range_headers.iter().next();
    let raw = first_range.and_then(|h| h.to_str().ok());
    if range_headers.iter().count() > 1 || first_range.is_some() && raw.is_none() {
        return message(StatusCode::RANGE_NOT_SATISFIABLE, "播放位置无效");
    }
    let Ok((start, count, partial)) = mobile_policy::byte_range(raw, length) else {
        let mut r = message(StatusCode::RANGE_NOT_SATISFIABLE, "播放位置超出视频范围");
        r.headers_mut().insert(
            header::CONTENT_RANGE,
            format!("bytes */{length}").parse().unwrap(),
        );
        return r;
    };
    let mut file = tokio::fs::File::from_std(file);
    if start > 0 && file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return message(StatusCode::SERVICE_UNAVAILABLE, "视频暂时无法读取");
    }
    let body = if request.method() == Method::HEAD {
        Body::empty()
    } else {
        Body::from_stream(ReaderStream::with_capacity(
            RevocableRead {
                inner: file.take(count),
                grant,
            },
            64 * 1024,
        ))
    };
    let mut r = response(
        if partial {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        },
        body,
        mime,
    );
    r.headers_mut()
        .insert(header::ACCEPT_RANGES, "bytes".parse().unwrap());
    r.headers_mut()
        .insert(header::CONTENT_LENGTH, count.to_string().parse().unwrap());
    if partial {
        r.headers_mut().insert(
            header::CONTENT_RANGE,
            format!("bytes {start}-{}/{length}", start + count - 1)
                .parse()
                .unwrap(),
        );
    }
    r
}

#[cfg(test)]
mod tests;
