use crate::{Error, MiioClient, Property, Result, Specification};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Debug, Clone, Deserialize)]
pub struct LegacyProperty {
    pub siid: u32,
    pub piid: u32,
    pub name: String,
    pub read: Value,
    pub write: Option<Value>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    pub properties: Vec<LegacyProperty>,
    pub chunk: Option<usize>,
    #[serde(rename = "ignoreResult", default)]
    pub ignore_result: bool,
    #[serde(rename = "statusMethod", default)]
    pub status_method: Option<String>,
    #[serde(default)]
    pub actions: Vec<LegacyAction>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct LegacyAction {
    pub siid: u32,
    pub aiid: u32,
    pub methods: Vec<String>,
}

impl Profile {
    pub fn for_model(model: &str) -> Option<Self> {
        static PROFILES: OnceLock<BTreeMap<String, Profile>> = OnceLock::new();
        let profiles = PROFILES.get_or_init(|| {
            let document: Value =
                serde_json::from_str(include_str!("../../../resources/legacy-miio.json"))
                    .expect("bundled legacy map");
            let mut profiles: BTreeMap<String, Profile> =
                serde_json::from_value(document["models"].clone()).expect("bundled profiles");
            profiles.insert("rockrobo.vacuum.v1".into(), Self::roborock(true));
            for model in [
                "roborock.vacuum.c1",
                "roborock.vacuum.e2",
                "roborock.vacuum.s5",
            ] {
                profiles.insert(model.into(), Self::roborock(false));
            }
            profiles
        });
        profiles.get(model).cloned()
    }
    pub fn adapt_spec(&self, spec: &mut Specification) {
        for property in &mut spec.properties {
            let mapped = self
                .properties
                .iter()
                .find(|p| p.siid == property.siid && p.piid == property.piid);
            property.writable &= mapped.is_some_and(|p| p.write.is_some());
            property.readable &= mapped.is_some();
        }
        spec.actions.retain(|action| {
            action.inputs.is_empty()
                && self.actions.iter().any(|a| {
                    a.siid == action.siid && a.aiid == action.aiid && !a.methods.is_empty()
                })
        });
    }
    pub fn read(
        &self,
        client: &mut MiioClient,
        spec: &Specification,
    ) -> Result<BTreeMap<String, Value>> {
        let mapped: Vec<_> = self
            .properties
            .iter()
            .filter(|p| {
                spec.properties
                    .iter()
                    .any(|s| s.readable && s.siid == p.siid && s.piid == p.piid)
            })
            .collect();
        let mut names = Vec::new();
        for p in &mapped {
            if !names.contains(&p.name) {
                names.push(p.name.clone());
            }
        }
        let mut raw = BTreeMap::new();
        if let Some(method) = &self.status_method {
            let result = client.call(method, json!([]))?;
            let object = result
                .as_array()
                .and_then(|items| items.first())
                .unwrap_or(&result)
                .as_object()
                .ok_or_else(|| Error::Response("旧协议状态结果不是对象".into()))?;
            for name in &names {
                if let Some(value) = object.get(name) {
                    raw.insert(name.clone(), value.clone());
                }
            }
        } else {
            for chunk in names.chunks(self.chunk.filter(|n| *n > 0).unwrap_or(15)) {
                let values = client.call("get_prop", json!(chunk))?;
                let values = values
                    .as_array()
                    .ok_or_else(|| Error::Response("旧协议属性结果不是数组".into()))?;
                for (name, value) in chunk.iter().zip(values) {
                    raw.insert(name.clone(), value.clone());
                }
            }
        }
        let mut result = BTreeMap::new();
        for p in mapped {
            if let Some(raw) = raw.get(&p.name)
                && let Some(spec) = spec
                    .properties
                    .iter()
                    .find(|s| s.siid == p.siid && s.piid == p.piid)
                && let Some(value) = read_transform(&p.read, raw, &spec.format)
            {
                result.insert(spec.key(), value);
            }
        }
        Ok(result)
    }
    pub fn invoke(&self, client: &mut MiioClient, siid: u32, aiid: u32) -> Result<()> {
        let action = self
            .actions
            .iter()
            .find(|a| a.siid == siid && a.aiid == aiid && !a.methods.is_empty())
            .ok_or_else(|| Error::Validation("旧协议没有已确认的动作映射".into()))?;
        for method in &action.methods {
            let result = client.call(method, json!([]))?;
            if !confirmed(&result) {
                return Err(Error::Response("设备未确认旧协议动作".into()));
            }
        }
        Ok(())
    }
    pub fn write(&self, client: &mut MiioClient, property: &Property, value: &Value) -> Result<()> {
        let p = self
            .properties
            .iter()
            .find(|p| p.siid == property.siid && p.piid == property.piid)
            .ok_or_else(|| Error::Validation("旧协议没有对应映射".into()))?;
        let write = p
            .write
            .as_ref()
            .ok_or_else(|| Error::Validation("旧协议属性只读".into()))?;
        let (method, args) = write_transform(write, value)
            .ok_or_else(|| Error::Validation("无法安全转换此参数".into()))?;
        let result = client.call(&method, json!(args))?;
        if self.ignore_result || confirmed(&result) {
            Ok(())
        } else {
            Err(Error::Response("设备未确认旧协议写入".into()))
        }
    }
    fn roborock(pauses_before_dock: bool) -> Self {
        // Native MijiaLAN's explicit adapter for the first-generation robot and
        // C1/E2/S5. These protocol values are translated to the spec's status list.
        let status = json!({"kind":"values","default":1,"table":{
            "1":2,"2":7,"3":1,"4":6,"5":2,"6":5,"7":6,"8":3,"9":7,
            "10":4,"11":2,"12":7,"13":7,"15":5,"16":6,"17":2,"18":2,"100":3
        }});
        let property = |siid, piid, name: &str, read, write| LegacyProperty {
            siid,
            piid,
            name: name.into(),
            read,
            write,
        };
        let action = |siid, aiid, methods: Vec<&str>| LegacyAction {
            siid,
            aiid,
            methods: methods.into_iter().map(str::to_owned).collect(),
        };
        Self {
            properties: vec![
                property(2, 1, "state", status, None),
                property(
                    2,
                    2,
                    "fan_power",
                    json!({"kind":"number"}),
                    Some(json!({"kind":"value","method":"set_custom_mode"})),
                ),
                property(3, 1, "battery", json!({"kind":"number"}), None),
                property(
                    3,
                    2,
                    "state",
                    json!({"kind":"values","table":{"8":1},"default":2}),
                    None,
                ),
            ],
            chunk: None,
            ignore_result: false,
            status_method: Some("get_status".into()),
            actions: vec![
                action(2, 1, vec!["app_start"]),
                action(2, 2, vec!["app_stop"]),
                action(
                    3,
                    1,
                    if pauses_before_dock {
                        vec!["app_pause", "app_charge"]
                    } else {
                        vec!["app_charge"]
                    },
                ),
            ],
        }
    }
}

fn confirmed(result: &Value) -> bool {
    result == &json!(["ok"])
        || result == &json!([0])
        || result == &json!([true])
        || result == &json!("ok")
}

fn number(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.parse().ok())
}
fn boolean(v: &Value) -> Option<bool> {
    v.as_bool()
        .or_else(|| number(v).map(|n| n != 0.))
        .or_else(|| match v.as_str()?.to_ascii_lowercase().as_str() {
            "on" | "true" | "yes" | "enable" => Some(true),
            "off" | "false" | "no" | "disable" => Some(false),
            _ => None,
        })
}
fn same(a: &Value, b: &Value) -> bool {
    a == b || number(a).zip(number(b)).is_some_and(|(a, b)| a == b)
}
pub fn read_transform(t: &Value, raw: &Value, format: &str) -> Option<Value> {
    match t["kind"].as_str()? {
        "onOff" | "boolean" => boolean(raw).map(Value::Bool),
        "auto" if format == "bool" => boolean(raw).map(Value::Bool),
        "auto" if format != "string" => number(raw).map(|n| json!(n)),
        "auto" => Some(raw.clone()),
        "number" => {
            let value = if let Some(strip) = t["strip"].as_str() {
                Value::String(raw.as_str()?.replace(strip, ""))
            } else {
                raw.clone()
            };
            let n = number(&value)? * t["ratio"].as_f64().unwrap_or(1.)
                + t["offset"].as_f64().unwrap_or(0.);
            Some(json!((n * 1000.).round() / 1000.))
        }
        "values" => {
            let key = raw
                .as_str()
                .map(str::to_owned)
                .or_else(|| {
                    number(raw)
                        .filter(|n| n.is_finite() && n.fract() == 0.)
                        .map(|n| format!("{n:.0}"))
                })
                .unwrap_or_else(|| raw.to_string());
            t["table"]
                .get(&key)
                .cloned()
                .or_else(|| t.get("default").cloned())
        }
        "nonZero" => Some(json!(number(raw)? != 0.)),
        "equals" => Some(json!(same(raw, &t["value"]))),
        "notEquals" => Some(json!(!same(raw, &t["value"]))),
        "ifEquals" => Some(if same(raw, &t["value"]) {
            t["then"].clone()
        } else {
            t["else"].clone()
        }),
        "ifNonZero" => Some(if number(raw)? != 0. {
            t["then"].clone()
        } else {
            t["else"].clone()
        }),
        _ => None,
    }
}

pub fn write_transform(t: &Value, value: &Value) -> Option<(String, Vec<Value>)> {
    let kind = t["kind"].as_str()?;
    if kind == "methodSwitch" {
        return Some((
            t[if boolean(value)? { "on" } else { "off" }]
                .as_str()?
                .into(),
            vec![],
        ));
    }
    let argument = match kind {
        "onOff" => json!(if boolean(value)? { "on" } else { "off" }),
        "value" => value.clone(),
        "string" => json!(
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string())
        ),
        "integer" => json!(
            (number(value)? * t["ratio"].as_f64().unwrap_or(1.)
                + t["offset"].as_f64().unwrap_or(0.))
            .trunc() as i64
        ),
        "values" => json!(
            t["table"]
                .as_object()?
                .iter()
                .find(|(_, v)| same(v, value))?
                .0
        ),
        "boolChoice" => t[if boolean(value)? { "on" } else { "off" }].clone(),
        _ => return None,
    };
    let mut args = vec![argument];
    args.extend(t["suffix"].as_array().into_iter().flatten().cloned());
    Some((t["method"].as_str()?.into(), args))
}
