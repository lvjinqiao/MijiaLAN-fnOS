mod camera;
mod custom;
mod pairing;
mod xiaomi;
use anyhow::{Context, Result, bail};
use custom::{CustomDevice, CustomTransport};
use mijialan_core::{
    Engine,
    cloud::{self, Session},
};
use pairing::DeviceBridges as HomeKit;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Read,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

struct App {
    cameras: Mutex<camera::Cameras>,
    engine: Arc<Engine>,
    bridge: HomeKit,
    token: RwLock<String>,
    custom: Mutex<BTreeMap<String, Arc<CustomTransport>>>,
    session: Arc<RwLock<Option<Session>>>,
    gate: Mutex<()>,
    login: Mutex<Value>,
    cancelled: Mutex<Arc<AtomicBool>>,
    generation: AtomicU64,
    sync_status: Mutex<Value>,
    homes: Mutex<Vec<xiaomi::Home>>,
    cloud_locations: Mutex<BTreeMap<String, (String, String)>>,
    sync_generation: AtomicU64,
    stop: AtomicBool,
}
fn ensure_bridge(app: &App, did: &str) -> Result<()> {
    app.bridge.enable(did)
}
fn main() -> Result<()> {
    if std::env::var_os("MIJIALAN_DATA_DIR").is_none() {
        bail!("请设置 MIJIALAN_DATA_DIR 为独立的应用数据目录");
    }
    let engine = Arc::new(Engine::open()?);
    engine.store.update(|c| c.keep_awake = false)?;
    let token = engine
        .store
        .vault
        .get("daemon:admin-token")?
        .or_else(|| std::env::var("MIJIALAN_ADMIN_TOKEN").ok())
        .unwrap_or_else(|| "12345".into());
    if token.is_empty() {
        bail!("管理密码不能为空");
    }
    engine
        .store
        .vault
        .set("daemon:admin-token", Some(token.clone()))?;
    let saved_session = engine
        .store
        .vault
        .get("daemon:xiaomi-session")?
        .map(|s| serde_json::from_str::<Session>(&s))
        .transpose()?;
    let session = Arc::new(RwLock::new(saved_session));
    let bridge = HomeKit::new(engine.clone());
    let app = Arc::new(App {
        cameras: Mutex::new(camera::Cameras::new()?),
        engine: engine.clone(),
        bridge,
        token: RwLock::new(token),
        custom: Mutex::new(BTreeMap::new()),
        session: session.clone(),
        gate: Mutex::new(()),
        login: Mutex::new(json!({"status":"idle"})),
        cancelled: Mutex::new(Arc::new(AtomicBool::new(false))),
        generation: AtomicU64::new(0),
        sync_status: Mutex::new(json!({"status":"idle"})),
        homes: Mutex::new(Vec::new()),
        cloud_locations: Mutex::new(BTreeMap::new()),
        sync_generation: AtomicU64::new(0),
        stop: AtomicBool::new(false),
    });
    let customs: Vec<CustomDevice> = load(&engine, "daemon:custom")?;
    for mut c in customs {
        c.validate()?;
        if let Some(s) = engine.state(&c.did()) {
            c.bridged = s.device.bridged;
        }
        let t = Arc::new(CustomTransport::new(c.clone()));
        engine.register_transport(c.device(), c.spec(), t.clone())?;
        app.custom.lock().unwrap().insert(c.id.clone(), t);
    }
    if session.read().unwrap().is_some() {
        *app.homes.lock().unwrap() = load(&engine, "daemon:homes")?;
        let records: Vec<xiaomi::CloudDevice> = load(&engine, "daemon:cloud-devices")?;
        for mut record in records {
            if let Some(state) = engine.state(&record.device.did) {
                record.device.bridged = state.device.bridged;
            }
            app.cloud_locations.lock().unwrap().insert(
                record.device.did.clone(),
                (record.home_id.clone(), record.home_name.clone()),
            );
            engine.register_transport(
                record.device.clone(),
                record.spec.clone(),
                Arc::new(xiaomi::CloudTransport::new(record, session.clone())),
            )?;
        }
    } else {
        remove_cloud_devices(&app)?;
    }
    let _poller = engine.start_polling();
    if engine.states().iter().any(|s| s.device.bridged) {
        if let Err(e) = app.bridge.start() {
            eprintln!("HomeKit 启动失败：{e}");
        }
    }
    let worker = app.clone();
    std::thread::spawn(move || {
        let mut elapsed = 0;
        while !worker.stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(1));
            elapsed += 1;
            if elapsed >= 300 {
                elapsed = 0;
                if worker.session.read().unwrap().is_some() {
                    let _ = sync_devices(&worker);
                }
            }
        }
    });
    let signal = app.clone();
    ctrlc::set_handler(move || {
        signal.stop.store(true, Ordering::Relaxed);
        signal.cameras.lock().unwrap().stop();
    })?;
    let address = std::env::var("MIJIALAN_LISTEN").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let server = Arc::new(if let Ok(socket) = std::env::var("MIJIALAN_SOCKET") {
        let server = Server::http_unix(std::path::Path::new(&socket))
            .map_err(|e| anyhow::anyhow!("Socket 监听失败：{e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o666))?;
        }
        server
    } else {
        Server::http(&address).map_err(|e| anyhow::anyhow!("监听失败：{e}"))?
    });
    eprintln!("MijiaLAN fnOS 已启动，开发监听地址 {address}");
    let mut servers = vec![server];
    if let Ok(extra) = std::env::var("MIJIALAN_EXTRA_LISTEN") {
        servers.push(Arc::new(
            Server::http(&extra).map_err(|e| anyhow::anyhow!("API 端口监听失败：{e}"))?,
        ));
    }
    let mut threads = vec![];
    for server in servers {
        for _ in 0..6 {
            let app = app.clone();
            let server = server.clone();
            threads.push(std::thread::spawn(move || {
                while !app.stop.load(Ordering::Relaxed) {
                    match server.recv_timeout(Duration::from_millis(250)) {
                        Ok(Some(request)) => handle(request, &app),
                        Ok(None) => {}
                        Err(_) => break,
                    }
                }
            }));
        }
    }
    for t in threads {
        let _ = t.join();
    }
    app.cancelled.lock().unwrap().store(true, Ordering::Relaxed);
    app.bridge.stop();
    app.cameras.lock().unwrap().stop();
    Ok(())
}
fn load<T: serde::de::DeserializeOwned>(engine: &Engine, key: &str) -> Result<Vec<T>> {
    Ok(engine
        .store
        .vault
        .get(key)?
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_default())
}
fn auth(req: &Request, token: &str) -> bool {
    let expected = format!("Bearer {token}");
    let supplied = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .map(|h| h.value.as_str())
        .unwrap_or("");
    let mut difference = expected.len() ^ supplied.len();
    for (a, b) in expected.bytes().zip(supplied.bytes()) {
        difference |= (a ^ b) as usize;
    }
    difference == 0
}
fn reply(req: Request, status: u16, body: String, content_type: &str) {
    let response=Response::from_string(body).with_status_code(StatusCode(status)).with_header(Header::from_bytes("Content-Type",content_type).unwrap()).with_header(Header::from_bytes("Cache-Control","no-store").unwrap()).with_header(Header::from_bytes("X-Content-Type-Options","nosniff").unwrap()).with_header(Header::from_bytes("Referrer-Policy","no-referrer").unwrap()).with_header(Header::from_bytes("Content-Security-Policy","default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; media-src 'self' blob:; connect-src 'self'; frame-ancestors 'self'").unwrap());
    let _ = req.respond(response);
}
fn handle(mut req: Request, app: &Arc<App>) {
    let raw = req.url().split('?').next().unwrap_or("").to_string();
    let gateway = raw == "/app/mijialan" || raw.starts_with("/app/mijialan/");
    let path = if gateway {
        let path = raw.strip_prefix("/app/mijialan").unwrap();
        if path.is_empty() {
            "/".into()
        } else {
            path.to_string()
        }
    } else {
        raw
    };
    if *req.method() == Method::Get {
        let asset = match path.as_str() {
            "/" => Some((
                include_str!("../web/index.html"),
                "text/html; charset=utf-8",
            )),
            "/app.js" => Some((
                include_str!("../web/app.js"),
                "text/javascript; charset=utf-8",
            )),
            "/style.css" => Some((include_str!("../web/style.css"), "text/css; charset=utf-8")),
            _ => None,
        };
        if let Some((body, kind)) = asset {
            reply(
                req,
                200,
                body.replace("__BASE__", if gateway { "/app/mijialan/" } else { "/" }),
                kind,
            );
            return;
        }
    }
    if !auth(&req, &app.token.read().unwrap()) {
        reply(
            req,
            401,
            json!({"error":"请先输入管理密码"}).to_string(),
            "application/json",
        );
        return;
    }
    let method = req.method().as_str().to_string();
    let mut bytes = Vec::new();
    if req
        .as_reader()
        .take(262145)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() > 262144
    {
        reply(
            req,
            413,
            json!({"error":"请求过大"}).to_string(),
            "application/json",
        );
        return;
    }
    let body = if bytes.is_empty() {
        Ok(Value::Null)
    } else {
        serde_json::from_slice(&bytes)
    };
    if path == "/api/cameras/live" && method == "POST" {
        let stream = body
            .map_err(anyhow::Error::from)
            .and_then(|b| app.cameras.lock().unwrap().live(field(&b, "id")?));
        match stream {
            Ok(stream) => {
                let response = Response::new(
                    StatusCode(200),
                    vec![
                        Header::from_bytes("Content-Type", "video/mp4").unwrap(),
                        Header::from_bytes("Cache-Control", "no-store").unwrap(),
                    ],
                    stream,
                    None,
                    None,
                );
                let _ = req.respond(response);
            }
            Err(error) => reply(
                req,
                400,
                json!({"error":error.to_string()}).to_string(),
                "application/json",
            ),
        }
        return;
    }
    let result = body
        .map_err(anyhow::Error::from)
        .and_then(|body| route(app, &method, &path, body));
    match result {
        Ok(v) => reply(req, 200, v.to_string(), "application/json"),
        Err(e) => reply(
            req,
            400,
            json!({"error":e.to_string()}).to_string(),
            "application/json",
        ),
    }
}
fn route(app: &Arc<App>, method: &str, path: &str, body: Value) -> Result<Value> {
    match (method, path) {
        ("POST", "/api/password") => {
            let _gate = app.gate.lock().unwrap();
            let current = field(&body, "current_password")?;
            let new = field(&body, "new_password")?;
            let confirm = field(&body, "confirm_password")?;
            if current != app.token.read().unwrap().as_str() {
                bail!("当前密码不正确");
            }
            if new.is_empty() || new.len() > 128 {
                bail!("新密码长度应为 1–128 字节");
            }
            if new != confirm {
                bail!("两次输入的新密码不一致");
            }
            app.engine
                .store
                .vault
                .set("daemon:admin-token", Some(new.into()))?;
            *app.token.write().unwrap() = new.into();
            if let Some(root) = app.engine.store.root() {
                let _ = std::fs::remove_file(root.join("admin-token"));
            }
            Ok(json!({"ok":true,"message":"密码已修改，旧密码立即失效"}))
        }
        ("GET", "/api/status") => {
            let devices = app.bridge.info();
            Ok(
                json!({"version":env!("CARGO_PKG_VERSION"),"cloud_logged_in":app.session.read().unwrap().is_some(),"login":*app.login.lock().unwrap(),"sync":*app.sync_status.lock().unwrap(),"homekit":{"devices":devices}}),
            )
        }
        ("GET", "/api/cameras") => app.cameras.lock().unwrap().list(),
        ("POST", "/api/cameras") => app.cameras.lock().unwrap().save(body),
        ("POST", "/api/cameras/pairing") => {
            app.cameras.lock().unwrap().pairing(field(&body, "id")?)
        }
        ("POST", "/api/cameras/delete") => app.cameras.lock().unwrap().delete(field(&body, "id")?),
        ("POST", "/api/cameras/snapshot") => {
            app.cameras.lock().unwrap().snapshot(field(&body, "id")?)
        }
        ("POST", "/api/cameras/reset") => {
            if body["confirm"] != true {
                bail!("请确认重置此摄像头的配对");
            }
            app.cameras.lock().unwrap().reset(field(&body, "id")?)
        }
        ("GET", "/api/homes") => Ok(json!(*app.homes.lock().unwrap())),
        ("GET", "/api/devices") => {
            let logged_in = app.session.read().unwrap().is_some();
            let locations = app.cloud_locations.lock().unwrap();
            let devices = app
                .engine
                .states()
                .into_iter()
                .filter(|s| logged_in || s.device.did.starts_with("custom:"))
                .map(|state| {
                    let location = locations.get(&state.device.did);
                    let mut value = serde_json::to_value(&state).unwrap();
                    value["home_id"] = json!(location.map(|x| x.0.as_str()).unwrap_or(""));
                    value["home_name"] = json!(location.map(|x| x.1.as_str()).unwrap_or(""));
                    value
                })
                .collect::<Vec<_>>();
            Ok(json!(devices))
        }
        ("GET", "/api/custom") => Ok(json!(
            app.custom
                .lock()
                .unwrap()
                .values()
                .map(|t| t.config.clone())
                .collect::<Vec<_>>()
        )),
        ("POST", "/api/custom") => {
            let config: CustomDevice = serde_json::from_value(body)?;
            config.validate()?;
            let _gate = app.gate.lock().unwrap();
            let mut customs = app.custom.lock().unwrap();
            let mut configs = customs
                .values()
                .map(|t| t.config.clone())
                .filter(|c| c.id != config.id)
                .collect::<Vec<_>>();
            configs.push(config.clone());
            app.engine
                .store
                .vault
                .set("daemon:custom", Some(serde_json::to_string(&configs)?))?;
            let t = Arc::new(CustomTransport::new(config.clone()));
            app.engine
                .register_transport(config.device(), config.spec(), t.clone())?;
            customs.insert(config.id.clone(), t);
            if config.bridged {
                ensure_bridge(app, &config.did())?;
            } else {
                app.bridge.disable(&config.did());
            }
            Ok(json!({"ok":true,"did":config.did()}))
        }
        ("POST", "/api/custom/delete") => {
            let id = field(&body, "id")?;
            let _gate = app.gate.lock().unwrap();
            let mut customs = app.custom.lock().unwrap();
            if !customs.contains_key(id) {
                bail!("设备不存在");
            }
            let configs = customs
                .values()
                .filter(|t| t.config.id != id)
                .map(|t| t.config.clone())
                .collect::<Vec<_>>();
            app.engine
                .store
                .vault
                .set("daemon:custom", Some(serde_json::to_string(&configs)?))?;
            app.engine.remove(&format!("custom:{id}"))?;
            customs.remove(id);
            app.bridge.disable(&format!("custom:{id}"));
            Ok(json!({"ok":true}))
        }
        ("POST", "/api/custom/report") => {
            let id = field(&body, "id")?;
            let t = app
                .custom
                .lock()
                .unwrap()
                .get(id)
                .cloned()
                .context("设备不存在")?;
            if !matches!(t.config.probe, custom::Probe::Report) {
                bail!("该设备未使用主动上报状态模式");
            }
            let value = body.get("value").cloned().context("缺少 value")?;
            t.report(value.clone())?;
            app.engine
                .report(&t.config.did(), BTreeMap::from([("2.1".into(), value)]))?;
            Ok(json!({"ok":true}))
        }
        ("POST", "/api/device/write") => {
            let did = field(&body, "did")?;
            let key = field(&body, "key")?;
            let value = body.get("value").cloned().context("缺少 value")?;
            app.engine.write(did, key, value.clone())?;
            let engine = app.engine.clone();
            let did = did.to_owned();
            let key = key.to_owned();
            std::thread::spawn(move || {
                for delay in [0, 300, 900, 1800] {
                    std::thread::sleep(Duration::from_millis(delay));
                    if engine.state(&did).is_none() {
                        break;
                    }
                    if let Ok(state) = engine.refresh(&did) {
                        if state.values.get(&key) == Some(&value) {
                            break;
                        }
                    }
                }
            });
            Ok(json!({"ok":true,"message":"指令已发送，正在确认设备状态"}))
        }
        ("POST", "/api/device/state") => Ok(json!(
            app.engine
                .state(field(&body, "did")?)
                .context("设备不存在")?
        )),
        ("POST", "/api/device/refresh") => Ok(json!(app.engine.refresh(field(&body, "did")?)?)),
        ("POST", "/api/device/bridge") => {
            let did = field(&body, "did")?;
            let _gate = app.gate.lock().unwrap();
            if app.engine.state(did).is_none() {
                bail!("设备不存在");
            }
            let enabled = body["enabled"].as_bool().context("enabled 必须为布尔值")?;
            let previous = app.engine.state(did).unwrap().device.bridged;
            app.engine.set_bridged(did, enabled)?;
            if enabled {
                if let Err(error) = ensure_bridge(app, did) {
                    app.engine.set_bridged(did, previous)?;
                    return Err(error);
                }
            } else {
                app.bridge.disable(did);
            }
            Ok(json!({"ok":true}))
        }
        ("POST", "/api/homekit") => bail!("请在设备卡片中逐台开启 HomeKit；不再提供整组配对"),
        ("POST", "/api/homekit/reset") => {
            if body["confirm"].as_bool() != Some(true) {
                bail!("需要确认清除配对");
            }
            app.bridge.reset_pairing(field(&body, "did")?)?;
            Ok(json!({"ok":true}))
        }
        ("POST", "/api/xiaomi/login") => {
            let region = body["region"].as_str().unwrap_or("cn").to_string();
            if !cloud::regions().contains(&region.as_str()) {
                bail!("未知地区");
            }
            let _gate = app.gate.lock().unwrap();
            if app.session.read().unwrap().is_some() {
                bail!("已登录米家，请先退出后再切换账号");
            }
            app.cancelled.lock().unwrap().store(true, Ordering::Relaxed);
            let cancel = Arc::new(AtomicBool::new(false));
            *app.cancelled.lock().unwrap() = cancel.clone();
            let generation = app.generation.fetch_add(1, Ordering::SeqCst) + 1;
            let ticket = cloud::start()?;
            let response = json!({"status":"waiting","qr_svg":qr(&ticket.login_url)?});
            *app.login.lock().unwrap() = response.clone();
            let worker = app.clone();
            std::thread::spawn(move || {
                let result = cloud::wait(&ticket, &region, &cancel);
                let _guard = worker.gate.lock().unwrap();
                if worker.generation.load(Ordering::SeqCst) != generation {
                    return;
                }
                match result {
                    Ok(session) => {
                        let saved = (|| -> Result<()> {
                            let owner = session.account_key();
                            if worker
                                .engine
                                .store
                                .vault
                                .get("daemon:cloud-owner")?
                                .as_deref()
                                != Some(&owner)
                            {
                                worker
                                    .engine
                                    .store
                                    .vault
                                    .set("daemon:cloud-devices", None)?;
                                worker.engine.store.vault.set("daemon:homes", None)?;
                            }
                            worker
                                .engine
                                .store
                                .vault
                                .set("daemon:cloud-owner", Some(owner))?;
                            worker.engine.store.vault.set(
                                "daemon:xiaomi-session",
                                Some(serde_json::to_string(&session)?),
                            )?;
                            Ok(())
                        })();
                        match saved {
                            Ok(()) => {
                                *worker.session.write().unwrap() = Some(session);
                                *worker.login.lock().unwrap() = json!({"status":"success"});
                                *worker.sync_status.lock().unwrap() = json!({"status":"running"});
                            }
                            Err(e) => {
                                *worker.login.lock().unwrap() =
                                    json!({"status":"error","error":e.to_string()});
                            }
                        }
                    }
                    Err(e) => {
                        *worker.login.lock().unwrap() =
                            json!({"status":"error","error":e.to_string()});
                    }
                }
                drop(_guard);
                if worker.generation.load(Ordering::SeqCst) == generation {
                    let _ = sync_devices(&worker);
                }
            });
            Ok(response)
        }
        ("POST", "/api/xiaomi/logout") => {
            let _gate = app.gate.lock().unwrap();
            app.generation.fetch_add(1, Ordering::SeqCst);
            app.cancelled.lock().unwrap().store(true, Ordering::Relaxed);
            app.engine.store.vault.set("daemon:xiaomi-session", None)?;
            *app.session.write().unwrap() = None;
            remove_cloud_devices(app)?;
            *app.login.lock().unwrap() = json!({"status":"idle"});
            *app.sync_status.lock().unwrap() = json!({"status":"idle"});
            Ok(json!({"ok":true,"message":"已退出米家并移除当前米家设备，自定义设备不受影响"}))
        }
        ("POST", "/api/xiaomi/sync") => {
            if app.session.read().unwrap().is_none() {
                bail!("请先登录米家");
            }
            let worker = app.clone();
            std::thread::spawn(move || {
                let _ = sync_devices(&worker);
            });
            Ok(json!({"ok":true,"message":"正在同步，请查看同步状态"}))
        }
        _ => bail!("未知接口或请求方法"),
    }
}
fn field<'a>(body: &'a Value, key: &str) -> Result<&'a str> {
    body[key]
        .as_str()
        .with_context(|| format!("缺少字段 {key}"))
}
fn qr(text: &str) -> Result<String> {
    Ok(qrcode::QrCode::new(text.as_bytes())?
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(220, 220)
        .build())
}
fn remove_cloud_devices(app: &App) -> Result<()> {
    // Cache only preferences. Logged-out devices are removed from the live engine and bridge.
    let mut records: Vec<xiaomi::CloudDevice> = load(&app.engine, "daemon:cloud-devices")?;
    for record in &mut records {
        if let Some(state) = app.engine.state(&record.device.did) {
            record.device.bridged = state.device.bridged;
        }
    }
    app.engine.store.vault.set(
        "daemon:cloud-devices",
        Some(serde_json::to_string(&records)?),
    )?;
    for state in app.engine.states() {
        if !state.device.did.starts_with("custom:") {
            app.bridge.disable(&state.device.did);
            app.engine.remove(&state.device.did)?;
        }
    }
    app.homes.lock().unwrap().clear();
    app.cloud_locations.lock().unwrap().clear();
    Ok(())
}
struct SyncLease<'a> {
    slot: &'a AtomicU64,
    value: u64,
}
impl Drop for SyncLease<'_> {
    fn drop(&mut self) {
        let _ = self
            .slot
            .compare_exchange(self.value, 0, Ordering::SeqCst, Ordering::SeqCst);
    }
}
fn sync_devices(app: &Arc<App>) -> Result<()> {
    let guard = app.gate.lock().unwrap();
    let Some(session) = app.session.read().unwrap().clone() else {
        return Ok(());
    };
    let generation = app.generation.load(Ordering::SeqCst);
    let lease_value = generation + 1;
    if app.sync_generation.swap(lease_value, Ordering::SeqCst) == lease_value {
        return Ok(());
    }
    let _lease = SyncLease {
        slot: &app.sync_generation,
        value: lease_value,
    };
    let cancel = app.cancelled.lock().unwrap().clone();
    *app.sync_status.lock().unwrap() = json!({"status":"running"});
    drop(guard);
    // Network requests do not hold the account lock, so logout cancels immediately.
    let fetched = xiaomi::fetch_devices(&session, &app.engine, &cancel);
    let _guard = app.gate.lock().unwrap();
    if app.generation.load(Ordering::SeqCst) != generation || app.session.read().unwrap().is_none()
    {
        return Ok(());
    }
    let result = (|| -> Result<Value> {
        let snapshot = fetched?;
        let records = snapshot.devices;
        let previous: Vec<xiaomi::CloudDevice> = load(&app.engine, "daemon:cloud-devices")?;
        let added = records
            .iter()
            .filter(|r| !previous.iter().any(|p| p.device.did == r.device.did))
            .count();
        app.engine.store.vault.set(
            "daemon:cloud-devices",
            Some(serde_json::to_string(&records)?),
        )?;
        app.engine.store.vault.set(
            "daemon:homes",
            Some(serde_json::to_string(&snapshot.homes)?),
        )?;
        let count = records.len();
        let unsupported = records
            .iter()
            .filter(|r| r.spec.properties.is_empty())
            .count();
        for state in app.engine.states() {
            if !state.device.did.starts_with("custom:")
                && !records.iter().any(|r| r.device.did == state.device.did)
            {
                app.bridge.disable(&state.device.did);
                app.engine.remove(&state.device.did)?;
            }
        }
        let mut locations = BTreeMap::new();
        for record in records {
            locations.insert(
                record.device.did.clone(),
                (record.home_id.clone(), record.home_name.clone()),
            );
            app.engine.register_transport(
                record.device.clone(),
                record.spec.clone(),
                Arc::new(xiaomi::CloudTransport::new(record, app.session.clone())),
            )?;
        }
        app.bridge.start()?;
        *app.homes.lock().unwrap() = snapshot.homes;
        *app.cloud_locations.lock().unwrap() = locations;
        Ok(json!({"status":"success","total":count,"added":added,"unsupported":unsupported}))
    })();
    match result {
        Ok(status) => {
            *app.sync_status.lock().unwrap() = status;
            Ok(())
        }
        Err(e) => {
            *app.sync_status.lock().unwrap() = json!({"status":"error","error":e.to_string()});
            Err(e)
        }
    }
}
