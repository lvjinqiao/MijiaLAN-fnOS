use mijialan_core::{DeviceState, Engine, Property, engine::numeric};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

mod value;
pub(crate) use value::validate_write;

const MAX_SERVICES: usize = 100;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Layout {
    pub aids: BTreeMap<String, u64>,
    pub iids: BTreeMap<String, u64>,
    pub version: u64,
    pub signature: String,
}
impl Layout {
    fn aid(&mut self, did: &str) -> u64 {
        let next = self.aids.values().copied().max().unwrap_or(1) + 1;
        *self.aids.entry(did.into()).or_insert(next)
    }
    fn iid(&mut self, did: &str, key: &str) -> u64 {
        let prefix = format!("{did}:");
        let next = self
            .iids
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(_, v)| *v)
            .max()
            .unwrap_or(7)
            + 1;
        *self.iids.entry(format!("{prefix}{key}")).or_insert(next)
    }
}

#[derive(Clone)]
pub enum Transform {
    Direct,
    Active,
    Working(u8),
    Percent,
    Mired,
    Option(Value),
    Adjust(f64),
    Fixed(Value),
    Action(u32, u32),
    Cleaning { start: (u32, u32), dock: (u32, u32) },
    Mapped(Vec<(u8, Value)>),
    FanSteps { values: Vec<Value>, power: String },
    FilterChange,
}
#[derive(Clone)]
pub struct Binding {
    pub did: String,
    pub key: String,
    pub transform: Transform,
    pub readable: bool,
    pub writable: bool,
}

impl Binding {
    pub fn read(&self, engine: &Engine, stale: bool) -> Result<Value, i32> {
        match &self.transform {
            Transform::Fixed(value) => return Ok(value.clone()),
            Transform::Adjust(_) | Transform::Action(_, _) => return Ok(json!(false)),
            _ => {}
        }
        let state = engine.state(&self.did).ok_or(-70402)?;
        self.read_state(&state, stale)
    }
    fn read_state(&self, state: &DeviceState, stale: bool) -> Result<Value, i32> {
        match &self.transform {
            Transform::Fixed(value) => return Ok(value.clone()),
            Transform::Adjust(_) | Transform::Action(_, _) => return Ok(json!(false)),
            _ => {}
        }
        if !stale && !state.online {
            return Err(-70402);
        }
        if matches!(self.transform, Transform::Cleaning { .. }) && !state.online {
            return Err(-70402);
        }
        let value = state.values.get(&self.key).ok_or(-70402)?;
        let property = state
            .spec
            .properties
            .iter()
            .find(|p| p.key() == self.key)
            .ok_or(-70402)?;
        Ok(match &self.transform {
            Transform::Cleaning { .. } => json!(mijialan_core::vacuum::is_cleaning(
                mijialan_core::vacuum::status_description(property, value).ok_or(-70402)?
            )),
            Transform::Direct => value.clone(),
            Transform::Active => json!(if as_bool(value).map_err(|_| -70402)? {
                1
            } else {
                0
            }),
            Transform::Working(active) => json!(if as_bool(value).map_err(|_| -70402)? {
                *active
            } else {
                0
            }),
            Transform::Option(option) => json!(equivalent(value, option)),
            Transform::Mapped(options) => json!(
                options
                    .iter()
                    .find(|(_, v)| equivalent(value, v))
                    .map(|(n, _)| *n)
                    .ok_or(-70402)?
            ),
            Transform::FanSteps { values, power } => {
                if !as_bool(state.values.get(power).ok_or(-70402)?).map_err(|_| -70402)? {
                    json!(0)
                } else {
                    let index = values
                        .iter()
                        .position(|v| equivalent(value, v))
                        .ok_or(-70402)?;
                    json!(((index + 1) as f64 * 100. / values.len() as f64).round())
                }
            }
            Transform::FilterChange => json!(if value.as_f64().ok_or(-70402)? <= 0. {
                1
            } else {
                0
            }),
            Transform::Percent => {
                let [min, max, _] = property.range.ok_or(-70402)?;
                let raw = value.as_f64().filter(|v| v.is_finite()).ok_or(-70402)?;
                if !min.is_finite() || !max.is_finite() || max <= min || raw < min || raw > max {
                    return Err(-70402);
                }
                json!((raw - min) / (max - min) * 100.)
            }
            Transform::Mired => json!(
                (1_000_000. / value.as_f64().filter(|v| *v > 0.).ok_or(-70402)?).round() as i64
            ),
            _ => unreachable!(),
        })
    }
    pub fn write(&self, engine: &Engine, value: &Value) -> Result<(), i32> {
        if !self.writable {
            return Err(-70404);
        }
        match &self.transform {
            Transform::Fixed(fixed) => {
                return if equivalent(fixed, value) {
                    Ok(())
                } else {
                    Err(-70410)
                };
            }
            Transform::Action(siid, aiid) => {
                return if as_bool(value)? {
                    engine
                        .action(&self.did, *siid, *aiid, vec![])
                        .map_err(|_| -70402)
                } else {
                    Ok(())
                };
            }
            _ => {}
        }
        let state = engine.state(&self.did).ok_or(-70402)?;
        if !state.online {
            return Err(-70402);
        }
        let property = state
            .spec
            .properties
            .iter()
            .find(|p| p.key() == self.key)
            .ok_or(-70402)?;
        if let Transform::Cleaning { start, dock } = &self.transform {
            let on = as_bool(value)?;
            let current = state
                .values
                .get(&self.key)
                .and_then(|v| mijialan_core::vacuum::status_description(property, v));
            if current.is_some_and(|description| {
                if on {
                    mijialan_core::vacuum::is_cleaning(description)
                } else {
                    mijialan_core::vacuum::is_docking(description)
                }
            }) {
                return Ok(());
            }
            let (siid, aiid) = if on { start } else { dock };
            return engine
                .action(&self.did, *siid, *aiid, vec![])
                .map_err(|_| -70402);
        }
        let output = match &self.transform {
            Transform::Direct => {
                if property.format == "bool" {
                    json!(as_bool(value)?)
                } else if let Some(number) = value.as_f64() {
                    numeric(number, &property.format)
                } else {
                    value.clone()
                }
            }
            Transform::Active => json!(match value.as_u64() {
                Some(0) => false,
                Some(1) => true,
                _ => return Err(-70410),
            }),
            Transform::FanSteps { values, power } => {
                let percent = value
                    .as_f64()
                    .filter(|v| v.is_finite() && *v >= 0. && *v <= 100.)
                    .ok_or(-70410)?;
                if percent == 0. {
                    return engine
                        .write(&self.did, power, json!(false))
                        .map_err(|_| -70402);
                }
                let index = ((percent * values.len() as f64 / 100.).round() as usize)
                    .clamp(1, values.len())
                    - 1;
                values[index].clone()
            }
            Transform::Mapped(options) => options
                .iter()
                .find(|(n, _)| Some(*n as u64) == value.as_u64())
                .map(|(_, v)| v.clone())
                .ok_or(-70410)?,
            Transform::Option(option) => {
                if as_bool(value)? {
                    option.clone()
                } else {
                    return Ok(());
                }
            }
            Transform::Adjust(direction) => {
                if !as_bool(value)? {
                    return Ok(());
                }
                let [min, max, step] = property.range.ok_or(-70410)?;
                let current = state
                    .values
                    .get(&self.key)
                    .and_then(Value::as_f64)
                    .ok_or(-70402)?;
                numeric(
                    (current + step * direction).clamp(min, max),
                    &property.format,
                )
            }
            Transform::Percent | Transform::Mired => {
                let number = value.as_f64().filter(|n| n.is_finite()).ok_or(-70410)?;
                let [min, max, step] = property.range.ok_or(-70410)?;
                let raw = if matches!(self.transform, Transform::Percent) {
                    if !(0. ..=100.).contains(&number) {
                        return Err(-70410);
                    }
                    min + number * (max - min) / 100.
                } else {
                    if number <= 0. {
                        return Err(-70410);
                    }
                    1_000_000. / number
                };
                numeric(
                    (min + ((raw - min) / step).round() * step).clamp(min, max),
                    &property.format,
                )
            }
            _ => return Err(-70404),
        };
        property.validate(&output).map_err(|_| -70410)?;
        engine
            .write(&self.did, &self.key, output)
            .map_err(|_| -70402)
    }
}
fn as_bool(value: &Value) -> Result<bool, i32> {
    value
        .as_bool()
        .or_else(|| match value.as_u64() {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => None,
        })
        .ok_or(-70410)
}
fn equivalent(a: &Value, b: &Value) -> bool {
    a == b || a.as_f64().zip(b.as_f64()).is_some_and(|(a, b)| a == b)
}

pub struct Database {
    pub accessories: Vec<Value>,
    pub bindings: BTreeMap<(u64, u64), Binding>,
    pub names: BTreeMap<(u64, u64), String>,
    pub warnings: Vec<String>,
}
impl Database {
    pub fn attribute(&self, aid: u64, iid: u64) -> Option<&Value> {
        self.accessories
            .iter()
            .find(|a| a["aid"].as_u64() == Some(aid))?["services"]
            .as_array()?
            .iter()
            .flat_map(|s| s["characteristics"].as_array().into_iter().flatten())
            .find(|c| c["iid"].as_u64() == Some(iid))
    }
    pub fn json(&self, engine: &Engine) -> Value {
        let mut accessories = self.accessories.clone();
        for accessory in &mut accessories {
            let aid = accessory["aid"].as_u64().unwrap();
            for service in accessory["services"].as_array_mut().unwrap() {
                for characteristic in service["characteristics"].as_array_mut().unwrap() {
                    let iid = characteristic["iid"].as_u64().unwrap();
                    if let Some(binding) = self.bindings.get(&(aid, iid)).filter(|b| b.readable) {
                        characteristic["value"] = binding
                            .read(engine, true)
                            .and_then(|raw| value::normalize_read(characteristic, raw))
                            .unwrap_or(Value::Null);
                    }
                }
            }
        }
        json!({"accessories":accessories})
    }
    /// All read paths use the same wire constraints, including events.
    pub fn read(&self, engine: &Engine, aid: u64, iid: u64, stale: bool) -> Result<Value, i32> {
        let attribute = self.attribute(aid, iid).ok_or(-70409)?;
        if !attribute["perms"]
            .as_array()
            .is_some_and(|p| p.iter().any(|p| p == "pr"))
        {
            return Err(-70405);
        }
        let raw = if let Some(binding) = self.bindings.get(&(aid, iid)) {
            binding.read(engine, stale)?
        } else {
            attribute.get("value").cloned().ok_or(-70402)?
        };
        value::normalize_read(attribute, raw)
    }
}

struct Builder<'a> {
    engine: &'a Engine,
    layout: &'a mut Layout,
    device: &'a DeviceState,
    aid: u64,
    services: Vec<Value>,
    bindings: BTreeMap<(u64, u64), Binding>,
    names: BTreeMap<(u64, u64), String>,
    consumed: BTreeSet<String>,
    service_indices: BTreeMap<String, usize>,
    omitted_services: usize,
}
impl Builder<'_> {
    fn service(&mut self, key: &str, kind: &str, name: &str) -> Option<usize> {
        if let Some(index) = self.service_indices.get(key) {
            return Some(*index);
        }
        if self.services.len() >= MAX_SERVICES {
            self.omitted_services += 1;
            return None;
        }
        let iid = self
            .layout
            .iid(&self.device.device.did, &format!("service:{key}"));
        let name_iid = self
            .layout
            .iid(&self.device.device.did, &format!("{key}:name"));
        let configured_iid = self
            .layout
            .iid(&self.device.device.did, &format!("{key}:configured-name"));
        let name_key = format!("{}:{key}", self.device.device.did);
        let configured_name = self
            .engine
            .store
            .config()
            .homekit_names
            .get(&name_key)
            .cloned()
            .unwrap_or(name.into());
        self.names.insert((self.aid, configured_iid), name_key);
        self.services.push(json!({"iid":iid,"type":kind,"characteristics":[{"iid":name_iid,"type":"23","perms":["pr"],"format":"string","value":hap_string(name)},{"iid":configured_iid,"type":"E3","perms":["pr","pw","ev"],"format":"string","value":hap_string(&configured_name)}]}));
        let index = self.services.len() - 1;
        self.service_indices.insert(key.into(), index);
        Some(index)
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "Keep each HAP field visible together in the declarative mapping table"
    )]
    fn characteristic(
        &mut self,
        service: usize,
        key: &str,
        kind: &str,
        format: &str,
        property: Option<&Property>,
        transform: Transform,
        writable: bool,
        range: Option<[f64; 3]>,
    ) {
        if self.services[service]["characteristics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["type"] == kind)
        {
            return;
        }
        let iid = self.layout.iid(&self.device.device.did, key);
        let mut perms = vec!["pr", "ev"];
        if writable {
            perms.push("pw");
        }
        let binding = Binding {
            did: self.device.device.did.clone(),
            key: property.map(Property::key).unwrap_or_default(),
            transform,
            readable: true,
            writable,
        };
        let mut value = json!({"iid":iid,"type":kind,"format":format,"perms":perms});
        if let Some([min, max, step]) = range {
            value["minValue"] = json!(min);
            value["maxValue"] = json!(max);
            value["minStep"] = json!(step);
        }
        if format == "uint8" {
            let valid: Option<BTreeSet<u8>> = match &binding.transform {
                Transform::Fixed(v) => v
                    .as_u64()
                    .and_then(|v| u8::try_from(v).ok())
                    .map(|v| [v].into()),
                Transform::Mapped(v) => Some(v.iter().map(|(n, _)| *n).collect()),
                _ => None,
            };
            if let Some(valid) = valid {
                value["valid-values"] = json!(valid);
            }
        }
        value["value"] = binding
            .read_state(self.device, true)
            .and_then(|raw| value::normalize_read(&value, raw))
            .unwrap_or(Value::Null);
        self.services[service]["characteristics"]
            .as_array_mut()
            .unwrap()
            .push(value);
        self.bindings.insert((self.aid, iid), binding);
        if let Some(property) = property {
            self.consumed.insert(property.key());
        }
    }
    fn named(&mut self, property: &Property, suffix: &str, title: &str, transform: Transform) {
        let key = format!("{}:{suffix}", property.key());
        let Some(service) = self.service(&key, "49", title) else {
            return;
        };
        self.characteristic(
            service,
            &format!("{key}:on"),
            "25",
            "bool",
            Some(property),
            transform,
            true,
            None,
        );
    }
}

pub fn build(engine: &Engine, layout: &mut Layout) -> Database {
    build_states(engine, layout, engine.states())
}

pub(crate) fn build_states(
    engine: &Engine,
    layout: &mut Layout,
    states: Vec<DeviceState>,
) -> Database {
    let mut database = Database {
        accessories: vec![json!({"aid":1,"services":[
            information("MijiaLAN Rust Bridge","MijiaLAN-Rust","bridge"),
            // HAP IP 1.1 requires protocol information on the primary
            // accessory only. IIDs 1–7 remain the existing identity.
            {"iid":8,"type":"A2","characteristics":[
                {"iid":9,"type":"37","format":"string","perms":["pr"],"value":"1.1.0"}
            ]}
        ]})],
        bindings: BTreeMap::new(),
        names: BTreeMap::new(),
        warnings: Vec::new(),
    };
    let included: BTreeSet<_> = engine
        .store
        .config()
        .devices
        .iter()
        .filter(|d| d.bridged)
        .map(|d| d.did.clone())
        .collect();
    for state in states
        .into_iter()
        .filter(|s| included.contains(&s.device.did) && !s.spec.properties.is_empty())
        .take(149)
    {
        let aid = layout.aid(&state.device.did);
        let mut builder = Builder {
            engine,
            layout,
            device: &state,
            aid,
            services: vec![information(
                &state.device.name,
                &state.device.model,
                &state.device.did,
            )],
            bindings: BTreeMap::new(),
            names: BTreeMap::new(),
            consumed: BTreeSet::new(),
            service_indices: BTreeMap::new(),
            omitted_services: 0,
        };
        let props = &state.spec.properties;
        let power = props
            .iter()
            .find(|p| p.name == "on" && p.writable && p.format == "bool");
        let native = native(&mut builder, &state);
        if let Some(power) = power.filter(|_| {
            !native
                && matches!(
                    state.spec.kind.as_str(),
                    "light" | "switch" | "outlet" | "socket" | "fan" | "air-purifier"
                )
        }) {
            let kind = state.spec.kind.as_str();
            let service_type = match kind {
                "light" => "43",
                "switch" => "49",
                "fan" => "40",
                "air-purifier" => "BB",
                _ => "47",
            };
            let s = builder
                .service("primary", service_type, &state.device.name)
                .expect("primary service is added before optional services");
            let active = kind == "air-purifier";
            builder.characteristic(
                s,
                "primary:power",
                if active { "B0" } else { "25" },
                if active { "uint8" } else { "bool" },
                Some(power),
                if active {
                    Transform::Active
                } else {
                    Transform::Direct
                },
                true,
                if active { Some([0., 1., 1.]) } else { None },
            );
            if kind == "air-purifier" {
                builder.characteristic(
                    s,
                    "primary:current",
                    "A9",
                    "uint8",
                    Some(power),
                    Transform::Working(2),
                    false,
                    Some([0., 2., 1.]),
                );
                builder.characteristic(
                    s,
                    "primary:target",
                    "A8",
                    "uint8",
                    None,
                    Transform::Fixed(json!(0)),
                    true,
                    Some([0., 1., 1.]),
                );
            } else if service_type == "47" {
                builder.characteristic(
                    s,
                    "primary:in-use",
                    "26",
                    "bool",
                    Some(power),
                    Transform::Direct,
                    false,
                    None,
                );
            }
            for p in props {
                if !p.writable {
                    continue;
                }
                if kind == "light" && p.name == "brightness" {
                    builder.characteristic(
                        s,
                        "primary:brightness",
                        "8",
                        "int",
                        Some(p),
                        Transform::Direct,
                        true,
                        Some([0., 100., 1.]),
                    );
                }
                if kind == "light"
                    && p.name == "color-temperature"
                    && let Some([min, max, _]) = p.range.filter(|r| r[0] > 0.)
                {
                    builder.characteristic(
                        s,
                        "primary:ct",
                        "CE",
                        "uint32",
                        Some(p),
                        Transform::Mired,
                        true,
                        Some([(1_000_000. / max).round(), (1_000_000. / min).round(), 1.]),
                    );
                }
                if ["fan", "air-purifier"].contains(&kind)
                    && p.name == "speed-level"
                    && p.range.is_some_and(|r| r[0] < r[1])
                {
                    builder.characteristic(
                        s,
                        "primary:speed",
                        "29",
                        "float",
                        Some(p),
                        Transform::Percent,
                        true,
                        Some([0., 100., 1.]),
                    );
                }
            }
        }
        for p in props.iter().filter(|p| p.readable) {
            let (service, characteristic, range) = match p.name.as_str() {
                "temperature" => ("8A", "11", [-40., 100., 0.1]),
                "relative-humidity" => ("82", "10", [0., 100., 1.]),
                "pm2.5-density" => ("8D", "C6", [0., 1000., 1.]),
                "filter-life-level" => ("BA", "AB", [0., 100., 1.]),
                _ => continue,
            };
            let Some(s) = builder.service(&format!("sensor:{}", p.key()), service, p.label())
            else {
                continue;
            };
            if service == "8D" {
                builder.characteristic(
                    s,
                    &format!("sensor:{}:quality", p.key()),
                    "95",
                    "uint8",
                    None,
                    Transform::Fixed(json!(0)),
                    false,
                    Some([0., 5., 1.]),
                );
            }
            if service == "BA" {
                builder.characteristic(
                    s,
                    &format!("sensor:{}:change", p.key()),
                    "AC",
                    "uint8",
                    Some(p),
                    Transform::FilterChange,
                    false,
                    Some([0., 1., 1.]),
                );
            }
            builder.characteristic(
                s,
                &format!("sensor:{}:value", p.key()),
                characteristic,
                "float",
                Some(p),
                Transform::Direct,
                false,
                Some(range),
            );
        }
        // Unmapped properties/actions stay in the management UI. Never invent switches
        // for numeric endpoints, enum choices, timers or momentary actions.
        if builder.omitted_services > 0 {
            database.warnings.push(if mijialan_core::localization::english() {
                format!("Some extra Home controls for {} exceed the bridge limit; use the client for those controls.", state.device.name)
            } else {
                format!("「{}」的部分附加功能超出家庭桥接上限，仍可在客户端使用。", state.device.name)
            });
        }
        if builder.services.len() == 1 {
            continue;
        }
        database
            .accessories
            .push(json!({"aid":aid,"services":builder.services}));
        database.bindings.extend(builder.bindings);
        database.names.extend(builder.names);
    }
    database
}

fn information(name: &str, model: &str, serial: &str) -> Value {
    json!({"iid":1,"type":"3E","characteristics":[
        {"iid":2,"type":"14","format":"bool","perms":["pw"]},
        {"iid":3,"type":"20","format":"string","perms":["pr"],"value":"iPocket"},
        {"iid":4,"type":"21","format":"string","perms":["pr"],"value":hap_string(model)},
        {"iid":5,"type":"23","format":"string","perms":["pr"],"value":hap_string(name)},
        {"iid":6,"type":"30","format":"string","perms":["pr"],"value":hap_string(&format!("mijia-{serial}"))},
        {"iid":7,"type":"52","format":"string","perms":["pr"],"value":"0.1.0"}
    ]})
}

/// The default HAP string limit is 64 UTF-8 bytes, not 64 characters.
/// Bound wire values without modifying the user's original device/config name.
fn hap_string(value: &str) -> &str {
    let mut end = value.len().min(64);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn native(b: &mut Builder<'_>, state: &DeviceState) -> bool {
    let props = &state.spec.properties;
    let power = props
        .iter()
        .find(|p| p.name == "on" && p.writable && p.format == "bool");
    let find = |names: &[&str]| props.iter().find(|p| names.contains(&p.name.as_str()));
    let kind = state.spec.kind.as_str();
    if kind == "bath-heater" && state.device.model == "opple.bhf_light.wlyb" {
        for (service_name, property_name, service_type, title) in [
            ("light", "on", "43", "照明"),
            ("custom", "night", "43", "小夜灯"),
            ("ptc-bath-heater", "ventilation", "B7", "换气"),
            ("ptc-bath-heater", "blow", "B7", "吹风"),
        ] {
            if let Some(p) = props.iter().find(|p| {
                p.service == service_name
                    && p.name == property_name
                    && p.readable
                    && p.writable
                    && p.format == "bool"
            }) {
                let key = format!("bath:{}", p.key());
                if let Some(service) = b.service(&key, service_type, title) {
                    let fan = service_type == "B7";
                    b.characteristic(
                        service,
                        &format!("{key}:power"),
                        if fan { "B0" } else { "25" },
                        if fan { "uint8" } else { "bool" },
                        Some(p),
                        if fan {
                            Transform::Active
                        } else {
                            Transform::Direct
                        },
                        true,
                        if fan { Some([0., 1., 1.]) } else { None },
                    );
                }
            }
        }
        let heating = props.iter().find(|p| {
            p.service == "ptc-bath-heater"
                && p.name == "heating"
                && p.readable
                && p.writable
                && p.format == "bool"
        });
        let temperature = props
            .iter()
            .find(|p| p.service == "ptc-bath-heater" && p.name == "temperature" && p.readable);
        let target = props.iter().find(|p| {
            p.service == "custom"
                && p.name == "temp"
                && p.readable
                && p.writable
                && p.range.is_some()
        });
        if let (Some(power), Some(current), Some(target)) = (heating, temperature, target) {
            if let Some(service) = b.service("bath:heater", "BC", "取暖") {
                b.characteristic(
                    service,
                    "bath:active",
                    "B0",
                    "uint8",
                    Some(power),
                    Transform::Active,
                    true,
                    Some([0., 1., 1.]),
                );
                b.characteristic(
                    service,
                    "bath:state",
                    "B1",
                    "uint8",
                    Some(power),
                    Transform::Working(2),
                    false,
                    Some([0., 3., 1.]),
                );
                b.characteristic(
                    service,
                    "bath:mode",
                    "B2",
                    "uint8",
                    None,
                    Transform::Fixed(json!(1)),
                    true,
                    Some([1., 1., 1.]),
                );
                b.characteristic(
                    service,
                    "bath:temperature",
                    "11",
                    "float",
                    Some(current),
                    Transform::Direct,
                    false,
                    current.range,
                );
                b.characteristic(
                    service,
                    "bath:target",
                    "12",
                    "float",
                    Some(target),
                    Transform::Direct,
                    true,
                    target.range,
                );
            }
        }
        return true;
    }
    if kind == "airer" {
        // Only properties with known semantics get a HomeKit service. In particular,
        // direction commands alone cannot provide a covering's position feedback.
        for p in props
            .iter()
            .filter(|p| p.readable && p.writable && p.format == "bool")
        {
            let mapping = match (p.service.as_str(), p.name.as_str()) {
                ("light", "on") => Some(("43", "25", "照明", Transform::Direct)),
                ("airer", "dryer") if state.device.model == "zjjsw.airer.sa006" => {
                    Some(("B7", "B0", "风干", Transform::Active))
                }
                _ => None,
            };
            if let Some((service_type, characteristic_type, name, transform)) = mapping {
                let key = format!("airer:{}", p.key());
                if let Some(service) = b.service(&key, service_type, name) {
                    let active = characteristic_type == "B0";
                    b.characteristic(
                        service,
                        &format!("{key}:power"),
                        characteristic_type,
                        if active { "uint8" } else { "bool" },
                        Some(p),
                        transform,
                        true,
                        if active { Some([0., 1., 1.]) } else { None },
                    );
                }
            }
        }
        return true;
    }
    if kind == "fan" {
        let Some(power) = props
            .iter()
            .find(|p| {
                p.service == "fan"
                    && p.name == "on"
                    && p.readable
                    && p.writable
                    && p.format == "bool"
            })
            .or(power)
        else {
            return false;
        };
        let Some(service) = b.service("native", "B7", &state.device.name) else {
            return false;
        };
        b.characteristic(
            service,
            "native:active",
            "B0",
            "uint8",
            Some(power),
            Transform::Active,
            true,
            Some([0., 1., 1.]),
        );
        if let Some(speed) = find(&["speed-level"])
            .filter(|p| p.readable && p.writable && p.range.is_some_and(|r| r[0] < r[1]))
        {
            b.characteristic(
                service,
                "native:speed",
                "29",
                "float",
                Some(speed),
                Transform::Percent,
                true,
                Some([0., 100., 1.]),
            );
        }
        if let Some(speed) = props.iter().find(|p| {
            p.service == "fan"
                && p.name == "fan-level"
                && p.readable
                && p.writable
                && p.choices.len() >= 2
                && p.choices.iter().all(|c| c.value.as_u64().is_some())
        }) {
            let mut values: Vec<_> = speed.choices.iter().map(|c| c.value.clone()).collect();
            values.sort_by_key(|v| v.as_u64().unwrap());
            values.dedup();
            b.characteristic(
                service,
                "native:speed",
                "29",
                "float",
                Some(speed),
                Transform::FanSteps {
                    values,
                    power: power.key(),
                },
                true,
                Some([0., 100., 1.]),
            );
        }
        if let Some(swing) =
            find(&["horizontal-swing"]).filter(|p| p.readable && p.writable && p.format == "bool")
        {
            b.characteristic(
                service,
                "native:swing",
                "B6",
                "uint8",
                Some(swing),
                Transform::Active,
                true,
                Some([0., 1., 1.]),
            );
        }
        if let Some(lock) = find(&["physical-controls-locked"])
            .filter(|p| p.readable && p.writable && p.format == "bool")
        {
            b.characteristic(
                service,
                "native:lock",
                "A7",
                "uint8",
                Some(lock),
                Transform::Active,
                true,
                Some([0., 1., 1.]),
            );
        }
        return true;
    }
    if kind == "vacuum" {
        let action = |service: &str, name: &str| {
            state
                .spec
                .actions
                .iter()
                .find(|a| a.service == service && a.name == name && a.inputs.is_empty())
        };
        let (Some(status), Some(start), Some(dock)) = (
            mijialan_core::vacuum::status_property(&state.spec),
            action("vacuum", "start-sweep"),
            action("battery", "start-charge").or_else(|| action("vacuum", "start-charge")),
        ) else {
            return false;
        };
        let Some(service) = b.service("native", "49", &state.device.name) else {
            return false;
        };
        b.characteristic(
            service,
            "native:cleaning",
            "25",
            "bool",
            Some(status),
            Transform::Cleaning {
                start: (start.siid, start.aiid),
                dock: (dock.siid, dock.aiid),
            },
            true,
            None,
        );
        b.consumed
            .insert(format!("action:{}.{}", start.siid, start.aiid));
        b.consumed
            .insert(format!("action:{}.{}", dock.siid, dock.aiid));
        return true;
    }
    if kind == "curtain" || kind == "window-covering" {
        let Some(target) = find(&["target-position"]).filter(|p| p.writable && p.range.is_some())
        else {
            return false;
        };
        let current = find(&["current-position"]).unwrap_or(target);
        let Some(s) = b.service("native", "8C", &state.device.name) else {
            return false;
        };
        b.characteristic(
            s,
            "native:target",
            "7C",
            "uint8",
            Some(target),
            Transform::Percent,
            true,
            Some([0., 100., 1.]),
        );
        b.characteristic(
            s,
            "native:current",
            "6D",
            "uint8",
            Some(current),
            Transform::Percent,
            false,
            Some([0., 100., 1.]),
        );
        b.characteristic(
            s,
            "native:state",
            "72",
            "uint8",
            None,
            Transform::Fixed(json!(2)),
            false,
            Some([0., 2., 1.]),
        );
        return true;
    }
    if kind == "humidifier" || kind == "dehumidifier" {
        let (Some(power), Some(humidity), Some(target)) = (
            power,
            find(&["relative-humidity"]).filter(|p| p.readable),
            find(&["target-humidity"]).filter(|p| p.writable && p.range.is_some()),
        ) else {
            return false;
        };
        let humidifying = kind == "humidifier";
        let Some(s) = b.service("native", "BD", &state.device.name) else {
            return false;
        };
        b.characteristic(
            s,
            "native:power",
            "B0",
            "uint8",
            Some(power),
            Transform::Active,
            true,
            Some([0., 1., 1.]),
        );
        b.characteristic(
            s,
            "native:current",
            "B3",
            "uint8",
            Some(power),
            Transform::Working(if humidifying { 2 } else { 3 }),
            false,
            Some([0., 3., 1.]),
        );
        b.characteristic(
            s,
            "native:target",
            "B4",
            "uint8",
            None,
            Transform::Fixed(json!(if humidifying { 1 } else { 2 })),
            true,
            Some([0., 2., 1.]),
        );
        b.characteristic(
            s,
            "native:humidity",
            "10",
            "float",
            Some(humidity),
            Transform::Direct,
            false,
            Some([0., 100., 1.]),
        );
        b.characteristic(
            s,
            "native:threshold",
            if humidifying { "CA" } else { "C9" },
            "float",
            Some(target),
            Transform::Direct,
            true,
            target.range,
        );
        if let Some(water) =
            find(&["water-level"]).filter(|p| p.readable && p.range.is_some_and(|r| r[0] < r[1]))
        {
            b.characteristic(
                s,
                "native:water",
                "B5",
                "float",
                Some(water),
                Transform::Percent,
                false,
                Some([0., 100., 1.]),
            );
        }
        return true;
    }
    if kind == "heater" || kind == "air-conditioner" {
        let (Some(power), Some(target)) = (
            power,
            find(&["target-temperature"]).filter(|p| p.writable && p.range.is_some()),
        ) else {
            return false;
        };
        let modes = if kind == "air-conditioner" {
            let Some(mode) = find(&["mode"]).filter(|p| p.writable) else {
                return false;
            };
            let values: Vec<_> = mode
                .choices
                .iter()
                .filter_map(|c| {
                    let text = c.description.to_lowercase();
                    let code = if text.contains("auto") || text.contains("自动") {
                        0
                    } else if text.contains("heat") || text.contains("制热") {
                        1
                    } else if text.contains("cool") || text.contains("制冷") {
                        2
                    } else {
                        return None;
                    };
                    Some((code, c.value.clone()))
                })
                .collect();
            if values.is_empty() {
                return false;
            }
            Some((mode, values))
        } else {
            None
        };
        let temperature = find(&["temperature", "indoor-temperature"])
            .filter(|p| p.readable)
            .unwrap_or(target);
        let Some(s) = b.service("native", "BC", &state.device.name) else {
            return false;
        };
        b.characteristic(
            s,
            "native:power",
            "B0",
            "uint8",
            Some(power),
            Transform::Active,
            true,
            Some([0., 1., 1.]),
        );
        b.characteristic(
            s,
            "native:current",
            "B1",
            "uint8",
            Some(power),
            Transform::Working(if kind == "heater" { 2 } else { 1 }),
            false,
            Some([0., 3., 1.]),
        );
        if let Some((mode, values)) = modes {
            let states: Vec<_> = values.iter().map(|(n, _)| *n).collect();
            b.characteristic(
                s,
                "native:target-mode",
                "B2",
                "uint8",
                Some(mode),
                Transform::Mapped(values),
                true,
                Some([0., 2., 1.]),
            );
            if states.contains(&0) || states.contains(&2) {
                b.characteristic(
                    s,
                    "native:cooling",
                    "D",
                    "float",
                    Some(target),
                    Transform::Direct,
                    true,
                    target.range,
                );
            }
            if states.contains(&0) || states.contains(&1) {
                b.characteristic(
                    s,
                    "native:heating",
                    "12",
                    "float",
                    Some(target),
                    Transform::Direct,
                    true,
                    target.range,
                );
            }
        } else {
            b.characteristic(
                s,
                "native:target-mode",
                "B2",
                "uint8",
                None,
                Transform::Fixed(json!(1)),
                true,
                Some([0., 2., 1.]),
            );
            b.characteristic(
                s,
                "native:heating",
                "12",
                "float",
                Some(target),
                Transform::Direct,
                true,
                target.range,
            );
        }
        b.characteristic(
            s,
            "native:temperature",
            "11",
            "float",
            Some(temperature),
            Transform::Direct,
            false,
            Some([-40., 100., 0.1]),
        );
        b.characteristic(
            s,
            "native:units",
            "36",
            "uint8",
            None,
            Transform::Fixed(json!(0)),
            true,
            Some([0., 1., 1.]),
        );
        return true;
    }
    false
}

#[cfg(test)]
mod tests;
