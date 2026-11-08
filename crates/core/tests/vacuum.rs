use mijialan_core::{MiioClient, Specification, Token, legacy::Profile, vacuum};
use serde_json::{Value, json};
use std::{net::Ipv4Addr, sync::atomic::Ordering};

#[path = "support/vacuum.rs"]
mod support;
use support::{C1, TOKEN, V19, Vacuum};

#[test]
fn schema_actions_and_status_meanings_match_the_native_client() {
    let c1 = Specification::for_model("roborock.vacuum.c1", C1).unwrap();
    assert_eq!(
        c1.actions
            .iter()
            .map(|a| (a.service.as_str(), a.name.as_str()))
            .collect::<Vec<_>>(),
        [
            ("vacuum", "start-sweep"),
            ("vacuum", "stop-sweeping"),
            ("battery", "start-charge")
        ]
    );
    assert!(
        c1.compatibility
            .iter()
            .all(|c| !["a:2:1", "a:2:2", "a:3:1"].contains(&c.id.as_str()))
    );
    let mut raw: Value = serde_json::from_str(V19).unwrap();
    raw["services"][0]["actions"]
        .as_array_mut()
        .unwrap()
        .extend([
            json!({"iid":8,"type":"urn:miot-spec-v2:action:start-charge:0","in":[]}),
            json!({"iid":9,"type":"urn:miot-spec-v2:action:pause-sweeping:0","in":[]}),
            json!({"iid":10,"type":"urn:miot-spec-v2:action:start-sweep:0"}),
            json!({"iid":11,"type":"urn:miot-spec-v2:action:start-mop:0","in":[4]}),
        ]);
    let v19 = Specification::for_model("ijai.vacuum.v19", &raw.to_string()).unwrap();
    assert_eq!(v19.actions.len(), 8);
    assert!(!v19.actions.iter().any(|a| [7, 10, 11].contains(&a.aiid)));
    assert_eq!(v19.actions[0].label_for(true), "Start Cleaning");
    assert_eq!(v19.actions[0].label_for(false), "开始清扫");
    assert_eq!(
        vacuum::status_description(vacuum::status_property(&c1).unwrap(), &json!(2.0)),
        Some("Sweeping")
    );
    assert_eq!(
        vacuum::status_description(vacuum::status_property(&v19).unwrap(), &json!(2)),
        Some("Paused")
    );
    assert_eq!(
        vacuum::status_description(vacuum::status_property(&v19).unwrap(), &json!(99)),
        None
    );
    assert!(Profile::for_model("roborock.vacuum.unknown").is_none());
}

#[test]
fn confirmation_accepts_work_transitions_and_rejects_resting_or_fault_states() {
    for status in [
        "Sweeping",
        "Sweeping and Mopping",
        "Mopping",
        "RemoteClean",
        "TwoCleaning",
    ] {
        assert!(vacuum::is_cleaning(status), "{status}");
        assert!(vacuum::confirms("start-sweep", Some(status), Some("Idle")));
    }
    for status in [
        "SelfCleaning",
        "StationAssistingCleaning",
        "SweepSummoning",
        "Paused",
    ] {
        assert!(!vacuum::is_cleaning(status), "{status}");
    }
    for status in ["Map Building", "Mop Washing"] {
        assert!(vacuum::confirms("start-mop", Some(status), Some("Idle")));
        assert!(!vacuum::confirms("start-mop", Some(status), Some(status)));
    }
    for status in [
        "Idle",
        "Paused",
        "Upgrading",
        "Error",
        "Charging",
        "Go Charging",
    ] {
        assert!(
            !vacuum::confirms("start-sweep", Some(status), Some("Idle")),
            "{status}"
        );
    }
    for status in ["ChargingError", "FindChargerPause", "Exit Dock"] {
        assert!(!vacuum::is_docking(status));
    }
    assert!(vacuum::confirms(
        "start-charge",
        Some("Go Charging"),
        Some("Paused")
    ));
    assert!(!vacuum::confirms("start-charge", None, Some("Idle")));
}

#[test]
fn legacy_status_and_writes_use_explicit_roborock_methods() {
    let device = Vacuum::start("roborock.vacuum.c1", C1);
    let engine = device.engine("roborock.vacuum.c1", C1);
    let state = engine.state("77").unwrap();
    assert_eq!(state.values["2.1"], 1);
    assert_eq!(state.values["2.2"], 55.0);
    assert_eq!(state.values["3.1"], 65.0);
    assert_eq!(device.requests().len(), 1);
    assert_eq!(device.requests()[0]["method"], "get_status");
    for (raw, status) in [(15, 5), (100, 3), (8, 3), (10, 4)] {
        device.state.store(raw, Ordering::SeqCst);
        assert_eq!(engine.refresh("77").unwrap().values["2.1"], status);
    }
    engine.write("77", "2.2", json!(73)).unwrap();
    assert_eq!(device.commands()[0]["method"], "set_custom_mode");
    assert_eq!(device.commands()[0]["params"], json!([73]));
    engine.action("77", 2, 1, vec![]).unwrap();
    engine.action("77", 3, 1, vec![]).unwrap();
    assert_eq!(
        device
            .commands()
            .iter()
            .map(|r| r["method"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["set_custom_mode", "app_start", "app_charge"]
    );
}

#[test]
fn first_generation_docking_pauses_once_and_aborts_on_ambiguous_reply() {
    for reject in [false, true] {
        let device = Vacuum::start("rockrobo.vacuum.v1", C1);
        let profile = Profile::for_model("rockrobo.vacuum.v1").unwrap();
        if reject {
            *device.reject.lock().unwrap() = Some("app_pause".into());
        }
        let mut client = MiioClient::with_port(
            Ipv4Addr::LOCALHOST,
            device.port,
            Token::parse(TOKEN).unwrap(),
        );
        assert_eq!(profile.invoke(&mut client, 3, 1).is_ok(), !reject);
        let commands = device.commands();
        assert_eq!(commands.len(), if reject { 1 } else { 2 });
        assert_eq!(commands[0]["method"], "app_pause");
        assert_eq!(commands[0]["params"], json!([]));
        if !reject {
            assert_eq!(commands[1]["method"], "app_charge");
        }
    }
}

#[test]
fn acknowledgement_alone_is_not_success_and_actions_are_never_retried() {
    let device = Vacuum::start("ijai.vacuum.v19", V19);
    let engine = device.engine("ijai.vacuum.v19", V19);
    device.hold.store(true, Ordering::SeqCst);
    device.ack_code.store(1, Ordering::SeqCst);
    assert!(engine.action("77", 2, 1, vec![]).is_err());
    assert_eq!(device.commands().len(), 1);
    assert_eq!(engine.state("77").unwrap().values["2.1"], 1);
    assert_eq!(
        device
            .requests()
            .iter()
            .filter(|r| r["method"] == "get_properties")
            .count(),
        7
    );
    device.hold.store(false, Ordering::SeqCst);
    device.lag_reads.store(2, Ordering::SeqCst);
    engine.action("77", 2, 1, vec![]).unwrap();
    assert_eq!(device.commands().len(), 2);
    assert_eq!(
        device.commands()[1]["params"],
        json!({"did":"77","siid":2,"aiid":1,"in":[]})
    );
    assert_eq!(engine.state("77").unwrap().values["2.1"], 5);
    assert!(engine.action("77", 2, 7, vec![]).is_err());
    assert_eq!(device.commands().len(), 2);
}
