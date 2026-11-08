use crate::{
    Device, Error, MiioClient, Result, Specification, Store, Token, legacy::Profile, spec,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    net::Ipv4Addr,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize)]
pub struct DeviceState {
    pub device: Device,
    pub spec: Specification,
    pub values: BTreeMap<String, Value>,
    pub errors: BTreeMap<String, String>,
    pub online: bool,
    pub error: Option<String>,
    pub revision: u64,
}

/// Additional transports share the same HomeKit state and stable device IDs.
pub trait DeviceTransport: Send + Sync {
    fn read(&self) -> Result<BTreeMap<String, Value>>;
    fn poll_seconds(&self) -> u64 {
        10
    }
    fn action(&self, _siid: u32, _aiid: u32, _inputs: Vec<Value>) -> Result<()> {
        Err(Error::Validation("此设备没有动作接口".into()))
    }
    fn write(&self, key: &str, value: Value) -> Result<()>;
}
pub struct Engine {
    pub store: Arc<Store>,
    clients: Mutex<HashMap<String, Arc<Mutex<MiioClient>>>>,
    states: RwLock<BTreeMap<String, DeviceState>>,
    demo: bool,
    transports: RwLock<HashMap<String, Arc<dyn DeviceTransport>>>,
}

impl Engine {
    pub fn open() -> Result<Self> {
        Ok(Self::new(Arc::new(Store::open()?), false))
    }
    pub fn new(store: Arc<Store>, demo: bool) -> Self {
        crate::localization::configure(&store.config().language);
        let states = store
            .config()
            .devices
            .into_iter()
            .map(|device| {
                let text = Specification::bundled(&device.model)
                    .map(str::to_owned)
                    .or_else(|| store.cached_spec(&device.model));
                let spec = text
                    .as_deref()
                    .and_then(|text| Specification::for_model(&device.model, text).ok())
                    .unwrap_or_default();
                (
                    device.did.clone(),
                    DeviceState {
                        device,
                        spec,
                        values: BTreeMap::new(),
                        errors: BTreeMap::new(),
                        online: false,
                        error: None,
                        revision: 0,
                    },
                )
            })
            .collect();
        Self {
            store,
            clients: Mutex::new(HashMap::new()),
            states: RwLock::new(states),
            demo,
            transports: RwLock::new(HashMap::new()),
        }
    }
    pub fn demo() -> Self {
        let store = Arc::new(Store::memory());
        for (index, name, model, room) in [
            (1, "书桌台灯", "yeelink.light.lamp4", "书房"),
            (2, "循环风扇", "dmaker.fan.p28", "客厅"),
            (3, "空气净化器", "zhimi.airpurifier.rma1", "卧室"),
            (4, "智能插座", "chuangmi.plug.m3", "客厅"),
        ] {
            store
                .put(
                    Device {
                        did: index.to_string(),
                        name: name.into(),
                        model: model.into(),
                        host: Ipv4Addr::new(192, 168, 31, 20 + index),
                        room: room.into(),
                        port: crate::packet::PORT,
                        bridged: true,
                    },
                    &Token::parse("00112233445566778899aabbccddeeff").unwrap(),
                )
                .unwrap();
        }
        let engine = Self::new(store, true);
        for state in engine.states.write().unwrap().values_mut() {
            state.online = true;
            for property in &state.spec.properties {
                let value = if property.format == "bool" {
                    json!(property.name == "on")
                } else if let Some(choice) = property.choices.first() {
                    choice.value.clone()
                } else if let Some([min, max, step]) = property.range {
                    numeric(
                        min + (((max - min) * 0.45) / step).round() * step,
                        &property.format,
                    )
                } else {
                    json!(0)
                };
                state.values.insert(property.key(), value);
            }
        }
        engine
    }
    pub fn register_transport(
        &self,
        device: Device,
        spec: Specification,
        transport: Arc<dyn DeviceTransport>,
    ) -> Result<()> {
        self.store.update(|c| {
            if let Some(old) = c.devices.iter_mut().find(|d| d.did == device.did) {
                *old = device.clone();
            } else {
                c.devices.push(device.clone());
            }
        })?;
        self.transports
            .write()
            .unwrap()
            .insert(device.did.clone(), transport);
        let previous = self.state(&device.did);
        self.states.write().unwrap().insert(
            device.did.clone(),
            DeviceState {
                device,
                spec,
                values: previous
                    .as_ref()
                    .map(|s| s.values.clone())
                    .unwrap_or_default(),
                errors: BTreeMap::new(),
                online: previous.as_ref().is_some_and(|s| s.online),
                error: None,
                revision: previous.map_or(0, |s| s.revision + 1),
            },
        );
        Ok(())
    }
    pub fn report(&self, did: &str, values: BTreeMap<String, Value>) -> Result<()> {
        let mut states = self.states.write().unwrap();
        let state = states
            .get_mut(did)
            .ok_or_else(|| Error::Validation("设备不存在".into()))?;
        for (key, value) in &values {
            let mut property = state
                .spec
                .properties
                .iter()
                .find(|p| p.key() == *key)
                .cloned()
                .ok_or_else(|| Error::Validation("未知属性".into()))?;
            property.writable = true;
            if property.format == "bool" || !property.choices.is_empty() || property.range.is_some()
            {
                property.validate(value)?;
            } else if (property.format == "string" && !value.is_string())
                || (property.format != "string" && !value.is_number())
            {
                return Err(Error::Validation("属性类型无效".into()));
            }
        }
        state.values.extend(values);
        state.online = true;
        state.error = None;
        state.revision += 1;
        Ok(())
    }
    pub fn is_demo(&self) -> bool {
        self.demo
    }
    /// A credential-free control-center fixture, isolated from the real device store.
    pub fn demo_control_center() -> Self {
        let store = Arc::new(Store::memory());
        for (index, name, model, room) in [
            (1, "插座", "chuangmi.plug.m3", "卧室"),
            (2, "台灯", "yeelink.light.lamp4", "卧室"),
            (
                3,
                "米家智能空气净化器 4 Lite",
                "zhimi.airpurifier.rma1",
                "卧室",
            ),
            (4, "米家智能破壁料理机", "chunmi.juicer.a1", "厨房"),
            (5, "米家智能多功能养生壶", "chunmi.health_pot.a1", "客厅"),
            (6, "空调插座", "lumi.acpartner.mcn02", "卧室"),
            (
                7,
                "米家智能空气炸锅 5.5L 可视版",
                "careli.fryer.maf07c",
                "厨房",
            ),
            (
                8,
                "米家智能微压 IH 电饭煲 3L",
                "chunmi.cooker.cmwy3",
                "厨房",
            ),
            (9, "米家智能直流变频循环扇 落地式", "dmaker.fan.p28", "卧室"),
            (10, "客厅-米家智能插座 3", "cuco.plug.v3", "客厅"),
            (11, "卧室-米家智能插座 3", "cuco.plug.v3", "卧室"),
        ] {
            store
                .put(
                    Device {
                        did: format!("{index:02}"),
                        name: name.into(),
                        model: model.into(),
                        host: Ipv4Addr::new(192, 0, 2, 20 + index),
                        room: room.into(),
                        port: crate::packet::PORT,
                        bridged: true,
                    },
                    &Token::parse("00112233445566778899aabbccddeeff").unwrap(),
                )
                .unwrap();
        }
        let engine = Self::new(store, true);
        for state in engine.states.write().unwrap().values_mut() {
            if state.device.did != "02" && state.device.did != "06" {
                state.error = Some(Error::Timeout.to_string());
                continue;
            }
            state.online = true;
            for property in &state.spec.properties {
                let value = if property.format == "bool" {
                    json!(property.name == "on")
                } else if property.name == "brightness" {
                    json!(71)
                } else if property.name == "color-temperature" {
                    json!(3500)
                } else if property.name == "target-temperature" {
                    json!(29)
                } else if let Some(choice) = property.choices.first() {
                    choice.value.clone()
                } else if let Some([min, _, _]) = property.range {
                    numeric(min, &property.format)
                } else {
                    json!(0)
                };
                state.values.insert(property.key(), value);
            }
        }
        engine
    }
    pub fn set_bridged(&self, did: &str, bridged: bool) -> Result<()> {
        self.store.update(|config| {
            if let Some(device) = config.devices.iter_mut().find(|d| d.did == did) {
                device.bridged = bridged;
            }
        })?;
        if let Some(state) = self.states.write().unwrap().get_mut(did) {
            state.device.bridged = bridged;
        }
        Ok(())
    }
    pub fn states(&self) -> Vec<DeviceState> {
        self.states.read().unwrap().values().cloned().collect()
    }
    pub fn state(&self, did: &str) -> Option<DeviceState> {
        self.states.read().unwrap().get(did).cloned()
    }
    fn device(&self, did: &str) -> Result<Device> {
        self.store
            .config()
            .devices
            .into_iter()
            .find(|d| d.did == did)
            .ok_or_else(|| Error::Validation("设备不存在".into()))
    }
    fn client(&self, device: &Device) -> Result<Arc<Mutex<MiioClient>>> {
        let mut clients = self.clients.lock().unwrap();
        if let Some(client) = clients.get(&device.did) {
            return Ok(client.clone());
        }
        let client = Arc::new(Mutex::new(MiioClient::with_port(
            device.host,
            device.port,
            self.store.token(&device.did)?,
        )));
        clients.insert(device.did.clone(), client.clone());
        Ok(client)
    }
    pub fn add(&self, name: &str, host: Ipv4Addr, token: Token, port: u16) -> Result<Device> {
        if self.demo {
            return Err(Error::Validation("演示模式不添加真实设备".into()));
        }
        let mut client = MiioClient::with_port(host, port, token.clone());
        let hello = client.handshake()?;
        let info = client.info()?;
        let model = info["model"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| Error::Response("设备信息缺少型号".into()))?;
        let device = Device {
            did: hello.device_id.to_string(),
            name: if name.trim().is_empty() {
                model.into()
            } else {
                name.trim().into()
            },
            model: model.into(),
            host,
            room: String::new(),
            port,
            bridged: true,
        };
        self.import(device.clone(), &token)?;
        self.clients
            .lock()
            .unwrap()
            .insert(device.did.clone(), Arc::new(Mutex::new(client)));
        let _ = self.refresh(&device.did);
        Ok(device)
    }
    pub fn import(&self, mut device: Device, token: &Token) -> Result<()> {
        if let Some(old) = self
            .store
            .config()
            .devices
            .iter()
            .find(|d| d.did == device.did)
        {
            if device.name.is_empty() {
                device.name = old.name.clone();
            }
            if device.room.is_empty() {
                device.room = old.room.clone();
            }
            device.bridged = old.bridged;
        }
        self.store.put(device.clone(), token)?;
        self.clients.lock().unwrap().remove(&device.did);
        let spec = self.load_spec(&device.model, false).unwrap_or_default();
        self.states.write().unwrap().insert(
            device.did.clone(),
            DeviceState {
                device,
                spec,
                values: BTreeMap::new(),
                errors: BTreeMap::new(),
                online: false,
                error: None,
                revision: 0,
            },
        );
        Ok(())
    }
    fn load_spec(&self, model: &str, network: bool) -> Result<Specification> {
        if let Some(text) = Specification::bundled(model) {
            return Specification::for_model(model, text);
        }
        if let Some(text) = self.store.cached_spec(model) {
            return Specification::for_model(model, &text);
        }
        if !network {
            return Err(Error::Spec("规格尚未缓存".into()));
        }
        let text = spec::fetch(model)?;
        self.store.cache_spec(model, &text)?;
        Specification::for_model(model, &text)
    }
    pub fn refresh(&self, did: &str) -> Result<DeviceState> {
        if self.demo {
            return self
                .state(did)
                .ok_or_else(|| Error::Validation("设备不存在".into()));
        }
        let result = (|| {
            let device = self.device(did)?;
            let spec = self
                .state(did)
                .map(|s| s.spec)
                .filter(|s| !s.properties.is_empty())
                .map(Ok)
                .unwrap_or_else(|| self.load_spec(&device.model, true))?;
            let transport = self.transports.read().unwrap().get(did).cloned();
            if let Some(transport) = transport {
                let values = transport.read()?;
                self.report(did, values.clone())?;
                if let Some(state) = self.states.write().unwrap().get_mut(did) {
                    state.values = values;
                    state.errors = state
                        .spec
                        .properties
                        .iter()
                        .filter(|p| p.readable && !state.values.contains_key(&p.key()))
                        .map(|p| (p.key(), "本次未返回此属性".into()))
                        .collect();
                }
                return self
                    .state(did)
                    .ok_or_else(|| Error::Validation("设备不存在".into()));
            }
            let profile = Profile::for_model(&device.model);
            let client = self.client(&device)?;
            let mut client = client.lock().unwrap();
            let mut errors = BTreeMap::new();
            let values = if let Some(profile) = profile {
                profile.read(&mut client, &spec)?
            } else {
                let props: Vec<_> = spec
                    .properties
                    .iter()
                    .filter(|p| p.readable)
                    .map(|p| (p.siid, p.piid))
                    .collect();
                let response = client.get_properties(did, &props)?;
                let mut values = BTreeMap::new();
                for p in &spec.properties {
                    if p.readable {
                        errors.insert(p.key(), "设备未返回此属性".into());
                    }
                }
                for item in response {
                    let Some(siid) = item["siid"].as_u64() else {
                        continue;
                    };
                    let Some(piid) = item["piid"].as_u64() else {
                        continue;
                    };
                    if !props.contains(&(siid as u32, piid as u32)) {
                        continue;
                    }
                    let key = format!("{siid}.{piid}");
                    if item["code"].as_i64() == Some(0) && item.get("value").is_some() {
                        values.insert(key.clone(), item["value"].clone());
                        errors.remove(&key);
                    } else {
                        errors.insert(key, format!("设备返回 {}", item["code"]));
                    }
                }
                values
            };
            if values.is_empty() {
                return Err(Error::Response("设备没有返回可用属性".into()));
            }
            let revision = self.state(did).map_or(1, |s| s.revision + 1);
            Ok(DeviceState {
                device,
                spec,
                values,
                errors,
                online: true,
                error: None,
                revision,
            })
        })();
        match result {
            Ok(state) => {
                let mut states = self.states.write().unwrap();
                let current = states
                    .get_mut(did)
                    .ok_or_else(|| Error::Validation("设备已移除".into()))?;
                *current = state.clone();
                Ok(state)
            }
            Err(error) => {
                if let Some(state) = self.states.write().unwrap().get_mut(did) {
                    state.online = false;
                    state.error = Some(error.to_string());
                    state.revision += 1;
                }
                Err(error)
            }
        }
    }
    pub fn write(&self, did: &str, key: &str, value: Value) -> Result<()> {
        let state = self
            .state(did)
            .ok_or_else(|| Error::Validation("设备不存在".into()))?;
        let property = state
            .spec
            .properties
            .iter()
            .find(|p| p.key() == key)
            .ok_or_else(|| Error::Validation("属性不存在".into()))?;
        property.validate(&value)?;
        if let Some(transport) = self.transports.read().unwrap().get(did).cloned() {
            transport.write(key, value)?;
            // A successful command is not a state confirmation. Poll/readback updates state.
            return Ok(());
        }
        if !self.demo {
            let client = self.client(&state.device)?;
            let mut client = client.lock().unwrap();
            if let Some(profile) = Profile::for_model(&state.device.model) {
                profile.write(&mut client, property, &value)?;
            } else {
                client.set_property(did, property.siid, property.piid, value.clone())?;
            }
        }
        if let Some(state) = self.states.write().unwrap().get_mut(did) {
            state.values.insert(key.into(), value);
            state.revision += 1;
        }
        Ok(())
    }
    pub fn remove(&self, did: &str) -> Result<()> {
        self.store.remove(did)?;
        self.transports.write().unwrap().remove(did);
        self.clients.lock().unwrap().remove(did);
        self.states.write().unwrap().remove(did);
        Ok(())
    }
    pub fn action(&self, did: &str, siid: u32, aiid: u32, inputs: Vec<Value>) -> Result<()> {
        let state = self
            .state(did)
            .ok_or_else(|| Error::Validation("设备不存在".into()))?;
        let action = state
            .spec
            .actions
            .iter()
            .find(|a| a.siid == siid && a.aiid == aiid)
            .ok_or_else(|| Error::Validation("动作不存在".into()))?;
        if inputs.len() != action.inputs.len() {
            return Err(Error::Validation("请先设置动作所需参数".into()));
        }
        for (property, value) in action.inputs.iter().zip(&inputs) {
            property.validate(value)?;
        }
        if self.demo {
            return Ok(());
        }
        if let Some(transport) = self.transports.read().unwrap().get(did).cloned() {
            return transport.action(siid, aiid, inputs);
        }
        let client = self.client(&state.device)?;
        let result = {
            let mut client = client.lock().unwrap();
            if let Some(profile) = Profile::for_model(&state.device.model) {
                profile.invoke(&mut client, siid, aiid)
            } else {
                client
                    .call(
                        "action",
                        json!({"did":did,"siid":siid,"aiid":aiid,"in":inputs}),
                    )
                    .and_then(|result| match result["code"].as_i64() {
                        Some(0 | 1) => Ok(()),
                        Some(code) => Err(Error::Device {
                            code,
                            message: "设备拒绝动作".into(),
                        }),
                        None => Err(Error::Response("动作响应缺少返回码".into())),
                    })
            }
        };
        if state.spec.kind != "vacuum" {
            return result;
        }
        if let Err(error) = result {
            // An action with an ambiguous reply is never sent again.
            let _ = self.refresh(did);
            return Err(error);
        }
        let status = crate::vacuum::status_property(&state.spec);
        let before = status.and_then(|p| {
            state
                .values
                .get(&p.key())
                .and_then(|v| crate::vacuum::status_description(p, v))
        });
        for attempt in 0..6 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(350));
            }
            let Ok(current) = self.refresh(did) else {
                break;
            };
            let description = crate::vacuum::status_property(&current.spec).and_then(|p| {
                current
                    .values
                    .get(&p.key())
                    .and_then(|v| crate::vacuum::status_description(p, v))
            });
            if crate::vacuum::confirms(&action.name, description, before) {
                return Ok(());
            }
        }
        Err(Error::Response("设备已接受动作，但尚未确认状态变化".into()))
    }
    pub fn update_discovered_hosts(
        &self,
        devices: &[crate::discovery::DiscoveredDevice],
    ) -> Result<()> {
        let mut changed = Vec::new();
        self.store.update(|config| {
            for device in &mut config.devices {
                if let Some(found) = devices
                    .iter()
                    .find(|d| d.device_id.to_string() == device.did && d.host != device.host)
                {
                    device.host = found.host;
                    changed.push(device.clone());
                }
            }
        })?;
        for device in changed {
            self.clients.lock().unwrap().remove(&device.did);
            if let Some(state) = self.states.write().unwrap().get_mut(&device.did) {
                state.device = device;
            }
        }
        Ok(())
    }
    pub fn refresh_all(&self) {
        for device in self.store.config().devices {
            let _ = self.refresh(&device.did);
        }
    }
    pub fn start_polling(self: &Arc<Self>) -> Poller {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        if !self.demo {
            let engine = Arc::downgrade(self);
            let stopped = stop.clone();
            std::thread::spawn(move || {
                let mut schedule: HashMap<String, (Instant, u32)> = HashMap::new();
                let mut discovery_due = Instant::now() + Duration::from_secs(30);
                while !stopped.load(std::sync::atomic::Ordering::Relaxed) {
                    let Some(engine) = engine.upgrade() else {
                        break;
                    };
                    if discovery_due <= Instant::now()
                        && engine
                            .store
                            .config()
                            .devices
                            .iter()
                            .any(|d| !d.host.is_unspecified())
                    {
                        if let Ok(found) = crate::discovery::discover(Duration::from_secs(2)) {
                            let _ = engine.update_discovered_hosts(&found);
                        }
                        discovery_due = Instant::now() + Duration::from_secs(300);
                    }
                    for device in engine.store.config().devices {
                        if stopped.load(std::sync::atomic::Ordering::Relaxed) {
                            break;
                        }
                        let due = schedule
                            .entry(device.did.clone())
                            .or_insert((Instant::now(), 0));
                        if due.0 <= Instant::now() {
                            let success = engine.refresh(&device.did).is_ok();
                            due.1 = if success { 0 } else { (due.1 + 1).min(12) };
                            let external_interval = engine
                                .transports
                                .read()
                                .unwrap()
                                .get(&device.did)
                                .map(|t| t.poll_seconds());
                            due.0 = Instant::now()
                                + Duration::from_secs(if let Some(seconds) = external_interval {
                                    seconds
                                } else if success {
                                    15
                                } else {
                                    (15u64 << due.1).min(86400)
                                });
                        }
                    }
                    drop(engine);
                    std::thread::sleep(Duration::from_secs(1));
                }
            });
        }
        Poller(stop)
    }
}

pub struct Poller(Arc<std::sync::atomic::AtomicBool>);
impl Drop for Poller {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
pub fn numeric(value: f64, format: &str) -> Value {
    if format == "float" {
        json!(value)
    } else {
        json!(value.round() as i64)
    }
}
