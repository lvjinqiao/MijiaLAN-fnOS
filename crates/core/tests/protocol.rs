use mijialan_core::{
    Engine, MiioClient, Specification, Store, Token,
    client::parse_json,
    packet::{self, Error},
};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, UdpSocket},
    sync::Arc,
    time::Duration,
};

const TOKEN: &str = "00112233445566778899aabbccddeeff";
const VECTOR: &str = "21310050000000000a1b2c3d123456783d32420e52de0a6a5ca0004151f14e432a36b2b7afa85bd6a9f1ed82b485a3d6c2c13a12855c97fc9a0bd7be55469c61b5393611ac2c922588d969ad8c226bde";
#[test]
fn packet_matches_python_miio() {
    let token = Token::parse(TOKEN).unwrap();
    let mut payload = br#"{"id": 1, "method": "miIO.info", "params": []}"#.to_vec();
    payload.push(0);
    assert_eq!(
        hex::encode(packet::encode(&payload, &token, 0x0a1b2c3d, 0x12345678).unwrap()),
        VECTOR
    );
    let (header, plain) = packet::decode(&hex::decode(VECTOR).unwrap(), &token).unwrap();
    assert_eq!(header.device_id, 0x0a1b2c3d);
    assert_eq!(plain, payload);
}
#[test]
fn tampering_and_wrong_tokens_are_rejected() {
    let mut packet = hex::decode(VECTOR).unwrap();
    let wrong = Token::parse("ffeeddccbbaa99887766554433221100").unwrap();
    assert!(matches!(
        packet::decode(&packet, &wrong),
        Err(Error::Checksum)
    ));
    packet[40] ^= 1;
    assert!(matches!(
        packet::decode(&packet, &Token::parse(TOKEN).unwrap()),
        Err(Error::Checksum)
    ));
}
#[test]
fn malformed_lengths_and_tokens_are_rejected() {
    for packet in [vec![], vec![0; 32], vec![0x21, 0x31, 0, 80]] {
        assert!(packet::header(&packet).is_err());
    }
    for token in ["", "0011", "gg112233445566778899aabbccddeeff"] {
        assert!(Token::parse(token).is_err());
    }
    let token = Token::parse("0011 2233 4455 6677 8899 AABB CCDD EEFF").unwrap();
    assert!(!format!("{token:?}").contains("0011"));
    assert_eq!(
        hex::encode(packet::HELLO),
        format!("21310020{}", "ff".repeat(28))
    );
}
#[test]
fn firmware_json_quirks() {
    assert_eq!(
        parse_json(b"{\"id\":1,\"result\":[]}\0junk").unwrap()["id"],
        1
    );
    assert_eq!(
        parse_json(br#"{"id":1,"result":{"a":1,,"otu_stat":2}}"#).unwrap()["result"]["otu_stat"],
        2
    );
    assert!(parse_json(b"not json").is_err());
}
#[test]
fn spec_validates_before_any_network_write() {
    let spec =
        Specification::parse(Specification::bundled("yeelink.light.lamp4").unwrap()).unwrap();
    let brightness = spec
        .properties
        .iter()
        .find(|p| p.name == "brightness")
        .unwrap();
    assert!(brightness.validate(&json!(60)).is_ok());
    assert!(brightness.validate(&json!(60.5)).is_err());
    assert!(brightness.validate(&json!(101)).is_err());
    let power = spec.properties.iter().find(|p| p.name == "on").unwrap();
    assert!(power.validate(&json!(true)).is_ok());
    assert!(power.validate(&json!(1)).is_err());
}
#[test]
fn credentials_never_appear_in_device_metadata() {
    let engine = Engine::demo();
    let serialized = serde_json::to_string(&engine.store.config()).unwrap();
    assert!(!serialized.contains(TOKEN));
    assert!(!serialized.contains("token"));
}
#[test]
fn legacy_light_writes_use_known_method_and_suffix() {
    let profile = mijialan_core::legacy::Profile::for_model("yeelink.light.lamp4").unwrap();
    let property = profile.properties.iter().find(|p| p.piid == 3).unwrap();
    let (method, args) =
        mijialan_core::legacy::write_transform(property.write.as_ref().unwrap(), &json!(4200))
            .unwrap();
    assert_eq!(method, "set_ct_abx");
    assert_eq!(
        args,
        json!([4200, "smooth", 500]).as_array().unwrap().clone()
    );
}
fn fake_device(requests: usize) -> (u16, std::thread::JoinHandle<Vec<String>>) {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let port = socket.local_addr().unwrap().port();
    let thread = std::thread::spawn(move || {
        let token = Token::parse(TOKEN).unwrap();
        let mut methods = Vec::new();
        let mut buffer = [0; 65535];
        let mut brightness = json!(45);
        while methods.len() < requests {
            let (len, from) = socket.recv_from(&mut buffer).unwrap();
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
            let method = request["method"].as_str().unwrap().to_string();
            methods.push(method.clone());
            let result=match method.as_str(){
                "miIO.info"=>json!({"model":"dmaker.fan.p28"}),
                "get_properties"=>Value::Array(request["params"].as_array().unwrap().iter().map(|p|json!({"siid":p["siid"],"piid":p["piid"],"code":0,"value":if p["piid"]==1{json!(true)}else{brightness.clone()}})).collect()),
                "set_properties"=>{brightness=request["params"][0]["value"].clone();json!([{"siid":request["params"][0]["siid"],"piid":request["params"][0]["piid"],"code":0}])},_=>json!([])
            };
            // A delayed response for another request must never be accepted.
            let stale =
                json!({"id":request["id"].as_u64().unwrap()+1,"result":"stale"}).to_string();
            socket
                .send_to(
                    &packet::encode(stale.as_bytes(), &token, 77, 900001).unwrap(),
                    from,
                )
                .unwrap();
            let reply = json!({"id":request["id"],"result":result}).to_string();
            socket
                .send_to(
                    &packet::encode(reply.as_bytes(), &token, 77, 900001).unwrap(),
                    from,
                )
                .unwrap();
        }
        methods
    });
    (port, thread)
}
#[test]
fn real_udp_handshake_read_write_and_response_matching() {
    let (port, server) = fake_device(3);
    let mut client = MiioClient::with_port(Ipv4Addr::LOCALHOST, port, Token::parse(TOKEN).unwrap());
    assert_eq!(client.info().unwrap()["model"], "dmaker.fan.p28");
    assert_eq!(
        client.get_properties("77", &[(2, 1)]).unwrap()[0]["value"],
        true
    );
    client.set_property("77", 2, 2, json!(60)).unwrap();
    assert_eq!(
        server.join().unwrap(),
        ["miIO.info", "get_properties", "set_properties"]
    );
}
#[test]
fn silent_socket_times_out() {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let mut client = MiioClient::with_port(
        Ipv4Addr::LOCALHOST,
        socket.local_addr().unwrap().port(),
        Token::parse(TOKEN).unwrap(),
    );
    client.set_timeout(Duration::from_millis(40));
    assert!(matches!(client.handshake(), Err(Error::Timeout)));
}
#[test]
fn shared_engine_changes_are_visible_to_both_interfaces() {
    let engine = Arc::new(Engine::demo());
    engine.write("1", "2.1", json!(false)).unwrap();
    assert_eq!(engine.state("1").unwrap().values["2.1"], false);
    assert!(engine.write("1", "2.2", json!(1000)).is_err());
    engine.set_bridged("1", false).unwrap();
    assert!(!engine.state("1").unwrap().device.bridged);
    assert!(
        !engine
            .store
            .config()
            .devices
            .iter()
            .find(|d| d.did == "1")
            .unwrap()
            .bridged
    );
    let store = Store::memory();
    assert!(store.token("missing").is_err());
}
