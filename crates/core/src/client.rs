use crate::packet::{self, Error, Result, Token};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddrV4, UdpSocket},
    time::{Duration, Instant},
};

/// Mutable ownership serializes requests to one device. Never execute this blocking client on the UI thread.
pub struct MiioClient {
    endpoint: SocketAddrV4,
    token: Token,
    timeout: Duration,
    clock: Option<(packet::Header, Instant)>,
    next_id: u32,
}

impl MiioClient {
    pub fn new(host: Ipv4Addr, token: Token) -> Self {
        Self::with_port(host, packet::PORT, token)
    }
    pub fn with_port(host: Ipv4Addr, port: u16, token: Token) -> Self {
        Self {
            endpoint: SocketAddrV4::new(host, port),
            token,
            timeout: Duration::from_secs(2),
            clock: None,
            next_id: 100,
        }
    }
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    fn socket(&self) -> Result<UdpSocket> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        socket.set_write_timeout(Some(self.timeout))?;
        socket.connect(self.endpoint)?;
        Ok(socket)
    }

    pub fn handshake(&mut self) -> Result<packet::Header> {
        let socket = self.socket()?;
        socket.send(&packet::HELLO)?;
        let deadline = Instant::now() + self.timeout;
        let mut buffer = [0u8; 65535];
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Timeout);
            }
            socket.set_read_timeout(Some(remaining))?;
            let len = socket.recv(&mut buffer).map_err(network_error)?;
            if len != packet::HEADER_LEN {
                continue;
            }
            if let Ok(header) = packet::header(&buffer[..len]) {
                if header.device_id == 0 || header.device_id == u32::MAX {
                    continue;
                }
                self.clock = Some((header, Instant::now()));
                return Ok(header);
            }
        }
    }

    pub fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        // Read retries are safe. Ambiguous writes/actions are surfaced rather than executed twice.
        let attempts = if matches!(
            method,
            "miIO.info" | "get_properties" | "get_prop" | "get_status"
        ) {
            2
        } else {
            1
        };
        let mut hello_answered = false;
        for attempt in 0..attempts {
            if self
                .clock
                .is_none_or(|(_, at)| at.elapsed() > Duration::from_secs(120))
            {
                self.handshake()?;
                hello_answered = true;
            }
            match self.send(method, params.clone()) {
                Err(Error::Timeout) => {
                    self.clock = None;
                    if attempt + 1 == attempts {
                        return Err(if hello_answered {
                            Error::NoEncryptedReply
                        } else {
                            Error::Timeout
                        });
                    }
                }
                result => return result,
            }
        }
        Err(Error::Timeout)
    }

    fn send(&mut self, method: &str, params: Value) -> Result<Value> {
        let (clock, at) = self.clock.ok_or(Error::Timeout)?;
        self.next_id = self.next_id % 9999 + 1;
        let id = self.next_id;
        let mut body = serde_json::to_vec(&json!({"id": id, "method": method, "params": params}))
            .map_err(|_| Error::Response("无法编码请求".into()))?;
        body.push(0);
        let stamp = clock
            .stamp
            .wrapping_add(at.elapsed().as_secs() as u32)
            .wrapping_add(1);
        let request = packet::encode(&body, &self.token, clock.device_id, stamp)?;
        let socket = self.socket()?;
        socket.send(&request)?;
        let deadline = Instant::now() + self.timeout;
        let mut buffer = [0u8; 65535];
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Timeout);
            }
            socket.set_read_timeout(Some(remaining))?;
            let length = socket.recv(&mut buffer).map_err(network_error)?;
            if length <= packet::HEADER_LEN {
                continue;
            }
            let (header, payload) = packet::decode(&buffer[..length], &self.token)?;
            if header.device_id != clock.device_id {
                continue;
            }
            let json = parse_json(&payload)?;
            if json["id"].as_u64() != Some(id as u64) {
                continue;
            }
            self.clock = Some((header, Instant::now()));
            if let Some(error) = json.get("error") {
                return Err(Error::Device {
                    code: error["code"].as_i64().unwrap_or(-1),
                    message: error["message"].as_str().unwrap_or("未知设备错误").into(),
                });
            }
            return json
                .get("result")
                .cloned()
                .ok_or_else(|| Error::Response("响应缺少 result".into()));
        }
    }

    pub fn info(&mut self) -> Result<Value> {
        self.call("miIO.info", json!([]))
    }

    pub fn get_properties(&mut self, did: &str, properties: &[(u32, u32)]) -> Result<Vec<Value>> {
        let mut result = Vec::new();
        for chunk in properties.chunks(15) {
            let params: Vec<_> = chunk
                .iter()
                .map(|(siid, piid)| json!({"did": did, "siid": siid, "piid": piid}))
                .collect();
            let response = self.call("get_properties", json!(params))?;
            result.extend(
                response
                    .as_array()
                    .ok_or_else(|| Error::Response("属性结果不是数组".into()))?
                    .iter()
                    .cloned(),
            );
        }
        Ok(result)
    }

    pub fn set_property(&mut self, did: &str, siid: u32, piid: u32, value: Value) -> Result<()> {
        let response = self.call(
            "set_properties",
            json!([{"did": did, "siid": siid, "piid": piid, "value": value}]),
        )?;
        let items = response
            .as_array()
            .filter(|v| v.len() == 1)
            .ok_or_else(|| Error::Response("写入返回结果数量不符".into()))?;
        if items[0]["siid"].as_u64() != Some(siid as u64)
            || items[0]["piid"].as_u64() != Some(piid as u64)
        {
            return Err(Error::Response("写入结果与请求属性不匹配".into()));
        }
        match items[0]["code"].as_i64() {
            Some(0 | 1) => Ok(()),
            Some(code) => Err(Error::Device {
                code,
                message: "设备拒绝写入".into(),
            }),
            None => Err(Error::Response("写入响应缺少返回码".into())),
        }
    }
}

fn network_error(error: std::io::Error) -> Error {
    if matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    ) {
        Error::Timeout
    } else {
        Error::Io(error)
    }
}

pub fn parse_json(payload: &[u8]) -> Result<Value> {
    let end = payload
        .iter()
        .position(|b| *b == 0)
        .unwrap_or(payload.len());
    let text = String::from_utf8_lossy(&payload[..end]);
    let repaired = text
        .replace(",,\"otu_stat\"", ",\"otu_stat\"")
        .replace("\"value\":00", "\"value\":0");
    let value: Value = serde_json::from_str(&text)
        .or_else(|_| serde_json::from_str(&repaired))
        .map_err(|_| Error::Response("设备返回了无效 JSON".into()))?;
    if !value.is_object() {
        return Err(Error::Response("设备响应不是对象".into()));
    }
    Ok(value)
}
