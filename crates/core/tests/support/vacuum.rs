use mijialan_core::{
    Device, Engine, Specification, Store, Token, client::parse_json, legacy::Profile, packet,
};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, UdpSocket},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering},
    },
    time::Duration,
};

pub const C1: &str = include_str!("../fixtures/roborock-c1.json");
pub const V19: &str = include_str!("../fixtures/ijai-v19.json");
pub const TOKEN: &str = "00112233445566778899aabbccddeeff";

/// A real encrypted miIO endpoint on loopback, using public fixture credentials.
/// Both integration suites share it; no physical device or OS credential store.
pub struct Vacuum {
    pub port: u16,
    pub state: Arc<AtomicI64>,
    pub hold: Arc<AtomicBool>,
    pub lag_reads: Arc<AtomicUsize>,
    pub reject: Arc<Mutex<Option<String>>>,
    pub ack_code: Arc<AtomicI64>,
    requests: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Vacuum {
    pub fn start(model: &str, document: &str) -> Self {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_millis(30)))
            .unwrap();
        let mut fixture = Self {
            port: socket.local_addr().unwrap().port(),
            state: Arc::new(AtomicI64::new(if Profile::for_model(model).is_some() {
                3
            } else {
                1
            })),
            hold: Arc::new(AtomicBool::new(false)),
            lag_reads: Arc::new(AtomicUsize::new(0)),
            reject: Arc::new(Mutex::new(None)),
            ack_code: Arc::new(AtomicI64::new(0)),
            requests: Arc::new(Mutex::new(Vec::new())),
            stop: Arc::new(AtomicBool::new(false)),
            thread: None,
        };
        let (state, hold, lag, reject, ack, requests, stop) = (
            fixture.state.clone(),
            fixture.hold.clone(),
            fixture.lag_reads.clone(),
            fixture.reject.clone(),
            fixture.ack_code.clone(),
            fixture.requests.clone(),
            fixture.stop.clone(),
        );
        let spec = Specification::for_model(model, document).unwrap();
        fixture.thread = Some(std::thread::spawn(move || {
            let token = Token::parse(TOKEN).unwrap();
            let mut pending: Option<(i64, usize)> = None;
            let mut buffer = [0; 65535];
            while !stop.load(Ordering::Relaxed) {
                let (len, from) = match socket.recv_from(&mut buffer) {
                    Ok(v) => v,
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                        ) =>
                    {
                        continue;
                    }
                    Err(_) => break,
                };
                if buffer[..len] == packet::HELLO {
                    let mut hello = vec![0x21, 0x31, 0, 32, 0, 0, 0, 0];
                    hello.extend_from_slice(&77u32.to_be_bytes());
                    hello.extend_from_slice(&900000u32.to_be_bytes());
                    hello.extend_from_slice(&[0; 16]);
                    socket.send_to(&hello, from).unwrap();
                    continue;
                }
                let (_, body) = packet::decode(&buffer[..len], &token).unwrap();
                let request = parse_json(&body).unwrap();
                requests.lock().unwrap().push(request.clone());
                let method = request["method"].as_str().unwrap();
                if matches!(method, "get_properties" | "get_status")
                    && let Some((target, remaining)) = pending.as_mut()
                {
                    if *remaining == 0 {
                        state.store(*target, Ordering::SeqCst);
                        pending = None;
                    } else {
                        *remaining -= 1;
                    }
                }
                let current = state.load(Ordering::SeqCst);
                let denied = reject.lock().unwrap().as_deref() == Some(method);
                let target = match method {
                    "app_start" => Some(5),
                    "app_charge" => Some(6),
                    "app_pause" => Some(10),
                    "app_stop" => Some(3),
                    "action" => match (
                        request["params"]["siid"].as_u64(),
                        request["params"]["aiid"].as_u64(),
                    ) {
                        (Some(3), Some(1)) => Some(3),
                        (Some(2), Some(2)) => Some(2),
                        (Some(2), _) => Some(5),
                        _ => None,
                    },
                    _ => None,
                };
                if !denied
                    && !hold.load(Ordering::SeqCst)
                    && let Some(target) = target
                {
                    pending = Some((target, lag.load(Ordering::SeqCst)));
                }
                let result = if denied {
                    json!(["busy"])
                } else {
                    match method {
                        "get_status" => json!([{"state":current,"fan_power":55,"battery":65}]),
                        "get_properties" => Value::Array(
                            request["params"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|query| {
                                    let p = spec
                                        .properties
                                        .iter()
                                        .find(|p| {
                                            query["siid"] == p.siid && query["piid"] == p.piid
                                        })
                                        .unwrap();
                                    let value = match p.name.as_str() {
                                        "status" => json!(current),
                                        "battery-level" => json!(65),
                                        "speed-level" => json!(55),
                                        _ if !p.choices.is_empty() => p.choices[0].value.clone(),
                                        _ => p.range.map_or(json!(0), |r| json!(r[0])),
                                    };
                                    json!({"siid":p.siid,"piid":p.piid,"code":0,"value":value})
                                })
                                .collect(),
                        ),
                        "action" => json!({"code":ack.load(Ordering::SeqCst)}),
                        "app_start" | "app_stop" | "app_pause" | "app_charge"
                        | "set_custom_mode" => json!(["ok"]),
                        "set_properties" => Value::Array(
                            request["params"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|p| json!({"siid":p["siid"],"piid":p["piid"],"code":0}))
                                .collect(),
                        ),
                        _ => panic!("unexpected vacuum request: {request}"),
                    }
                };
                let reply = json!({"id":request["id"],"result":result}).to_string();
                socket
                    .send_to(
                        &packet::encode(reply.as_bytes(), &token, 77, 900001).unwrap(),
                        from,
                    )
                    .unwrap();
            }
        }));
        fixture
    }

    pub fn engine(&self, model: &str, document: &str) -> Arc<Engine> {
        let store = Arc::new(Store::memory());
        store.cache_spec(model, document).unwrap();
        store
            .put(
                Device {
                    did: "77".into(),
                    name: "Fixture Vacuum".into(),
                    model: model.into(),
                    host: Ipv4Addr::LOCALHOST,
                    room: String::new(),
                    port: self.port,
                    bridged: true,
                },
                &Token::parse(TOKEN).unwrap(),
            )
            .unwrap();
        let engine = Arc::new(Engine::new(store, false));
        assert!(engine.refresh("77").unwrap().online);
        engine
    }

    pub fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }

    pub fn commands(&self) -> Vec<Value> {
        self.requests()
            .into_iter()
            .filter(|r| !matches!(r["method"].as_str(), Some("get_properties" | "get_status")))
            .collect()
    }
}

impl Drop for Vacuum {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
