use mijialan_core::{Specification, compatibility::Reason, legacy::Profile};
use serde_json::json;
use std::collections::BTreeSet;

#[test]
fn lamp_report_matches_original_order_and_bilingual_guidance() {
    let spec = Specification::for_model(
        "yeelink.light.lamp4",
        Specification::bundled("yeelink.light.lamp4").unwrap(),
    )
    .unwrap();
    let entries = &spec.compatibility;
    assert_eq!(
        entries
            .iter()
            .map(|c| (c.id.as_str(), c.title(true), c.reason))
            .collect::<Vec<_>>(),
        vec![
            ("p:2:4", "Light Scene", Reason::LegacyMapping),
            ("a:2:1", "Toggle Power", Reason::PowerSwitch),
            ("p:3:1", "Adjust Brightness", Reason::BrightnessSlider),
            (
                "p:3:2",
                "Adjust Color Temperature",
                Reason::ColorTemperatureControl
            ),
            (
                "p:3:3",
                "Alexa Color Temperature",
                Reason::ColorTemperatureControl
            ),
        ]
    );
    assert_eq!(
        entries.iter().map(|c| c.title(false)).collect::<Vec<_>>(),
        vec![
            "灯光场景",
            "切换开关状态",
            "增减亮度",
            "增减色温",
            "Alexa 色温调节",
        ]
    );
    assert_eq!(
        entries[0].explanation(true),
        "LAN control is not supported in this version yet."
    );
    assert_eq!(entries[1].explanation(false), "请使用顶部的电源开关");
    assert_eq!(entries[2].explanation(true), "Use the brightness slider.");
    assert_eq!(entries[4].explanation(false), "请使用色温滑条或数值输入");
    assert_eq!(spec.properties.iter().filter(|p| p.writable).count(), 3);
    assert!(spec.actions.is_empty());
}

#[test]
fn guidance_never_points_to_a_read_only_control() {
    for writable in [false, true] {
        let access = if writable {
            json!(["read", "write"])
        } else {
            json!(["read"])
        };
        let document = json!({"type":"urn:miot-spec-v2:device:light:0000", "services":[{
            "iid":2,"type":"urn:miot-spec-v2:service:light:0000",
            "properties":[
                {"iid":1,"type":"urn:miot-spec-v2:property:on:0000","format":"bool","access":access},
                {"iid":2,"type":"urn:miot-spec-v2:property:brightness:0000","format":"uint8","access":access,"value-range":[1,100,1]},
                {"iid":3,"type":"urn:miot-spec-v2:property:color-temperature:0000","format":"uint32","access":access,"value-range":[2600,5000,1]},
                {"iid":4,"type":"urn:miot-spec-v2:property:brightness-delta:0000","format":"int32","access":["write"]},
                {"iid":5,"type":"urn:miot-spec-v2:property:ct-delta:0000","format":"int32","access":["write"]}
            ],"actions":[{"iid":1,"type":"urn:miot-spec-v2:action:toggle:0000","in":[]}]
        }]});
        let spec = Specification::for_model("vendor.light.fixture", &document.to_string()).unwrap();
        assert_eq!(spec.compatibility.len(), 3);
        assert_eq!(
            spec.compatibility[0].reason,
            if writable {
                Reason::BrightnessSlider
            } else {
                Reason::IncompleteParameters
            }
        );
        assert_eq!(
            spec.compatibility[1].reason,
            if writable {
                Reason::ColorTemperatureControl
            } else {
                Reason::IncompleteParameters
            }
        );
        assert_eq!(
            spec.compatibility[2].reason,
            if writable {
                Reason::PowerSwitch
            } else {
                Reason::UnavailableOperation
            }
        );
    }
}

#[test]
fn unbundled_specs_report_hidden_actions_and_incomplete_arguments() {
    let document = json!({"type":"urn:miot-spec-v2:device:cooker:0000", "services":[
        {"iid":1,"type":"urn:miot-spec-v2:service:device-information:0000","actions":[
            {"iid":1,"type":"urn:miot-spec-v2:action:maintenance:0000","in":[]}]},
        {"iid":2,"type":"urn:miot-spec-v2:service:cooker:0000","properties":[
            {"iid":1,"type":"urn:miot-spec-v2:property:on:0000","format":"bool","access":["read","write"]},
            {"iid":2,"type":"urn:miot-spec-v2:property:target-time:0000","format":"uint32","access":["read","write"],"value-range":[1,60,1]},
            {"iid":3,"type":"urn:miot-spec-v2:property:vendor-setting:0000","format":"string","access":["write"],"description":"Vendor setting","comment":"厂商设置"}
        ],"actions":[
            {"iid":1,"type":"urn:miot-spec-v2:action:start-cook:0000","in":[2]},
            {"iid":2,"type":"urn:miot-spec-v2:action:reset:0000","in":[]},
            {"iid":3,"type":"urn:miot-spec-v2:action:pause:0000","in":[99]}
        ]}
    ]});
    let spec = Specification::for_model("vendor.cooker.fixture", &document.to_string()).unwrap();
    assert_eq!(spec.actions.len(), 1);
    assert_eq!(spec.actions[0].name, "start-cook");
    assert_eq!(
        spec.compatibility
            .iter()
            .map(|c| (c.id.as_str(), c.reason))
            .collect::<Vec<_>>(),
        vec![
            ("p:2:3", Reason::IncompleteParameters),
            ("a:2:2", Reason::UnavailableOperation),
            ("a:2:3", Reason::IncompleteParameters),
        ]
    );
    assert_eq!(spec.compatibility[0].title(true), "Vendor setting");
    assert_eq!(spec.compatibility[0].title(false), "厂商设置");
    assert!(
        !spec
            .properties
            .iter()
            .find(|p| p.piid == 3)
            .unwrap()
            .writable
    );
}

#[test]
fn reports_preserve_existing_controls_for_every_bundled_model() {
    for model in [
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
    ] {
        let text = Specification::bundled(model).unwrap();
        let mut previous = Specification::parse(text).unwrap();
        if let Some(profile) = Profile::for_model(model) {
            profile.adapt_spec(&mut previous);
        }
        let current = Specification::for_model(model, text).unwrap();
        assert_eq!(
            serde_json::to_value((&previous.properties, &previous.actions)).unwrap(),
            serde_json::to_value((&current.properties, &current.actions)).unwrap(),
            "{model}"
        );
        let unique: BTreeSet<_> = current.compatibility.iter().map(|c| &c.id).collect();
        assert_eq!(unique.len(), current.compatibility.len(), "{model}");
        assert!(
            current
                .compatibility
                .iter()
                .all(|c| c.service != "device-information")
        );
    }
}
