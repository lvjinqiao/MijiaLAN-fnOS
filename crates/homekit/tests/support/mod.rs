use mijialan_core::{Specification, Token, client::parse_json, engine::numeric, packet};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, UdpSocket},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    },
    time::Duration,
};

pub const TOKEN: &str = "00112233445566778899aabbccddeeff";

/// A loopback-only real miIO endpoint; no physical device or credential store.
pub struct MiotDevice {
    pub port: u16,
    pub phase: Arc<AtomicU8>,
    pub writes: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl MiotDevice {
    pub fn start() -> Self {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let port = socket.local_addr().unwrap().port();
        let phase = Arc::new(AtomicU8::new(0));
        let writes = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (worker_phase, worker_writes, worker_stop) =
            (phase.clone(), writes.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            let token = Token::parse(TOKEN).unwrap();
            let spec = Specification::for_model(
                "zhimi.airpurifier.rma1",
                Specification::bundled("zhimi.airpurifier.rma1").unwrap(),
            )
            .unwrap();
            let mut buffer = [0; 65535];
            while !worker_stop.load(Ordering::Relaxed) {
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
                let (_, payload) = packet::decode(&buffer[..len], &token).unwrap();
                let request = parse_json(&payload).unwrap();
                let phase = worker_phase.load(Ordering::SeqCst);
                let result = match request["method"].as_str().unwrap() {
                    "get_properties" => Value::Array(request["params"].as_array().unwrap().iter().map(|query| {
                        let property = spec.properties.iter().find(|p| query["siid"] == p.siid && query["piid"] == p.piid).unwrap();
                        let value = match property.name.as_str() {
                            "on" if phase == 1 => json!("on"),
                            "relative-humidity" if phase == 1 => json!(123),
                            "temperature" if phase == 1 => json!(-3276.8),
                            "relative-humidity" => json!(if phase == 2 {55} else {45}),
                            "temperature" => json!(21.56),
                            _ if property.format == "bool" => json!(property.name == "on"),
                            _ if !property.choices.is_empty() => property.choices[0].value.clone(),
                            _ => property.range.map_or(json!(0), |r| numeric(r[0], &property.format)),
                        };
                        json!({"siid":property.siid,"piid":property.piid,"code":0,"value":value})
                    }).collect()),
                    "set_properties" => {
                        worker_writes.fetch_add(1, Ordering::SeqCst);
                        Value::Array(request["params"].as_array().unwrap().iter().map(|p| json!({"siid":p["siid"],"piid":p["piid"],"code":0})).collect())
                    }
                    method => panic!("unexpected fixture method {method}"),
                };
                let body = json!({"id":request["id"],"result":result}).to_string();
                socket
                    .send_to(
                        &packet::encode(body.as_bytes(), &token, 77, 900001).unwrap(),
                        from,
                    )
                    .unwrap();
            }
        });
        Self {
            port,
            phase,
            writes,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for MiotDevice {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
