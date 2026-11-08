//! Validate the wire database independently of the mapping's own constants.
//! References: Apple HomeKitADK HAPUUID.c, HAPServiceTypes.h and HAPIPAccessory.c.
use mijialan_core::{Device, Engine, Store};
use mijialan_homekit::mapping::{self, Layout};
use serde_json::Value;
use std::{collections::BTreeSet, net::Ipv4Addr, sync::Arc};

fn all_models() -> Engine {
    let store = Arc::new(Store::memory());
    store
        .update(|config| {
            for (index, model) in [
                "chuangmi.plug.m3",
                "cuco.plug.v3",
                "zhimi.airpurifier.rma1",
                "yeelink.light.lamp4",
                "lumi.acpartner.mcn02",
                "dmaker.fan.p28",
                "chunmi.juicer.a1",
                "chunmi.health_pot.a1",
                "careli.fryer.maf07c",
                "chunmi.cooker.cmwy3",
            ]
            .into_iter()
            .enumerate()
            {
                config.devices.push(Device {
                    did: index.to_string(),
                    name: model.into(),
                    model: model.into(),
                    host: Ipv4Addr::new(192, 0, 2, 20 + index as u8),
                    room: String::new(),
                    port: 54321,
                    bridged: true,
                });
            }
        })
        .unwrap();
    Engine::new(store, true)
}

fn database(engine: &Engine) -> Value {
    mapping::build(engine, &mut Layout::default()).json(engine)
}

fn canonical_type(value: &Value) {
    let text = value.as_str().expect("HAP type must be a string");
    let number = u32::from_str_radix(text, 16).expect("Apple type must be hexadecimal");
    assert_eq!(text, format!("{number:X}"), "noncanonical HAP type");
    assert_ne!(number, 0);
}

fn validate(document: &Value) {
    let mut aids = BTreeSet::new();
    for accessory in document["accessories"].as_array().unwrap() {
        let aid = accessory["aid"].as_u64().unwrap();
        assert!(
            aid > 0 && aids.insert(aid),
            "duplicate or invalid AID {aid}"
        );
        let services = accessory["services"].as_array().unwrap();
        assert!(services.len() <= 100, "too many services for AID {aid}");
        let mut iids = BTreeSet::new();
        let mut information = 0;
        for service in services {
            canonical_type(&service["type"]);
            let iid = service["iid"].as_u64().unwrap();
            assert!(iid > 0 && iids.insert(iid), "duplicate IID {aid}.{iid}");
            let mut types = BTreeSet::new();
            for characteristic in service["characteristics"].as_array().unwrap() {
                canonical_type(&characteristic["type"]);
                let iid = characteristic["iid"].as_u64().unwrap();
                assert!(iid > 0 && iids.insert(iid), "duplicate IID {aid}.{iid}");
                assert!(types.insert(characteristic["type"].as_str().unwrap()));
                let perms = characteristic["perms"].as_array().unwrap();
                assert!(
                    [
                        "string", "bool", "float", "int", "uint8", "uint16", "uint32"
                    ]
                    .contains(&characteristic["format"].as_str().unwrap())
                );
                if let (Some(min), Some(max)) = (
                    characteristic["minValue"].as_f64(),
                    characteristic["maxValue"].as_f64(),
                ) {
                    assert!(min.is_finite() && max.is_finite() && min <= max);
                }
                if let Some(step) = characteristic["minStep"].as_f64() {
                    assert!(step.is_finite() && step >= 0.);
                }
                if let Some(valid) = characteristic["valid-values"].as_array() {
                    assert_eq!(characteristic["format"], "uint8");
                    assert!(!valid.is_empty());
                    assert!(valid.iter().all(|v| v.as_u64().is_some_and(|n| n <= 255)));
                    assert!(
                        valid
                            .windows(2)
                            .all(|p| p[0].as_u64().unwrap() < p[1].as_u64().unwrap())
                    );
                }
                assert!(!perms.is_empty());
                assert!(
                    perms
                        .iter()
                        .all(|p| ["pr", "pw", "ev"].contains(&p.as_str().unwrap()))
                );
                if !perms.iter().any(|p| p == "pr") {
                    assert!(characteristic.get("value").is_none());
                    continue;
                }
                let value = &characteristic["value"];
                assert!(
                    characteristic.get("value").is_some(),
                    "readable value must be present, even when unavailable"
                );
                // HAP serializes a failed/unavailable read as null in /accessories.
                // The service and its constraints must still remain in the database.
                if value.is_null() {
                    continue;
                }
                match characteristic["format"].as_str().unwrap() {
                    "string" => {
                        let max = characteristic["maxLen"].as_u64().unwrap_or(64) as usize;
                        assert!(
                            value.as_str().unwrap().len() <= max,
                            "overlong string {aid}.{iid}: {value}"
                        );
                    }
                    "bool" => assert!(value.is_boolean(), "invalid bool {aid}.{iid}: {value}"),
                    format => {
                        let number = value.as_f64().expect("numeric characteristic");
                        assert!(number.is_finite());
                        if format != "float" {
                            assert_eq!(number.fract(), 0., "fractional integer {aid}.{iid}");
                        }
                        if let Some(min) = characteristic["minValue"].as_f64() {
                            assert!(number >= min, "below minimum {aid}.{iid}");
                        }
                        if let Some(max) = characteristic["maxValue"].as_f64() {
                            assert!(number <= max, "above maximum {aid}.{iid}");
                        }
                        if let Some(valid) = characteristic["valid-values"].as_array() {
                            assert!(valid.iter().any(|v| v.as_f64() == Some(number)));
                        }
                        if let Some(step) = characteristic["minStep"].as_f64().filter(|s| *s > 0.) {
                            let base = characteristic["minValue"].as_f64().unwrap_or(0.);
                            let units = (number - base) / step;
                            assert!(
                                (units - units.round()).abs() < 1e-6,
                                "off-step value {aid}.{iid}"
                            );
                        }
                    }
                }
            }
            let required: &[&str] = match service["type"].as_str().unwrap() {
                "3E" => {
                    information += 1;
                    &["14", "20", "21", "23", "30", "52"]
                }
                "A2" => &["37"],
                "43" | "40" | "49" => &["25"],
                "47" => &["25", "26"],
                "BB" => &["B0", "A9", "A8"],
                "B7" => &["B0"],
                "BC" => &["B0", "11", "B1", "B2"],
                "BD" => &["B0", "10", "B3", "B4"],
                "8A" => &["11"],
                "82" => &["10"],
                "8D" => &["95"],
                "BA" => &["AC"],
                "8C" => &["6D", "7C", "72"],
                other => panic!("unreviewed service {other}"),
            };
            for required in required {
                assert!(
                    types.contains(required),
                    "missing {required} in service {}",
                    service["type"]
                );
            }
        }
        assert_eq!(information, 1);
    }
    assert!(aids.contains(&1));
    assert!(aids.len() <= 150);
}

#[test]
fn bundled_models_and_current_readings_have_valid_attribute_databases() {
    for engine in [all_models(), Engine::demo(), Engine::demo_control_center()] {
        let document = database(&engine);
        validate(&document);
        for accessory in document["accessories"].as_array().unwrap() {
            for service in accessory["services"].as_array().unwrap() {
                if service["type"] == "BC" {
                    assert!(
                        service["characteristics"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|c| c["type"] == "D")
                    );
                }
            }
        }
    }
}

#[test]
fn only_the_primary_accessory_exposes_ip_protocol_information() {
    let engine = all_models();
    let document = database(&engine);
    for accessory in document["accessories"].as_array().unwrap() {
        let protocol: Vec<_> = accessory["services"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["type"] == "A2")
            .collect();
        if accessory["aid"] == 1 {
            assert_eq!(
                protocol.len(),
                1,
                "primary bridge must expose HAP Protocol Information"
            );
            let version = protocol[0]["characteristics"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["type"] == "37")
                .unwrap();
            assert_eq!(version["value"], "1.1.0");
            assert_eq!(version["perms"], serde_json::json!(["pr"]));
        } else {
            assert!(protocol.is_empty());
        }
    }
}

#[test]
fn configured_names_support_read_write_and_notifications() {
    let document = database(&Engine::demo());
    let names: Vec<_> = document["accessories"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|a| a["services"].as_array().unwrap())
        .flat_map(|s| s["characteristics"].as_array().unwrap())
        .filter(|c| c["type"] == "E3")
        .collect();
    assert!(!names.is_empty());
    for name in names {
        for permission in ["pr", "pw", "ev"] {
            assert!(
                name["perms"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p == permission),
                "Configured Name is missing {permission}"
            );
        }
    }
}

#[test]
fn long_chinese_names_remain_valid_utf8_within_hap_byte_limits() {
    let store = Arc::new(Store::memory());
    let name = "书房台灯💡".repeat(20);
    store
        .update(|c| {
            c.devices.push(Device {
                did: "lamp".into(),
                name: name.clone(),
                model: "yeelink.light.lamp4".into(),
                host: Ipv4Addr::new(192, 0, 2, 2),
                room: String::new(),
                port: 54321,
                bridged: true,
            });
            c.homekit_names.insert("lamp:primary".into(), name.clone());
        })
        .unwrap();
    let engine = Engine::new(store.clone(), true);
    validate(&database(&engine));
    assert_eq!(
        store.config().devices[0].name,
        name,
        "do not truncate the user's source data"
    );
}
