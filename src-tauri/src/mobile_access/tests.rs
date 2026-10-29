use super::*;
use std::io::Read;

struct Fixture {
    core: Arc<Core>,
    worker: Option<thread::JoinHandle<()>>,
    root: std::path::PathBuf,
    port: u16,
    view: Arc<Mutex<serde_json::Value>>,
}
impl Fixture {
    fn start(port: u16) -> Self {
        let root = std::env::temp_dir().join(format!("wui-mobile-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("fixture.mp4");
        std::fs::write(
            &path,
            (0..262144).map(|n| (n % 251) as u8).collect::<Vec<_>>(),
        )
        .unwrap();
        let value = json!({"dir":"private-path","accounts":[{"openid":"123","path":"private-path","nick":"Fixture","matchCount":1}],"matches":[{"matches_id":"fixture-match","openID":"123","map":{"map_id":"/Game/Maps/Ascent/Ascent"},"agent":{"agent_name":"Jett"},"gameStartTime":"2026-06-20 12:00:00","matches_time":1781956800000i64,"stats":{"kills":12,"deaths":5,"assists":3,"has_won":true,"rounds_won":13,"rounds_lost":5},"videos":[{"video_id":"fixture-video","video_src":path,"video_type":"击杀集锦"}]}],"totalErrors":0});
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let view = Arc::new(Mutex::new(value));
        let loader_view = view.clone();
        let core = Core::new(
            Ipv4Addr::LOCALHOST,
            port,
            Arc::new(move || {
                mobile_library::project(
                    serde_json::from_value(loader_view.lock().unwrap().clone()).unwrap(),
                    "fixture-salt",
                )
            }),
            Arc::new(|_| {}),
        )
        .unwrap();
        let run = core.clone();
        let worker = thread::spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap()
                .block_on(serve(listener, run))
        });
        Self {
            core,
            worker: Some(worker),
            root,
            port,
            view,
        }
    }
    fn request(
        &self,
        method: &str,
        path: &str,
        cookie: Option<&str>,
        body: Option<serde_json::Value>,
        extra: Option<(&str, &str)>,
    ) -> ureq::Response {
        let mut r = ureq::request(method, &format!("{}{}", self.core.origin, path))
            .timeout(Duration::from_secs(5));
        if let Some(c) = cookie {
            r = r.set("Cookie", c)
        }
        if let Some((k, v)) = extra {
            r = r.set(k, v)
        }
        let result = if let Some(b) = body {
            r.set("Origin", &self.core.origin)
                .set("Content-Type", "application/json")
                .send_string(&b.to_string())
        } else {
            r.call()
        };
        match result {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("HTTP request failed: {e}"),
        }
    }
    fn paired(&self) -> (String, String) {
        let invite = self.core.pairing.lock().unwrap().invite.clone();
        let r = self.request(
            "POST",
            "/api/pair",
            None,
            Some(json!({"invite":invite,"name":"Fixture phone"})),
            None,
        );
        assert_eq!(r.status(), 200);
        let p: serde_json::Value = serde_json::from_str(&r.into_string().unwrap()).unwrap();
        let body = json!({"requestId":p["requestId"],"requestToken":p["requestToken"]});
        let waiting = self.request("POST", "/api/pair/status", None, Some(body.clone()), None);
        assert!(waiting.header("set-cookie").is_none());
        assert!(waiting.into_string().unwrap().contains("pending"));
        let id = p["requestId"].as_str().unwrap().to_string();
        self.core.pairing.lock().unwrap().decide(&id, true).unwrap();
        let accepted = self.request("POST", "/api/pair/status", None, Some(body), None);
        let cookie = accepted.header("set-cookie").unwrap();
        assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
        (id, cookie.split(';').next().unwrap().to_string())
    }
}

#[test]
#[ignore = "local browser QA only; requires an explicit workspace fixture directory"]
fn browser_fixture_for_mobile_qa() {
    let root = std::fs::canonicalize(
        std::env::var("WUI_MOBILE_BROWSER_ROOT").expect("explicit QA directory required"),
    )
    .unwrap();
    let project = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    assert!(root.starts_with(std::fs::canonicalize(project.join("output/playwright")).unwrap()));
    let f = Fixture::start(0);
    if root.join("sample.mp4").is_file() {
        std::fs::copy(root.join("sample.mp4"), f.root.join("fixture.mp4")).unwrap();
    }
    {
        let mut value = f.view.lock().unwrap();
        value["accounts"][0]["achievements"] =
            json!([{"matchesId":"fixture-match","achvType":"mvp","typeStr":"MVP"}]);
        value["accounts"][0]["matchCount"] = json!(2);
        value["accounts"].as_array_mut().unwrap().push(json!({"openid":"456","path":"private-path","nick":"Fixture B","tag":"002","matchCount":1}));
        value["matches"][0]["tier"] = json!({"tier_before":10,"tier_after":11});
        value["matches"][0]["mode"] = json!("competitive");
        let mut fourth = value["matches"][0]["videos"][0].clone();
        fourth["video_id"] = json!("fourth-video");
        fourth["video_type"] = json!("四杀时刻");
        value["matches"][0]["videos"]
            .as_array_mut()
            .unwrap()
            .push(fourth);
        let mut second = value["matches"][0].clone();
        second["matches_id"] = json!("match-two");
        second["openID"] = json!("456");
        second["agent"] = json!({"agent_name":"Sova"});
        second["map"] = json!({"map_id":"/Game/Maps/Duality/Duality"});
        second["stats"] = json!({"kills":9,"deaths":15,"assists":5,"has_won":false,"rounds_won":1,"rounds_lost":5});
        second["mode"] = json!("swiftplay");
        second["matches_time"] = json!(1781870400000i64);
        second["videos"][0]["video_id"] = json!("second-video");
        second["videos"].as_array_mut().unwrap().truncate(1);
        let mut third = second.clone();
        third["matches_id"] = json!("match-three");
        third["openID"] = json!("123");
        third["mode"] = json!("customgame");
        third["agent"] = json!({"agent_name":"Sage"});
        third["stats"] = json!({"kills":20,"deaths":20,"assists":10,"has_won":false,"rounds_won":14,"rounds_lost":14});
        third["videos"][0]["video_type"] = json!("死亡集锦");
        third["videos"][0]["video_id"] = json!("third-video");
        third["matches_time"] = json!(1781784000000i64);
        value["matches"]
            .as_array_mut()
            .unwrap()
            .extend([second, third]);
    }
    f.core.cached.lock().unwrap().loaded = Instant::now() - Duration::from_secs(5);
    let deadline = Instant::now() + Duration::from_secs(30 * 60);
    println!(
        "Browser QA fixture running on loopback; credentials remain in the ignored QA directory."
    );
    while Instant::now() < deadline {
        let (pending, devices, invite) = {
            let mut p = f.core.pairing.lock().unwrap();
            let (pending, devices) = p.infos();
            (pending, devices, p.invite.clone())
        };
        let url = format!("{}/#pair={invite}", f.core.origin);
        let svg = qrcode::QrCode::new(url.as_bytes())
            .unwrap()
            .render::<qrcode::render::svg::Color>()
            .min_dimensions(220, 220)
            .build();
        std::fs::write(root.join("state.json"),json!({"url":url,"origin":f.core.origin,"pending":pending,"devices":devices,"qrSvg":svg}).to_string()).unwrap();
        if let Ok(text) = std::fs::read_to_string(root.join("control.json")) {
            if let Ok(command) = serde_json::from_str::<serde_json::Value>(&text) {
                match command["action"].as_str() {
                    Some("approveFirst") => {
                        if let Some(p) = pending.first() {
                            f.core.pairing.lock().unwrap().decide(&p.id, true).unwrap();
                        }
                    }
                    Some("revokeAll") => {
                        for d in &devices {
                            f.core.pairing.lock().unwrap().revoke(&d.id);
                        }
                    }
                    Some("rotate") => f.core.pairing.lock().unwrap().rotate(),
                    Some("stop") => break,
                    _ => {}
                }
                std::fs::remove_file(root.join("control.json")).unwrap();
            }
        }
        thread::sleep(Duration::from_millis(200));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.core.close();
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap()
        }
        assert!(
            self.root.starts_with(std::env::temp_dir())
                && self
                    .root
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("wui-mobile-test-")
        );
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn actual_http_requires_approval_authorizes_only_registered_media_and_handles_seek() {
    let f = Fixture::start(0);
    assert_eq!(
        f.request("GET", "/api/library", None, None, None).status(),
        401
    );
    assert_eq!(
        f.request("GET", "/api/library", Some("wui_mobile=guess"), None, None)
            .status(),
        401
    );
    let (id, cookie) = f.paired();
    let response = f.request("GET", "/api/library", Some(&cookie), None, None);
    assert_eq!(response.header("Cache-Control"), Some("no-store"));
    let text = response.into_string().unwrap();
    assert!(!text.contains("private-path") && !text.contains(f.root.to_str().unwrap()));
    let library: serde_json::Value = serde_json::from_str(&text).unwrap();
    let video = library["matches"][0]["videos"][0]["video_src"]
        .as_str()
        .unwrap();
    let head = f.request("HEAD", video, Some(&cookie), None, None);
    assert_eq!(head.status(), 200);
    assert_eq!(head.header("Content-Length"), Some("262144"));
    assert_eq!(head.header("Accept-Ranges"), Some("bytes"));
    let partial = f.request(
        "GET",
        video,
        Some(&cookie),
        None,
        Some(("Range", "bytes=10-19")),
    );
    assert_eq!(partial.status(), 206);
    assert_eq!(partial.header("Content-Range"), Some("bytes 10-19/262144"));
    let mut bytes = Vec::new();
    partial.into_reader().read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, (10..20).collect::<Vec<u8>>());
    let bad = f.request(
        "GET",
        video,
        Some(&cookie),
        None,
        Some(("Range", "bytes=262144-")),
    );
    assert_eq!(bad.status(), 416);
    assert_eq!(bad.header("Content-Range"), Some("bytes */262144"));
    assert_eq!(
        f.request(
            "GET",
            "/media/not-a-registered-path",
            Some(&cookie),
            None,
            None
        )
        .status(),
        404
    );
    assert_eq!(
        f.request("POST", "/api/library", Some(&cookie), None, None)
            .status(),
        405
    );
    assert_eq!(
        f.request(
            "GET",
            "/api/library",
            Some(&cookie),
            None,
            Some(("Origin", "https://attacker.invalid"))
        )
        .status(),
        403
    );
    assert_eq!(
        f.request(
            "GET",
            "/api/library",
            Some(&cookie),
            None,
            Some(("Host", "attacker.invalid"))
        )
        .status(),
        403
    );
    f.core.pairing.lock().unwrap().revoke(&id);
    assert_eq!(
        f.request("GET", video, Some(&cookie), None, None).status(),
        401
    );
}

#[test]
fn stopping_and_restarting_releases_listener_and_invalidates_existing_cookies() {
    let mut port = 0;
    for _ in 0..12 {
        let f = Fixture::start(port);
        port = f.port;
        let (_, cookie) = f.paired();
        assert_eq!(
            f.request("GET", "/api/session", Some(&cookie), None, None)
                .status(),
            200
        );
        drop(f);
        assert!(std::net::TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            Duration::from_millis(500)
        )
        .is_err());
        let next = Fixture::start(port);
        assert_eq!(
            next.request("GET", "/api/library", Some(&cookie), None, None)
                .status(),
            401
        );
        drop(next);
    }
}

#[test]
fn shutdown_cancels_an_active_video_response() {
    use std::io::Write;
    let mut f = Fixture::start(0);
    std::fs::OpenOptions::new()
        .write(true)
        .open(f.root.join("fixture.mp4"))
        .unwrap()
        .set_len(64 * 1024 * 1024)
        .unwrap();
    let (_, cookie) = f.paired();
    let lib = f
        .request("GET", "/api/library", Some(&cookie), None, None)
        .into_string()
        .unwrap();
    let lib: serde_json::Value = serde_json::from_str(&lib).unwrap();
    let video = lib["matches"][0]["videos"][0]["video_src"]
        .as_str()
        .unwrap();
    let mut client = std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, f.port)).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(
        client,
        "GET {video} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nCookie: {cookie}\r\n\r\n",
        f.port
    )
    .unwrap();
    let mut bytes = [0; 1024];
    assert!(client.read(&mut bytes).unwrap() > 0);
    let started = Instant::now();
    f.core.close();
    f.worker.take().unwrap().join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, f.port)).is_ok());
}
