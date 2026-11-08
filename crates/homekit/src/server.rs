use crate::{
    crypto::{self, PairSetup, PairVerify, Session},
    mapping::{self, Database, Layout},
};
use anyhow::{Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use ed25519_dalek::SigningKey;
use mdns_sd::{ServiceDaemon, ServiceInfo};
use mijialan_core::Engine;
use rand::{Rng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha512};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize)]
struct Pairing {
    public: [u8; 32],
    admin: bool,
}
#[derive(Serialize, Deserialize)]
struct Identity {
    device_id: String,
    pin: String,
    setup_id: String,
    secret: [u8; 32],
    pairings: BTreeMap<String, Pairing>,
    port: u16,
}
impl Identity {
    fn create() -> Self {
        let mut rng = rand::thread_rng();
        let pin = loop {
            let text: String = (0..8)
                .map(|_| char::from(b'0' + rng.gen_range(0..10)))
                .collect();
            if text != "12345678"
                && text != "87654321"
                && text.as_bytes().iter().any(|b| *b != text.as_bytes()[0])
            {
                break format!("{}-{}-{}", &text[..3], &text[3..5], &text[5..]);
            }
        };
        let mut secret = [0; 32];
        rng.fill_bytes(&mut secret);
        let alphabet = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        Self {
            device_id: (0..6)
                .map(|_| format!("{:02X}", rng.r#gen::<u8>()))
                .collect::<Vec<_>>()
                .join(":"),
            pin,
            setup_id: (0..4)
                .map(|_| alphabet[rng.gen_range(0..36)] as char)
                .collect(),
            secret,
            pairings: BTreeMap::new(),
            port: 0,
        }
    }
    fn uri(&self) -> String {
        let code: u64 = self.pin.replace('-', "").parse().unwrap();
        let mut number = (2 << 31) | (2 << 27) | code;
        let mut digits = String::new();
        while number > 0 {
            digits.insert(
                0,
                b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"[(number % 36) as usize] as char,
            );
            number /= 36;
        }
        format!("X-HM://{:0>9}{}", digits, self.setup_id)
    }
}
impl Drop for Identity {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.secret.zeroize();
    }
}

pub struct BridgeInfo {
    pub running: bool,
    pub port: u16,
    pub paired: bool,
    pub code: String,
    pub uri: String,
    pub accessories: usize,
    pub error: Option<String>,
}
struct Shared {
    scope: Option<String>,
    prefix: String,
    engine: Arc<Engine>,
    identity: Mutex<Identity>,
    layout: Mutex<Layout>,
    database: RwLock<Database>,
    stop: AtomicBool,
    dirty: AtomicBool,
    warning: Mutex<Option<String>>,
    setup_owner: Mutex<Option<(u64, Instant)>>,
    attempts: Mutex<Vec<Instant>>,
    connections: Mutex<BTreeMap<u64, TcpStream>>,
}
impl Shared {
    fn save_identity(&self, identity: &Identity) -> Result<()> {
        self.engine.store.vault.set(
            &format!("{}identity", self.prefix),
            Some(serde_json::to_string(identity)?),
        )?;
        self.dirty.store(true, Ordering::Relaxed);
        Ok(())
    }
    fn refresh(&self) -> Result<()> {
        let mut layout = self.layout.lock().unwrap();
        let database = if let Some(did) = &self.scope {
            mapping::build_states(
                &self.engine,
                &mut layout,
                self.engine.state(did).into_iter().collect(),
            )
        } else {
            mapping::build(&self.engine, &mut layout)
        };
        let signature = serde_json::to_string(&database.accessories)?;
        // Values change independently. The configuration signature contains topology/metadata only.
        let mut topology: Value = serde_json::from_str(&signature)?;
        if let Some(accessories) = topology.as_array_mut() {
            for a in accessories {
                for s in a["services"].as_array_mut().unwrap() {
                    for c in s["characteristics"].as_array_mut().unwrap() {
                        if c["format"] != "string" {
                            c.as_object_mut().unwrap().remove("value");
                        }
                    }
                }
            }
        }
        let signature = topology.to_string();
        if layout.signature != signature {
            layout.version = (layout.version % 65535) + 1;
            layout.signature = signature;
            self.engine.store.vault.set(
                &format!("{}layout", self.prefix),
                Some(serde_json::to_string(&*layout)?),
            )?;
            self.dirty.store(true, Ordering::Relaxed);
        }
        *self.database.write().unwrap() = database;
        Ok(())
    }
}

pub struct HomeKit {
    scope: Option<String>,
    prefix: String,
    engine: Arc<Engine>,
    running: Mutex<Option<Arc<Shared>>>,
    error: Mutex<Option<String>>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}
impl HomeKit {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            scope: None,
            prefix: "homekit:".into(),
            engine,
            running: Mutex::new(None),
            error: Mutex::new(None),
            thread: Mutex::new(None),
        }
    }
    /// Each device has its own pairing identity, port and accessory database.
    pub fn for_device(engine: Arc<Engine>, did: &str) -> Self {
        let mut bridge = Self::new(engine);
        bridge.scope = Some(did.to_owned());
        bridge.prefix = format!("homekit:device:{:x}:", Sha512::digest(did.as_bytes()));
        bridge
    }
    pub fn start(&self) -> Result<()> {
        let result = self.start_at((Ipv4Addr::UNSPECIFIED, 0).into(), true);
        if let Err(error) = &result {
            *self.error.lock().unwrap() = Some(error.to_string());
        }
        result
    }
    pub fn start_at(&self, address: SocketAddr, announce: bool) -> Result<()> {
        let mut running = self.running.lock().unwrap();
        if running.is_some() {
            return Ok(());
        }
        let identity = match self
            .engine
            .store
            .vault
            .get(&format!("{}identity", self.prefix))?
        {
            Some(text) => {
                serde_json::from_str(&text).map_err(|_| anyhow!("桥接身份损坏，未覆盖配对数据"))?
            }
            None => Identity::create(),
        };
        let layout = match self
            .engine
            .store
            .vault
            .get(&format!("{}layout", self.prefix))?
        {
            Some(text) => serde_json::from_str(&text)?,
            None => Layout::default(),
        };
        let desired = if address.port() == 0 && identity.port != 0 {
            SocketAddr::new(address.ip(), identity.port)
        } else {
            address
        };
        let socket = socket2::Socket::new(
            socket2::Domain::for_address(desired),
            socket2::Type::STREAM,
            Some(socket2::Protocol::TCP),
        )?;
        #[cfg(not(target_os = "windows"))]
        socket.set_reuse_address(true)?;
        socket.bind(&desired.into())?;
        socket.listen(128)?;
        let listener: TcpListener = socket.into();
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let mut identity = identity;
        identity.port = port;
        let shared = Arc::new(Shared {
            scope: self.scope.clone(),
            prefix: self.prefix.clone(),
            engine: self.engine.clone(),
            identity: Mutex::new(identity),
            layout: Mutex::new(layout),
            database: RwLock::new(Database {
                accessories: vec![],
                bindings: BTreeMap::new(),
                names: BTreeMap::new(),
                warnings: Vec::new(),
            }),
            stop: AtomicBool::new(false),
            dirty: AtomicBool::new(true),
            warning: Mutex::new(None),
            setup_owner: Mutex::new(None),
            attempts: Mutex::new(vec![]),
            connections: Mutex::new(BTreeMap::new()),
        });
        shared.save_identity(&shared.identity.lock().unwrap())?;
        shared.refresh()?;
        let mdns = if announce {
            Some(ServiceDaemon::new()?)
        } else {
            None
        };
        if let Some(mdns) = &mdns {
            advertise(mdns, &shared)?;
        }
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
            .name("mijia-homekit".into())
            .spawn(move || {
                let mut refresh = Instant::now() - Duration::from_secs(6);
                let mut awake = None;
                while !worker.stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let mut connections = worker.connections.lock().unwrap();
                            if connections.len() >= 32 {
                                continue;
                            }
                            let id = rand::random();
                            if let Ok(copy) = stream.try_clone() {
                                connections.insert(id, copy);
                            }
                            drop(connections);
                            let worker = worker.clone();
                            std::thread::spawn(move || {
                                let _ = serve(stream, &worker, id);
                                worker.connections.lock().unwrap().remove(&id);
                                let mut owner = worker.setup_owner.lock().unwrap();
                                if owner.is_some_and(|(owner, _)| owner == id) {
                                    *owner = None;
                                }
                            });
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                        Err(_) => break,
                    }
                    if refresh.elapsed() > Duration::from_secs(5) {
                        let _ = worker.refresh();
                        if announce && worker.engine.store.config().keep_awake {
                            if awake.is_none() {
                                match keepawake::Builder::default()
                                    .idle(true)
                                    .app_name("MijiaLAN Rust")
                                    .app_reverse_domain("xyz.ipocket.MijiaLAN.Rust")
                                    .reason("HomeKit bridge")
                                    .create()
                                {
                                    Ok(assertion) => {
                                        awake = Some(assertion);
                                        *worker.warning.lock().unwrap() = None;
                                    }
                                    Err(error) => {
                                        *worker.warning.lock().unwrap() =
                                            Some(format!("无法阻止闲置休眠：{error}"));
                                    }
                                }
                            }
                        } else {
                            awake = None;
                            *worker.warning.lock().unwrap() = None;
                        }
                        refresh = Instant::now();
                    }
                    if worker.dirty.swap(false, Ordering::Relaxed)
                        && let Some(mdns) = &mdns
                    {
                        let _ = advertise(mdns, &worker);
                    }
                    std::thread::sleep(Duration::from_millis(40));
                }
                if let Some(mdns) = mdns {
                    let _ = mdns.shutdown();
                }
            })?;
        *self.thread.lock().unwrap() = Some(thread);
        *running = Some(shared);
        *self.error.lock().unwrap() = None;
        Ok(())
    }
    pub fn stop(&self) {
        if let Some(shared) = self.running.lock().unwrap().take() {
            shared.stop.store(true, Ordering::Relaxed);
            for (_, stream) in std::mem::take(&mut *shared.connections.lock().unwrap()) {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
            if let Some(thread) = self.thread.lock().unwrap().take() {
                let _ = thread.join();
            }
        }
    }
    /// Only the explicit, confirmed reset control invokes this operation.
    pub fn reset_pairing(&self) -> Result<()> {
        let restart = self.info().running;
        self.stop();
        if let Err(error) = self
            .engine
            .store
            .vault
            .set(&format!("{}identity", self.prefix), None)
        {
            if restart {
                let _ = self.start();
            }
            return Err(error.into());
        }
        if restart {
            self.start()?;
        }
        Ok(())
    }
    pub fn info(&self) -> BridgeInfo {
        if let Some(shared) = self.running.lock().unwrap().as_ref() {
            let identity = shared.identity.lock().unwrap();
            let (accessories, mut warnings) = {
                let database = shared.database.read().unwrap();
                (
                    database.accessories.len().saturating_sub(1),
                    database.warnings.clone(),
                )
            };
            warnings.extend(shared.warning.lock().unwrap().clone());
            BridgeInfo {
                running: true,
                port: identity.port,
                paired: !identity.pairings.is_empty(),
                code: identity.pin.clone(),
                uri: identity.uri(),
                accessories,
                error: (!warnings.is_empty()).then(|| warnings.join("\n")),
            }
        } else {
            BridgeInfo {
                running: false,
                port: 0,
                paired: false,
                code: String::new(),
                uri: String::new(),
                accessories: 0,
                error: self.error.lock().unwrap().clone(),
            }
        }
    }
}
impl Drop for HomeKit {
    fn drop(&mut self) {
        self.stop();
    }
}

fn advertise(mdns: &ServiceDaemon, shared: &Shared) -> Result<()> {
    let identity = shared.identity.lock().unwrap();
    let version = shared.layout.lock().unwrap().version;
    let hash = Sha512::digest(format!("{}{}", identity.setup_id, identity.device_id).as_bytes());
    let properties: Vec<(String, String)> = vec![
        ("c#".into(), version.to_string()),
        ("ff".into(), "0".into()),
        ("id".into(), identity.device_id.clone()),
        ("md".into(), "MijiaLAN Rust Bridge".into()),
        ("pv".into(), "1.1".into()),
        ("s#".into(), "1".into()),
        (
            "sf".into(),
            if identity.pairings.is_empty() {
                "1"
            } else {
                "0"
            }
            .into(),
        ),
        ("ci".into(), "2".into()),
        ("sh".into(), B64.encode(&hash[..4])),
    ];
    let hostname = format!("mijialan-rust-{}.local.", identity.setup_id.to_lowercase());
    let device_name = shared
        .scope
        .as_ref()
        .and_then(|did| shared.engine.state(did))
        .map(|state| state.device.name)
        .unwrap_or_else(|| "MijiaLAN".into());
    let name = format!(
        "{} {}",
        device_name.chars().take(16).collect::<String>(),
        identity.setup_id
    );
    let info = ServiceInfo::new(
        "_hap._tcp.local.",
        &name,
        &hostname,
        "",
        identity.port,
        properties.as_slice(),
    )?
    .enable_addr_auto();
    mdns.register(info)?;
    Ok(())
}

struct Request {
    method: String,
    target: String,
    body: Vec<u8>,
}
fn request(buffer: &mut Vec<u8>) -> Result<Option<Request>> {
    let Some(end) = buffer.windows(4).position(|s| s == b"\r\n\r\n") else {
        if buffer.len() > 16384 {
            bail!("headers too large");
        }
        return Ok(None);
    };
    if end > 16384 {
        bail!("headers too large");
    }
    let text = std::str::from_utf8(&buffer[..end])?;
    let mut lines = text.split("\r\n");
    let mut first = lines
        .next()
        .ok_or_else(|| anyhow!("missing HTTP start"))?
        .split_whitespace();
    let method = first
        .next()
        .ok_or_else(|| anyhow!("missing method"))?
        .to_owned();
    let target = first
        .next()
        .ok_or_else(|| anyhow!("missing target"))?
        .to_owned();
    let mut length = None;
    for line in lines {
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| anyhow!("invalid HTTP header"))?;
        if key.eq_ignore_ascii_case("transfer-encoding") {
            bail!("transfer encoding unsupported");
        }
        if key.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                bail!("duplicate content length");
            }
            length = Some(value.trim().parse::<usize>()?);
        }
    }
    let length = length.unwrap_or(0);
    if length > 262144 {
        bail!("body too large");
    }
    let offset = end + 4;
    if buffer.len() < offset + length {
        return Ok(None);
    }
    let body = buffer[offset..offset + length].to_vec();
    buffer.drain(..offset + length);
    Ok(Some(Request {
        method,
        target,
        body,
    }))
}
fn response(status: u16, content_type: &str, body: &[u8], event: bool) -> Vec<u8> {
    let phrase = match status {
        200 => "OK",
        204 => "No Content",
        207 => "Multi-Status",
        470 => "Connection Authorization Required",
        _ => "Error",
    };
    let mut data = format!(
        "{}/1.{} {status} {phrase}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
        if event { "EVENT" } else { "HTTP" },
        if event { 0 } else { 1 },
        body.len()
    )
    .into_bytes();
    data.extend_from_slice(body);
    data
}
fn json_response(status: u16, value: Value) -> Vec<u8> {
    response(
        status,
        "application/hap+json",
        value.to_string().as_bytes(),
        false,
    )
}
fn pairing_response(body: &[u8]) -> Vec<u8> {
    response(200, "application/pairing+tlv8", body, false)
}
fn pairing_error(state: u8, error: u8) -> Vec<u8> {
    pairing_response(&crypto::tlv(&[(6, &[state]), (7, &[error])]))
}

struct Connection {
    id: u64,
    setup: Option<PairSetup>,
    verify: Option<PairVerify>,
    session: Option<Session>,
    activate: Option<Session>,
    controller: Option<(String, [u8; 32])>,
    subscriptions: BTreeMap<(u64, u64), Value>,
}
impl Connection {
    fn authenticated(&self, shared: &Shared) -> bool {
        self.controller.as_ref().is_some_and(|(id, public)| {
            shared
                .identity
                .lock()
                .unwrap()
                .pairings
                .get(id)
                .is_some_and(|p| p.public == *public)
        })
    }
    fn route(&mut self, r: Request, shared: &Shared) -> Result<Vec<u8>> {
        if r.method == "POST" && r.target == "/pair-setup" {
            let fields = crypto::parse(&r.body)?;
            let state = crypto::field(&fields, 6)?.first().copied().unwrap_or(0);
            if !shared.identity.lock().unwrap().pairings.is_empty() {
                return Ok(pairing_error(state.saturating_add(1), 6));
            }
            let mut owner = shared.setup_owner.lock().unwrap();
            if owner.is_some_and(|(id, at)| id != self.id && at.elapsed() < Duration::from_secs(30))
            {
                return Ok(pairing_error(state.saturating_add(1), 7));
            }
            if state == 1 {
                let mut attempts = shared.attempts.lock().unwrap();
                attempts.retain(|at| at.elapsed() < Duration::from_secs(60));
                if attempts.len() >= 8 {
                    return Ok(pairing_error(2, 5));
                }
                attempts.push(Instant::now());
                if crypto::field(&fields, 0)? != [0] {
                    return Ok(pairing_error(2, 1));
                }
                *owner = Some((self.id, Instant::now()));
                let (setup, body) = PairSetup::start(&shared.identity.lock().unwrap().pin)?;
                self.setup = Some(setup);
                return Ok(pairing_response(&body));
            }
            if owner.is_none_or(|(id, at)| id != self.id || at.elapsed() > Duration::from_secs(30))
            {
                return Ok(pairing_error(state.saturating_add(1), 2));
            }
            let result = (|| -> Result<Vec<u8>> {
                let setup = self
                    .setup
                    .as_mut()
                    .ok_or_else(|| anyhow!("setup not started"))?;
                if state == 3 {
                    return Ok(pairing_response(&setup.prove(&r.body)?));
                }
                if state == 5 {
                    let mut identity = shared.identity.lock().unwrap();
                    let signing = SigningKey::from_bytes(&identity.secret);
                    let (id, public, body) =
                        setup.finish(&r.body, &identity.device_id, &signing)?;
                    identity.pairings.insert(
                        id.clone(),
                        Pairing {
                            public,
                            admin: true,
                        },
                    );
                    if let Err(error) = shared.save_identity(&identity) {
                        identity.pairings.remove(&id);
                        return Err(error);
                    }
                    *owner = None;
                    return Ok(pairing_response(&body));
                }
                bail!("unknown setup state")
            })();
            return Ok(match result {
                Ok(v) => v,
                Err(_) => {
                    self.setup = None;
                    *owner = None;
                    pairing_error(state.saturating_add(1), 2)
                }
            });
        }
        if r.method == "POST" && r.target == "/pair-verify" {
            let result = (|| -> Result<Vec<u8>> {
                let fields = crypto::parse(&r.body)?;
                match crypto::field(&fields, 6)? {
                    [1] => {
                        let identity = shared.identity.lock().unwrap();
                        let (verify, body) = PairVerify::start(
                            &r.body,
                            &identity.device_id,
                            &SigningKey::from_bytes(&identity.secret),
                        )?;
                        self.verify = Some(verify);
                        Ok(pairing_response(&body))
                    }
                    [3] => {
                        let verify = self
                            .verify
                            .take()
                            .ok_or_else(|| anyhow!("verify not started"))?;
                        let (id, signature) = verify.controller(&r.body)?;
                        let identity = shared.identity.lock().unwrap();
                        let pairing = identity
                            .pairings
                            .get(&id)
                            .ok_or_else(|| anyhow!("unknown controller"))?;
                        let session = verify.finish(&id, &pairing.public, &signature)?;
                        self.controller = Some((id, pairing.public));
                        self.activate = Some(session);
                        Ok(pairing_response(&crypto::tlv(&[(6, &[4])])))
                    }
                    _ => bail!("unknown verify state"),
                }
            })();
            return Ok(result
                .unwrap_or_else(|_| pairing_error(if self.verify.is_some() { 4 } else { 2 }, 2)));
        }
        if r.target == "/identify"
            && r.method == "POST"
            && shared.identity.lock().unwrap().pairings.is_empty()
        {
            return Ok(response(204, "application/hap+json", &[], false));
        }
        if !self.authenticated(shared) || self.session.is_none() {
            return Ok(json_response(470, json!({"status":-70401})));
        }
        if r.target == "/pairings" && r.method == "POST" {
            let mut identity = shared.identity.lock().unwrap();
            if !identity
                .pairings
                .get(&self.controller.as_ref().unwrap().0)
                .is_some_and(|p| p.admin)
            {
                return Ok(pairing_error(2, 2));
            }
            let fields = crypto::parse(&r.body)?;
            let original = identity.pairings.clone();
            match crypto::field(&fields, 0)? {
                [3] => {
                    if identity.pairings.len() >= 16 {
                        return Ok(pairing_error(2, 4));
                    }
                    let id = std::str::from_utf8(crypto::field(&fields, 1)?)?.to_string();
                    let public: [u8; 32] = crypto::field(&fields, 3)?
                        .try_into()
                        .map_err(|_| anyhow!("invalid key"))?;
                    let permissions = crypto::field(&fields, 11)?;
                    if permissions != [0] && permissions != [1] {
                        return Ok(pairing_error(2, 2));
                    }
                    if identity
                        .pairings
                        .get(&id)
                        .is_some_and(|p| p.public != public)
                    {
                        return Ok(pairing_error(2, 1));
                    }
                    identity.pairings.insert(
                        id,
                        Pairing {
                            public,
                            admin: permissions == [1],
                        },
                    );
                }
                [4] => {
                    let id = std::str::from_utf8(crypto::field(&fields, 1)?)?;
                    identity.pairings.remove(id);
                    if !identity.pairings.values().any(|p| p.admin) {
                        identity.pairings.clear();
                    }
                }
                [5] => {
                    let mut body = crypto::tlv(&[(6, &[2])]);
                    for (index, (id, p)) in identity.pairings.iter().enumerate() {
                        if index > 0 {
                            body.extend_from_slice(&[255, 0]);
                        }
                        body.extend_from_slice(&crypto::tlv(&[
                            (1, id.as_bytes()),
                            (3, &p.public),
                            (11, &[u8::from(p.admin)]),
                        ]));
                    }
                    return Ok(pairing_response(&body));
                }
                _ => return Ok(pairing_error(2, 1)),
            }
            if !identity.pairings.values().any(|p| p.admin) {
                identity.pairings.clear();
            }
            if let Err(error) = shared.save_identity(&identity) {
                identity.pairings = original;
                return Err(error);
            }
            return Ok(pairing_response(&crypto::tlv(&[(6, &[2])])));
        }
        if r.target == "/accessories" && r.method == "GET" {
            return Ok(json_response(
                200,
                shared.database.read().unwrap().json(&shared.engine),
            ));
        }
        if r.target.starts_with("/characteristics?") && r.method == "GET" {
            let ids = r
                .target
                .split('?')
                .nth(1)
                .unwrap_or_default()
                .split('&')
                .find_map(|f| f.strip_prefix("id="))
                .unwrap_or_default();
            let database = shared.database.read().unwrap();
            let mut values = Vec::new();
            let mut errors = false;
            for id in ids.split(',').take(2048) {
                let Some((aid, iid)) = id
                    .split_once('.')
                    .and_then(|(a, i)| Some((a.parse::<u64>().ok()?, i.parse::<u64>().ok()?)))
                else {
                    continue;
                };
                let mut item = json!({"aid":aid,"iid":iid});
                let attribute = database.attribute(aid, iid);
                let result = database.read(&shared.engine, aid, iid, false);
                if let Some(attribute) = attribute {
                    for flag in [
                        "type",
                        "perms",
                        "format",
                        "minValue",
                        "maxValue",
                        "minStep",
                        "unit",
                        "valid-values",
                    ] {
                        if (r.target.contains("meta=1")
                            || flag == "type" && r.target.contains("type=1")
                            || flag == "perms" && r.target.contains("perms=1"))
                            && attribute.get(flag).is_some()
                        {
                            item[flag] = attribute[flag].clone();
                        }
                    }
                }
                match result {
                    Ok(v) => item["value"] = v,
                    Err(status) => {
                        item["status"] = json!(status);
                        errors = true;
                    }
                }
                values.push(item);
            }
            if errors {
                for item in &mut values {
                    if item.get("status").is_none() {
                        item["status"] = json!(0);
                    }
                }
            }
            return Ok(json_response(
                if errors { 207 } else { 200 },
                json!({"characteristics":values}),
            ));
        }
        if r.target == "/characteristics" && r.method == "PUT" {
            let body: Value = serde_json::from_slice(&r.body)?;
            let mut results = Vec::new();
            let items = body["characteristics"]
                .as_array()
                .ok_or_else(|| anyhow!("missing characteristics"))?;
            if items.len() > 2048 {
                bail!("too many writes");
            }
            for item in items {
                let (Some(aid), Some(iid)) = (item["aid"].as_u64(), item["iid"].as_u64()) else {
                    continue;
                };
                // Release the database borrow before a successful rename
                // refreshes its metadata and configuration number.
                let (binding, name_key, attribute) = {
                    let database = shared.database.read().unwrap();
                    (
                        database.bindings.get(&(aid, iid)).cloned(),
                        database.names.get(&(aid, iid)).cloned(),
                        database.attribute(aid, iid).cloned(),
                    )
                };
                let mut status = 0;
                if let Some(binding) = binding {
                    if let Some(value) = item.get("value") {
                        let result = attribute
                            .as_ref()
                            .ok_or(-70409)
                            .and_then(|a| mapping::validate_write(a, value))
                            .and_then(|value| binding.write(&shared.engine, &value));
                        match result {
                            Err(error) => status = error,
                            Ok(()) => {
                                // Momentary actions and presets may read back false even after
                                // a successful true write. Echo their authoritative state.
                                if let Some(last) = self.subscriptions.get_mut(&(aid, iid)) {
                                    *last = Value::Null;
                                }
                            }
                        }
                    }
                } else if let Some(name_key) = name_key {
                    if let Some(value) = item.get("value") {
                        if let Some(name) = value
                            .as_str()
                            .filter(|n| !n.trim().is_empty() && n.len() <= 64)
                        {
                            if shared
                                .engine
                                .store
                                .update(|c| {
                                    c.homekit_names.insert(name_key, name.into());
                                })
                                .is_err()
                                || shared.refresh().is_err()
                            {
                                status = -70402;
                            }
                        } else {
                            status = -70410;
                        }
                    }
                } else if attribute.is_none() {
                    status = -70409;
                } else if item.get("value").is_some() && iid != 2 {
                    status = -70404;
                }
                if let Some(ev) = item.get("ev") {
                    let supports_events = attribute.as_ref().is_some_and(|a| {
                        a["perms"]
                            .as_array()
                            .is_some_and(|p| p.iter().any(|p| p == "ev"))
                    });
                    if status == 0 && !supports_events {
                        status = -70406;
                    } else if status == 0 {
                        match ev.as_bool() {
                            Some(true) if self.subscriptions.len() < 2048 => {
                                self.subscriptions.insert((aid, iid), Value::Null);
                            }
                            Some(true) => status = -70407,
                            Some(false) => {
                                self.subscriptions.remove(&(aid, iid));
                            }
                            None => status = -70410,
                        }
                    }
                }
                results.push(json!({"aid":aid,"iid":iid,"status":status}));
            }
            let errors = results.iter().any(|v| v["status"] != 0);
            return Ok(if errors {
                json_response(207, json!({"characteristics":results}))
            } else {
                response(204, "application/hap+json", &[], false)
            });
        }
        Ok(json_response(404, json!({"status":-70409})))
    }
}

fn serve(mut stream: TcpStream, shared: &Shared, id: u64) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_millis(250)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.set_nodelay(true)?;
    let mut connection = Connection {
        id,
        setup: None,
        verify: None,
        session: None,
        activate: None,
        controller: None,
        subscriptions: BTreeMap::new(),
    };
    let mut wire = Vec::new();
    let mut plain = Vec::new();
    let mut input = [0; 8192];
    let mut last = Instant::now();
    while !shared.stop.load(Ordering::Relaxed) {
        if connection.session.is_some() && !connection.authenticated(shared) {
            break;
        }
        if let Some(session) = &mut connection.session {
            while wire.len() >= 2 {
                let len = u16::from_le_bytes([wire[0], wire[1]]) as usize;
                if len > 1024 {
                    bail!("oversized HAP frame");
                }
                if wire.len() < len + 18 {
                    break;
                }
                let frame: Vec<_> = wire.drain(..len + 18).collect();
                plain.extend_from_slice(&session.decode_frame(&frame)?);
            }
        } else {
            plain.append(&mut wire);
        }
        while let Some(request) = request(&mut plain)? {
            let response = connection.route(request, shared)?;
            if let Some(session) = &mut connection.session {
                stream.write_all(&session.encode(&response)?)?;
            } else {
                stream.write_all(&response)?;
            }
            if let Some(session) = connection.activate.take() {
                connection.session = Some(session);
                wire.append(&mut plain);
                break;
            }
        }
        if plain.len() > 278528 || wire.len() > 278528 {
            bail!("buffer limit");
        }
        if let Some(session) = &mut connection.session {
            let database = shared.database.read().unwrap();
            let mut changed = Vec::new();
            for ((aid, iid), last_value) in &mut connection.subscriptions {
                let result = database.read(&shared.engine, *aid, *iid, false);
                let item = match result {
                    Ok(value) => json!({"aid":aid,"iid":iid,"value":value}),
                    Err(status) => json!({"aid":aid,"iid":iid,"status":status}),
                };
                if *last_value != item {
                    *last_value = item.clone();
                    changed.push(item);
                }
            }
            drop(database);
            if !changed.is_empty() {
                let body = json!({"characteristics":changed}).to_string();
                let event = response(200, "application/hap+json", body.as_bytes(), true);
                stream.write_all(&session.encode(&event)?)?;
            }
        }
        match stream.read(&mut input) {
            Ok(0) => break,
            Ok(n) => {
                wire.extend_from_slice(&input[..n]);
                last = Instant::now();
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                if connection.session.is_none() && last.elapsed() > Duration::from_secs(60) {
                    break;
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
    let mut owner = shared.setup_owner.lock().unwrap();
    if owner.is_some_and(|(id, _)| id == connection.id) {
        *owner = None;
    }
    Ok(())
}
