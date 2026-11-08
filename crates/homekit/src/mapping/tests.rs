use super::*;
use mijialan_core::{Specification, spec::Choice};

fn property(name: &str, format: &str, writable: bool, range: Option<[f64; 3]>) -> Property {
    Property {
        siid: 2,
        piid: 1,
        service: String::new(),
        name: name.into(),
        description: name.into(),
        description_en: name.into(),
        format: format.into(),
        readable: true,
        writable,
        unit: String::new(),
        range,
        choices: vec![],
    }
}

fn fixture(kind: &str, properties: Vec<(Property, Value)>) -> (Engine, DeviceState) {
    let engine = Engine::demo();
    let mut state = engine.state("1").unwrap();
    state.device.model = "audit.fixture".into();
    state.spec = Specification {
        kind: kind.into(),
        ..Default::default()
    };
    state.values.clear();
    for (index, (mut property, value)) in properties.into_iter().enumerate() {
        property.service = kind.into();
        property.piid = index as u32 + 1;
        state.values.insert(property.key(), value);
        state.spec.properties.push(property);
    }
    (engine, state)
}

fn snapshot(engine: &Engine, state: DeviceState) -> Database {
    build_states(engine, &mut Layout::default(), vec![state])
}

fn characteristic<'a>(database: &'a Database, service: &str, characteristic: &str) -> &'a Value {
    database.accessories[1]["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["type"] == service)
        .unwrap()["characteristics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["type"] == characteristic)
        .unwrap()
}

fn vacuum_fixture(model: &str, document: &str) -> (Engine, DeviceState) {
    let engine = Engine::demo();
    let mut state = engine.state("1").unwrap();
    state.device.model = model.into();
    state.spec = Specification::for_model(model, document).unwrap();
    state.values.clear();
    for p in &state.spec.properties {
        state.values.insert(
            p.key(),
            p.choices.first().map_or_else(
                || p.range.map_or(json!(0), |r| json!(r[0])),
                |c| c.value.clone(),
            ),
        );
    }
    (engine, state)
}

#[test]
fn vacuum_primary_switch_does_not_invent_speed_or_mode_switches() {
    for (model, document, _actions, _properties) in [
        (
            "roborock.vacuum.c1",
            include_str!("../../../core/tests/fixtures/roborock-c1.json"),
            1,
            4,
        ),
        (
            "ijai.vacuum.v19",
            include_str!("../../../core/tests/fixtures/ijai-v19.json"),
            4,
            3,
        ),
    ] {
        let (engine, state) = vacuum_fixture(model, document);
        let database = snapshot(&engine, state);
        assert_eq!(
            database
                .bindings
                .values()
                .filter(|b| matches!(b.transform, Transform::Cleaning { .. }))
                .count(),
            1
        );
        assert_eq!(
            database
                .bindings
                .values()
                .filter(|b| matches!(b.transform, Transform::Action(..)))
                .count(),
            0
        );
        assert_eq!(
            database
                .bindings
                .values()
                .filter(|b| b.key
                    == if model.starts_with("roborock") {
                        "2.2"
                    } else {
                        "2.4"
                    })
                .count(),
            0
        );
        assert!(!database.bindings.values().any(|b| matches!(
            b.transform,
            Transform::Action(2, 1) | Transform::Action(3, 1)
        )));
        assert_eq!(characteristic(&database, "49", "25")["value"], false);
    }
}

#[test]
fn vacuum_status_is_model_specific_and_stale_or_unknown_values_are_unavailable() {
    for (model, document, cleaning) in [
        (
            "roborock.vacuum.c1",
            include_str!("../../../core/tests/fixtures/roborock-c1.json"),
            true,
        ),
        (
            "ijai.vacuum.v19",
            include_str!("../../../core/tests/fixtures/ijai-v19.json"),
            false,
        ),
    ] {
        let (engine, mut state) = vacuum_fixture(model, document);
        state.values.insert("2.1".into(), json!(2));
        let database = snapshot(&engine, state.clone());
        let binding = database
            .bindings
            .values()
            .find(|b| matches!(b.transform, Transform::Cleaning { .. }))
            .unwrap();
        assert_eq!(binding.read_state(&state, false), Ok(json!(cleaning)));
        state.online = false;
        assert_eq!(binding.read_state(&state, true), Err(-70402));
        state.online = true;
        state.values.insert("2.1".into(), json!(255));
        assert_eq!(binding.read_state(&state, false), Err(-70402));
    }
}

#[test]
fn vacuum_without_required_capabilities_omits_action_switches() {
    for missing_status in [false, true] {
        let (engine, mut state) = vacuum_fixture(
            "ijai.vacuum.v19",
            include_str!("../../../core/tests/fixtures/ijai-v19.json"),
        );
        if missing_status {
            state
                .spec
                .properties
                .iter_mut()
                .find(|p| p.name == "status")
                .unwrap()
                .readable = false;
        } else {
            state.spec.actions.retain(|a| a.name != "start-charge");
        }
        let database = snapshot(&engine, state);
        assert!(
            !database
                .bindings
                .values()
                .any(|b| matches!(b.transform, Transform::Cleaning { .. }))
        );
        assert!(
            !database
                .bindings
                .values()
                .any(|b| matches!(b.transform, Transform::Action(2, 1)))
        );
    }
}

#[test]
fn invalid_device_readings_are_unavailable_instead_of_invalid_hap_values() {
    let (engine, state) = fixture(
        "light",
        vec![
            (property("on", "bool", true, None), json!("on")),
            (
                property("brightness", "uint8", true, Some([1., 100., 1.])),
                json!(255),
            ),
            (
                property(
                    "color-temperature",
                    "uint32",
                    true,
                    Some([2700., 6500., 1.]),
                ),
                json!(1),
            ),
        ],
    );
    let database = snapshot(&engine, state);
    for kind in ["25", "8", "CE"] {
        assert!(
            characteristic(&database, "43", kind)["value"].is_null(),
            "invalid reading leaked through {kind}"
        );
    }
}

#[test]
fn curtain_position_is_an_integral_percentage_of_the_actual_reading() {
    let (engine, state) = fixture(
        "curtain",
        vec![
            (
                property("target-position", "uint8", true, Some([0., 255., 1.])),
                json!(73),
            ),
            (
                property("current-position", "uint8", false, Some([0., 255., 1.])),
                json!(73),
            ),
        ],
    );
    let database = snapshot(&engine, state);
    for kind in ["7C", "6D"] {
        assert_eq!(
            characteristic(&database, "8C", kind)["value"].as_u64(),
            Some(29)
        );
    }
}

#[test]
fn humidifier_without_a_writable_threshold_uses_generic_controls() {
    for kind in ["humidifier", "dehumidifier"] {
        for target in [None, Some(false), Some(true)] {
            let mut props = vec![
                (property("on", "bool", true, None), json!(true)),
                (
                    property("relative-humidity", "float", false, Some([0., 100., 1.])),
                    json!(45),
                ),
            ];
            if let Some(writable) = target {
                props.push((
                    property("target-humidity", "uint8", writable, Some([30., 80., 1.])),
                    json!(55),
                ));
            }
            let (engine, state) = fixture(kind, props);
            let database = snapshot(&engine, state);
            let native = database.accessories[1]["services"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["type"] == "BD");
            assert_eq!(native, target == Some(true), "{kind}: target {target:?}");
            if native {
                let threshold = if kind == "humidifier" { "CA" } else { "C9" };
                assert_eq!(
                    characteristic(&database, "BD", threshold)["value"].as_f64(),
                    Some(55.)
                );
                assert_eq!(
                    characteristic(&database, "BD", "B4")["valid-values"],
                    json!([if kind == "humidifier" { 1 } else { 2 }])
                );
            }
        }
    }
}

#[test]
fn heating_and_cooling_modes_only_advertise_supported_values() {
    for (kind, modes, expected) in [
        ("heater", vec![], vec![1]),
        ("air-conditioner", vec!["Cool"], vec![2]),
        (
            "air-conditioner",
            vec!["Cool", "Auto", "Heat"],
            vec![0, 1, 2],
        ),
    ] {
        let mut props = vec![
            (property("on", "bool", true, None), json!(true)),
            (
                property("target-temperature", "float", true, Some([16., 30., 1.])),
                json!(22),
            ),
            (
                property("temperature", "float", false, Some([-40., 100., 0.1])),
                json!(21.5),
            ),
        ];
        if !modes.is_empty() {
            let mut mode = property("mode", "uint8", true, None);
            mode.choices = modes
                .into_iter()
                .enumerate()
                .map(|(n, description)| Choice {
                    value: json!(n),
                    description: description.into(),
                    description_en: description.into(),
                })
                .collect();
            props.push((mode, json!(0)));
        }
        let (engine, state) = fixture(kind, props);
        let database = snapshot(&engine, state);
        assert_eq!(
            characteristic(&database, "BC", "B2")["valid-values"],
            json!(expected)
        );
        if expected.contains(&0) || expected.contains(&1) {
            characteristic(&database, "BC", "12");
        }
        if expected.contains(&0) || expected.contains(&2) {
            characteristic(&database, "BC", "D");
        }
    }
}

#[test]
fn unmapped_enum_choices_do_not_generate_services_and_ids_remain_stable() {
    let mut mode = property("mode", "uint16", true, None);
    mode.choices = (0..150)
        .map(|n| Choice {
            value: json!(n),
            description: format!("Mode {n}"),
            description_en: format!("Mode {n}"),
        })
        .collect();
    let (engine, state) = fixture(
        "outlet",
        vec![
            (property("on", "bool", true, None), json!(true)),
            (mode, json!(0)),
        ],
    );
    let mut layout = Layout::default();
    let first = build_states(&engine, &mut layout, vec![state.clone()]);
    assert_eq!(
        first.accessories[1]["services"].as_array().unwrap().len(),
        2
    );
    let second = build_states(&engine, &mut layout, vec![state]);
    assert_eq!(first.accessories, second.accessories);
    assert_eq!(
        first.warnings.len(),
        0,
        "unmapped properties stay in the management UI"
    );
    let exposed: BTreeSet<_> = first
        .accessories
        .iter()
        .flat_map(|a| {
            a["services"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|s| s["characteristics"].as_array().unwrap())
                .map(move |c| (a["aid"].as_u64().unwrap(), c["iid"].as_u64().unwrap()))
        })
        .collect();
    assert!(first.bindings.keys().all(|id| exposed.contains(id)));
}

#[test]
fn duplicate_properties_do_not_duplicate_hap_instance_ids() {
    let (engine, mut state) = fixture(
        "light",
        vec![
            (property("on", "bool", true, None), json!(true)),
            (
                property("brightness", "uint8", true, Some([1., 100., 1.])),
                json!(50),
            ),
        ],
    );
    state.spec.properties.push(state.spec.properties[1].clone());
    let database = snapshot(&engine, state);
    let mut iids = BTreeSet::new();
    for service in database.accessories[1]["services"].as_array().unwrap() {
        assert!(iids.insert(service["iid"].as_u64().unwrap()));
        for c in service["characteristics"].as_array().unwrap() {
            assert!(
                iids.insert(c["iid"].as_u64().unwrap()),
                "duplicate characteristic IID"
            );
        }
    }
}

#[test]
fn fan_exposes_native_controls_without_switch_explosion() {
    let (engine, state) = fixture(
        "fan",
        vec![
            (property("on", "bool", true, None), json!(true)),
            (
                property("speed-level", "uint8", true, Some([1., 100., 1.])),
                json!(50),
            ),
            (
                property("horizontal-swing", "bool", true, None),
                json!(true),
            ),
            (
                property("off-delay-time", "uint16", true, Some([0., 720., 1.])),
                json!(30),
            ),
            (property("alarm", "bool", true, None), json!(true)),
        ],
    );
    let db = snapshot(&engine, state);
    let services = db.accessories[1]["services"].as_array().unwrap();
    assert_eq!(services.len(), 2);
    assert!(services.iter().any(|s| s["type"] == "B7"));
    assert!(!services.iter().any(|s| s["type"] == "49"));
    assert_eq!(characteristic(&db, "B7", "B0")["value"], 1);
    characteristic(&db, "B7", "29");
    characteristic(&db, "B7", "B6");
}
#[test]
fn outlet_temperature_is_a_separate_sensor_not_another_switch() {
    let (engine, state) = fixture(
        "outlet",
        vec![
            (property("on", "bool", true, None), json!(true)),
            (
                property("temperature", "float", false, Some([-40., 100., 0.1])),
                json!(24.5),
            ),
        ],
    );
    let db = snapshot(&engine, state);
    assert_eq!(db.accessories[1]["services"].as_array().unwrap().len(), 3);
    assert_eq!(characteristic(&db, "47", "25")["value"], true);
    assert_eq!(characteristic(&db, "8A", "11")["value"], 24.5);
}

#[test]
fn airer_maps_light_and_fan_without_fabricating_position_or_generic_switches() {
    let (engine, mut state) = fixture("airer", vec![]);
    state.device.model = "zjjsw.airer.sa006".into();
    state.spec = serde_json::from_str(include_str!("fixtures/zjjsw.airer.sa006.json")).unwrap();
    for p in &state.spec.properties {
        state.values.insert(
            p.key(),
            if p.format == "bool" {
                json!(false)
            } else {
                json!(0)
            },
        );
    }
    let db = snapshot(&engine, state);
    let services = db.accessories[1]["services"].as_array().unwrap();
    assert_eq!(services.len(), 3); // Information, light, fan.
    assert!(services.iter().any(|s| s["type"] == "43"));
    assert!(services.iter().any(|s| s["type"] == "B7"));
    assert!(
        !services
            .iter()
            .any(|s| s["type"] == "49" || s["type"] == "8C")
    );
    assert_eq!(characteristic(&db, "43", "25")["value"], json!(false));
    assert_eq!(characteristic(&db, "B7", "B0")["value"], json!(0));
}

#[test]
fn real_bath_heater_has_distinct_function_services() {
    let (engine, mut state) = fixture("bath-heater", vec![]);
    state.device.model = "opple.bhf_light.wlyb".into();
    state.spec = serde_json::from_str(include_str!("fixtures/opple.bhf_light.wlyb.json")).unwrap();
    for p in &state.spec.properties {
        state.values.insert(
            p.key(),
            if p.format == "bool" {
                json!(false)
            } else {
                json!(35)
            },
        );
    }
    let db = snapshot(&engine, state);
    let services = db.accessories[1]["services"].as_array().unwrap();
    assert_eq!(services.iter().filter(|s| s["type"] == "43").count(), 2);
    assert_eq!(services.iter().filter(|s| s["type"] == "B7").count(), 2);
    assert_eq!(services.iter().filter(|s| s["type"] == "BC").count(), 1);
    assert!(!services.iter().any(|s| s["type"] == "49"));
    assert_eq!(characteristic(&db, "BC", "B2")["valid-values"], json!([1]));
    assert_eq!(characteristic(&db, "BC", "12")["minValue"], json!(32.));
    assert_eq!(characteristic(&db, "BC", "12")["maxValue"], json!(42.));
    assert_eq!(characteristic(&db, "BC", "11")["value"], json!(35.));
}

#[test]
fn real_three_speed_fan_reports_discrete_speeds_and_off() {
    let (engine, mut state) = fixture("fan", vec![]);
    state.device.model = "dmaker.fan.p8".into();
    state.spec = serde_json::from_str(include_str!("fixtures/dmaker.fan.p8.json")).unwrap();
    for p in &state.spec.properties {
        state.values.insert(
            p.key(),
            if p.format == "bool" {
                json!(false)
            } else {
                json!(1)
            },
        );
    }
    state.values.insert("2.1".into(), json!(true));
    for (level, percent) in [(1, 33.), (2, 67.), (3, 100.)] {
        state.values.insert("2.2".into(), json!(level));
        let db = snapshot(&engine, state.clone());
        assert_eq!(characteristic(&db, "B7", "29")["value"], json!(percent));
        assert!(
            !db.accessories[1]["services"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["type"] == "49")
        );
    }
    state.values.insert("2.1".into(), json!(false));
    assert_eq!(
        characteristic(&snapshot(&engine, state), "B7", "29")["value"],
        json!(0.)
    );
}
