use mijialan_core::{Device, Error, Property, Result, Specification, engine::DeviceTransport};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Read,
    net::{IpAddr, Ipv4Addr, UdpSocket},
    process::{Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpRequest {
    pub url: String,
    #[serde(default = "get")]
    pub method: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<Value>,
}
fn get() -> String {
    "GET".into()
}
impl HttpRequest {
    pub fn validate(&self) -> Result<()> {
        let url = reqwest::Url::parse(&self.url).map_err(|_| invalid("HTTP 地址无效"))?;
        if !["http", "https"].contains(&url.scheme())
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(invalid("仅支持 HTTP/HTTPS，请用请求头配置认证"));
        }
        if !["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"].contains(&self.method.as_str()) {
            return Err(invalid("不支持的 HTTP 方法"));
        }
        for (k, v) in &self.headers {
            reqwest::header::HeaderName::from_bytes(k.as_bytes())
                .map_err(|_| invalid("请求头名称无效"))?;
            reqwest::header::HeaderValue::from_str(v).map_err(|_| invalid("请求头值无效"))?;
        }
        Ok(())
    }
    pub fn execute(&self) -> Result<Value> {
        self.validate()?;
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(6))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| invalid("HTTP 客户端初始化失败"))?;
        let method =
            reqwest::Method::from_bytes(self.method.as_bytes()).map_err(|_| invalid("方法无效"))?;
        let mut request = client.request(method, &self.url);
        for (k, v) in &self.headers {
            request = request.header(k, v);
        }
        if let Some(body) = &self.body {
            request = request.json(body);
        }
        let mut response = request
            .send()
            .map_err(|e| Error::Response(format!("HTTP 请求失败：{}", e.without_url())))?;
        if !response.status().is_success() {
            return Err(Error::Response(format!(
                "HTTP 状态 {}",
                response.status().as_u16()
            )));
        }
        let mut bytes = Vec::new();
        response.by_ref().take(1_048_577).read_to_end(&mut bytes)?;
        if bytes.len() > 1_048_576 {
            return Err(invalid("HTTP 响应超过 1 MB"));
        }
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        Ok(serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into())))
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Wol {
        mac: String,
        broadcast: Ipv4Addr,
        #[serde(default = "wol_port")]
        port: u16,
    },
    Http {
        request: HttpRequest,
    },
}
fn wol_port() -> u16 {
    9
}
impl Action {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Wol { mac, port, .. } => {
                magic_packet(mac)?;
                if *port == 0 {
                    return Err(invalid("WOL 端口不能为 0"));
                }
                Ok(())
            }
            Self::Http { request } => request.validate(),
        }
    }
    fn execute(&self) -> Result<()> {
        match self {
            Self::Wol {
                mac,
                broadcast,
                port,
            } => {
                let packet = magic_packet(mac)?;
                let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
                socket.set_broadcast(true)?;
                socket.send_to(&packet, (*broadcast, *port))?;
                Ok(())
            }
            Self::Http { request } => request.execute().map(|_| ()),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Probe {
    Ping {
        host: IpAddr,
        #[serde(default = "three")]
        failures: u32,
    },
    Http {
        request: HttpRequest,
        pointer: String,
        #[serde(default)]
        on_value: Option<Value>,
    },
    Report,
    Optimistic,
}
fn three() -> u32 {
    3
}
fn ten() -> u64 {
    10
}
fn ninety() -> u64 {
    90
}
fn switch() -> String {
    "switch".into()
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomDevice {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub room: String,
    #[serde(default = "switch")]
    pub kind: String,
    #[serde(default)]
    pub bridged: bool,
    #[serde(default)]
    pub on: Option<Action>,
    #[serde(default)]
    pub off: Option<Action>,
    pub probe: Probe,
    #[serde(default = "ten")]
    pub interval_secs: u64,
    #[serde(default = "ninety")]
    pub boot_grace_secs: u64,
}
impl CustomDevice {
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty()
            || self.id.len() > 64
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(invalid("ID 仅允许字母、数字、横线和下划线，最多 64 字符"));
        }
        if self.name.trim().is_empty() || self.name.len() > 120 {
            return Err(invalid("设备名称不能为空或超过 120 字节"));
        }
        if !["switch", "outlet", "temperature", "humidity"].contains(&self.kind.as_str()) {
            return Err(invalid("不支持的虚拟设备类型"));
        }
        if !(5..=3600).contains(&self.interval_secs) || self.boot_grace_secs > 600 {
            return Err(invalid("检测间隔应为 5–3600 秒，开机等待不超过 600 秒"));
        }
        if let Some(a) = &self.on {
            a.validate()?;
        }
        if let Some(a) = &self.off {
            a.validate()?;
        }
        match &self.probe {
            Probe::Ping { failures, .. } if !(1..=20).contains(failures) => {
                return Err(invalid("连续失败次数应为 1–20"));
            }
            Probe::Http {
                request, pointer, ..
            } => {
                request.validate()?;
                if !pointer.is_empty() && !pointer.starts_with('/') {
                    return Err(invalid("状态字段使用 JSON Pointer，例如 /data/power"));
                }
            }
            _ => {}
        }
        if ["temperature", "humidity"].contains(&self.kind.as_str())
            && matches!(self.probe, Probe::Ping { .. } | Probe::Optimistic)
        {
            return Err(invalid("数值传感器需要 HTTP 或主动上报状态"));
        }
        Ok(())
    }
    pub fn did(&self) -> String {
        format!("custom:{}", self.id)
    }
    pub fn device(&self) -> Device {
        Device {
            did: self.did(),
            name: self.name.clone(),
            model: format!("custom.{}", self.kind),
            host: Ipv4Addr::UNSPECIFIED,
            room: self.room.clone(),
            port: 54321,
            bridged: self.bridged,
        }
    }
    pub fn spec(&self) -> Specification {
        let (name, format, range) = match self.kind.as_str() {
            "temperature" => ("temperature", "float", Some([-40., 100., 0.1])),
            "humidity" => ("relative-humidity", "float", Some([0., 100., 0.1])),
            "contact" => ("contact-state", "bool", None),
            _ => ("on", "bool", None),
        };
        Specification {
            kind: self.kind.clone(),
            properties: vec![Property {
                siid: 2,
                piid: 1,
                service: self.kind.clone(),
                name: name.into(),
                description: self.name.clone(),
                description_en: self.name.clone(),
                format: format.into(),
                readable: true,
                writable: matches!(self.kind.as_str(), "switch" | "outlet"),
                unit: "none".into(),
                range,
                choices: vec![],
            }],
            ..Default::default()
        }
    }
}
struct Runtime {
    last: Option<Value>,
    failures: u32,
    boot_until: Option<Instant>,
    checked: Option<Instant>,
}
pub struct CustomTransport {
    pub config: CustomDevice,
    state: Mutex<Runtime>,
}
impl CustomTransport {
    pub fn new(config: CustomDevice) -> Self {
        Self {
            config,
            state: Mutex::new(Runtime {
                last: None,
                failures: 0,
                boot_until: None,
                checked: None,
            }),
        }
    }
    pub fn report(&self, value: Value) -> Result<()> {
        let mut p = self.config.spec().properties.remove(0);
        p.writable = true;
        p.validate(&value)?;
        self.state.lock().unwrap().last = Some(value);
        Ok(())
    }
}
impl DeviceTransport for CustomTransport {
    fn poll_seconds(&self) -> u64 {
        self.config.interval_secs
    }
    fn read(&self) -> Result<BTreeMap<String, Value>> {
        let mut state = self.state.lock().unwrap();
        if !matches!(&self.config.probe, Probe::Http { .. })
            && state
                .checked
                .is_some_and(|t| t.elapsed() < Duration::from_secs(self.config.interval_secs))
        {
            return state
                .last
                .clone()
                .map(values)
                .ok_or_else(|| invalid("等待首次状态确认"));
        }
        let value = match &self.config.probe {
            Probe::Ping { host, failures } => {
                if ping(*host)? {
                    state.failures = 0;
                    state.boot_until = None;
                    json!(true)
                } else {
                    state.failures += 1;
                    if state.boot_until.is_some_and(|t| t > Instant::now())
                        || state.failures < *failures
                    {
                        state
                            .last
                            .clone()
                            .ok_or_else(|| invalid("正在确认电脑状态"))?
                    } else {
                        json!(false)
                    }
                }
            }
            Probe::Http {
                request,
                pointer,
                on_value,
            } => {
                let response = request.execute()?;
                let v = response
                    .pointer(pointer)
                    .ok_or_else(|| invalid("响应中找不到指定状态字段"))?
                    .clone();
                if let Some(on) = on_value {
                    json!(v == *on)
                } else {
                    v
                }
            }
            Probe::Report | Probe::Optimistic => state
                .last
                .clone()
                .ok_or_else(|| invalid("等待状态上报或首次操作"))?,
        };
        let mut p = self.config.spec().properties.remove(0);
        p.writable = true;
        p.validate(&value)?;
        state.checked = Some(Instant::now());
        state.last = Some(value.clone());
        Ok(values(value))
    }
    fn write(&self, key: &str, value: Value) -> Result<()> {
        if key != "2.1" {
            return Err(invalid("未知属性"));
        }
        let on = value.as_bool().ok_or_else(|| invalid("开关需要布尔值"))?;
        let action = if on {
            &self.config.on
        } else {
            &self.config.off
        };
        action
            .as_ref()
            .ok_or_else(|| {
                invalid(if on {
                    "未配置开启动作"
                } else {
                    "未配置关闭动作；WOL 不能关机"
                })
            })?
            .execute()?;
        let mut state = self.state.lock().unwrap();
        state.checked = None;
        if on && matches!(self.config.on, Some(Action::Wol { .. })) {
            state.boot_until =
                Some(Instant::now() + Duration::from_secs(self.config.boot_grace_secs));
        }
        if matches!(self.config.probe, Probe::Optimistic) {
            state.last = Some(value);
        }
        Ok(())
    }
}
fn values(v: Value) -> BTreeMap<String, Value> {
    BTreeMap::from([("2.1".into(), v)])
}
fn invalid(message: &str) -> Error {
    Error::Validation(message.into())
}
pub fn magic_packet(mac: &str) -> Result<Vec<u8>> {
    let parts: Vec<_> = mac.split([':', '-']).collect();
    if parts.len() != 6 || parts.iter().any(|p| p.len() != 2) {
        return Err(invalid("MAC 格式应为 AA:BB:CC:DD:EE:FF"));
    }
    let bytes = parts
        .iter()
        .map(|p| u8::from_str_radix(p, 16))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| invalid("MAC 地址无效"))?;
    let mut packet = vec![255; 6];
    for _ in 0..16 {
        packet.extend_from_slice(&bytes);
    }
    Ok(packet)
}
fn ping(host: IpAddr) -> Result<bool> {
    let program = if cfg!(target_os = "macos") && host.is_ipv6() {
        "/sbin/ping6"
    } else if cfg!(target_os = "macos") {
        "/sbin/ping"
    } else {
        "/bin/ping"
    };
    let mut command = Command::new(program);
    command.args(["-n", "-c", "1"]);
    if cfg!(target_os = "macos") {
        command.args(["-W", "1000"]);
    } else {
        command.args(["-W", "1"]);
    }
    let mut child = command
        .arg(host.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| invalid("无法运行 ping，请检查系统命令和 ICMP 权限"))?;
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait()? {
            return match status.code() {
                Some(0) => Ok(true),
                Some(1) | Some(2) => Ok(false),
                _ => Err(invalid("Ping 执行异常")),
            };
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wol_packet_exact() {
        let p = magic_packet("00:11:22:33:44:55").unwrap();
        assert_eq!(p.len(), 102);
        assert_eq!(&p[..6], &[255; 6]);
        for c in p[6..].chunks(6) {
            assert_eq!(c, &[0, 17, 34, 51, 68, 85]);
        }
        assert!(magic_packet("$(touch x)").is_err());
    }
    #[test]
    fn invalid_http_rejected() {
        let r = HttpRequest {
            url: "file:///etc/passwd".into(),
            method: "GET".into(),
            headers: BTreeMap::new(),
            body: None,
        };
        assert!(r.validate().is_err());
    }
    #[test]
    fn failed_action_does_not_set_state() {
        let c: CustomDevice =
            serde_json::from_value(json!({"id":"pc","name":"PC","probe":{"type":"optimistic"}}))
                .unwrap();
        let t = CustomTransport::new(c);
        assert!(t.write("2.1", json!(false)).is_err());
        assert!(t.read().is_err());
    }
}

#[cfg(test)]
mod bridge_tests {
    use super::*;
    use mijialan_core::{Engine, Store};
    use mijialan_homekit::mapping::{self, Layout};
    use std::sync::Arc;
    #[test]
    fn custom_device_is_a_real_homekit_switch_with_stable_identity() {
        let engine = Engine::new(Arc::new(Store::memory()), false);
        let config:CustomDevice=serde_json::from_value(json!({"id":"office","name":"电脑","kind":"switch","bridged":true,"probe":{"type":"report"}})).unwrap();
        let transport = Arc::new(CustomTransport::new(config.clone()));
        engine
            .register_transport(config.device(), config.spec(), transport)
            .unwrap();
        engine.report(&config.did(), values(json!(true))).unwrap();
        let mut layout = Layout::default();
        let db = mapping::build(&engine, &mut layout);
        assert_eq!(db.accessories.len(), 2);
        let aid = db.accessories[1]["aid"].clone();
        assert!(
            db.accessories[1]["services"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["type"] == "49")
        );
        assert!(!db.bindings.is_empty());
        let second = mapping::build(&engine, &mut layout);
        assert_eq!(second.accessories[1]["aid"], aid);
        engine.set_bridged(&config.did(), false).unwrap();
        assert_eq!(mapping::build(&engine, &mut layout).accessories.len(), 1);
    }
}
