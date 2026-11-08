use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub value: Value,
    pub description: String,
    pub description_en: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Property {
    pub siid: u32,
    pub piid: u32,
    pub service: String,
    pub name: String,
    pub description: String,
    pub description_en: String,
    pub format: String,
    pub readable: bool,
    pub writable: bool,
    pub unit: String,
    pub range: Option<[f64; 3]>,
    pub choices: Vec<Choice>,
}

impl Property {
    pub fn key(&self) -> String {
        format!("{}.{}", self.siid, self.piid)
    }
    pub fn label(&self) -> &str {
        Self::label_for(
            &self.service,
            &self.name,
            &self.description,
            &self.description_en,
            crate::localization::english(),
        )
    }
    pub(crate) fn label_for<'a>(
        service: &str,
        name: &str,
        description: &'a str,
        description_en: &'a str,
        english: bool,
    ) -> &'a str {
        if name == "on" && service == "indicator-light" {
            return if english {
                "Indicator light"
            } else {
                "指示灯"
            };
        }
        if english {
            return match name {
                "on" => "Power",
                "brightness" => "Brightness",
                "color-temperature" => "Color Temperature",
                "speed-level" => "Fan speed",
                "fan-level" => "Fan level",
                "pm2.5-density" => "PM2.5",
                "relative-humidity" => "Humidity",
                "target-temperature" => "Target temperature",
                "physical-controls-locked" => "Child lock",
                _ => description_en,
            };
        }
        match name {
            "on" => "电源",
            "brightness" => "亮度",
            "color-temperature" => "色温",
            "mode" => "模式",
            "fan-level" => "风速档位",
            "speed-level" => "风速",
            "horizontal-swing" => "左右摆风",
            "vertical-swing" => "上下摆风",
            "temperature" => "温度",
            "relative-humidity" => "湿度",
            "pm2.5-density" => "PM2.5",
            "target-temperature" => "设定温度",
            "fault" => "故障码",
            "status" => "状态",
            "physical-controls-locked" => "童锁",
            "indicator-light" => "指示灯",
            "filter-life-level" => "滤芯寿命",
            "air-quality" => "空气质量",
            _ => description,
        }
    }
    pub fn validate(&self, value: &Value) -> Result<()> {
        if !self.writable {
            return Err(Error::Validation("该属性未开放安全写入".into()));
        }
        if self.format == "bool" {
            return if value.is_boolean() {
                Ok(())
            } else {
                Err(Error::Validation("需要布尔值".into()))
            };
        }
        if !self.choices.is_empty() {
            return if self.choices.iter().any(|c| &c.value == value) {
                Ok(())
            } else {
                Err(Error::Validation("不在设备允许的选项内".into()))
            };
        }
        if let Some([min, max, step]) = self.range {
            let number = value
                .as_f64()
                .ok_or_else(|| Error::Validation("需要数值".into()))?;
            let units = (number - min) / step;
            if !number.is_finite()
                || number < min
                || number > max
                || (units - units.round()).abs() > 1e-6
            {
                return Err(Error::Validation("数值超出范围或不符合步长".into()));
            }
            if self.format != "float" && (number.fract().abs() > 1e-6) {
                return Err(Error::Validation("该属性只接受整数".into()));
            }
            return Ok(());
        }
        Err(Error::Validation("参数定义不完整，暂不写入".into()))
    }
    pub fn display(&self, value: &Value) -> String {
        if let Some(choice) = self.choices.iter().find(|c| c.value == *value) {
            return if crate::localization::english() {
                choice.description_en.clone()
            } else {
                choice.description.clone()
            };
        }
        if let Some(on) = value.as_bool() {
            return crate::localization::text(if on { "开启" } else { "关闭" });
        }
        let text = value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string());
        let unit = match self.unit.as_str() {
            "percentage" => "%",
            "celsius" => " °C",
            "kelvin" => " K",
            "none" => "",
            other => other,
        };
        format!("{text}{unit}")
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Specification {
    pub kind: String,
    pub properties: Vec<Property>,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub compatibility: Vec<crate::compatibility::Capability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub siid: u32,
    pub aiid: u32,
    #[serde(default)]
    pub service: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub description_en: String,
    pub inputs: Vec<Property>,
}

impl Action {
    pub fn label(&self) -> &str {
        self.label_for(crate::localization::english())
    }
    pub fn label_for(&self, english: bool) -> &str {
        action_label(&self.name, english).unwrap_or_else(|| {
            if english && !self.description_en.is_empty() {
                &self.description_en
            } else {
                &self.description
            }
        })
    }
}

pub(crate) fn action_label(name: &str, english: bool) -> Option<&'static str> {
    let (zh, en) = match name {
        "start-cook" => ("开始烹饪", "Start Cooking"),
        "cancel-cooking" => ("停止烹饪", "Stop Cooking"),
        "pause" => ("暂停", "Pause"),
        "resume-cook" => ("继续", "Resume"),
        "start-sweep" => ("开始清扫", "Start Cleaning"),
        "start-only-sweep" => ("仅扫地", "Vacuum Only"),
        "start-mop" => ("仅拖地", "Mop Only"),
        "start-sweep-mop" => ("扫拖", "Vacuum and Mop"),
        "stop-sweeping" => ("停止清扫", "Stop Cleaning"),
        "pause-sweeping" => ("暂停清扫", "Pause Cleaning"),
        "start-charge" => ("回充", "Return to Dock"),
        _ => return None,
    };
    Some(if english { en } else { zh })
}

pub(crate) fn urn_name(value: &Value) -> String {
    value
        .as_str()
        .unwrap_or_default()
        .split(':')
        .nth(3)
        .unwrap_or_default()
        .into()
}

impl Specification {
    pub fn parse(text: &str) -> Result<Self> {
        let document: Value =
            serde_json::from_str(text).map_err(|_| Error::Spec("无法解析设备规格".into()))?;
        let mut properties = Vec::new();
        let mut actions = Vec::new();
        for service in document["services"].as_array().into_iter().flatten() {
            let service_name = urn_name(&service["type"]);
            if ["device-information", "device-diagnosis", "device-config"]
                .contains(&service_name.as_str())
            {
                continue;
            }
            let Some(siid) = service["iid"].as_u64().filter(|id| *id <= u32::MAX as u64) else {
                continue;
            };
            for raw in service["properties"].as_array().into_iter().flatten() {
                let Some(piid) = raw["iid"].as_u64().filter(|id| *id <= u32::MAX as u64) else {
                    continue;
                };
                let name = urn_name(&raw["type"]);
                let format = raw["format"].as_str().unwrap_or_default().to_string();
                let access: Vec<_> = raw["access"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                let range = raw["value-range"].as_array().and_then(|r| {
                    if r.len() != 3 {
                        return None;
                    }
                    let range = [r[0].as_f64()?, r[1].as_f64()?, r[2].as_f64()?];
                    (range.iter().all(|n| n.is_finite()) && range[0] <= range[1] && range[2] > 0.)
                        .then_some(range)
                });
                let choices: Vec<_> = raw["value-list"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|c| {
                        Some(Choice {
                            value: c.get("value")?.clone(),
                            description_en: c["description"].as_str().unwrap_or("Option").into(),
                            description: c["comment"]
                                .as_str()
                                .or(c["description"].as_str())
                                .unwrap_or("选项")
                                .into(),
                        })
                    })
                    .collect();
                let writable = access.contains(&"write")
                    && (format == "bool" || !choices.is_empty() || range.is_some())
                    && !name.is_empty();
                properties.push(Property {
                    siid: siid as u32,
                    piid: piid as u32,
                    service: service_name.clone(),
                    name,
                    description_en: raw["description"].as_str().unwrap_or("Property").into(),
                    description: raw["comment"]
                        .as_str()
                        .or(raw["description"].as_str())
                        .unwrap_or("属性")
                        .into(),
                    format,
                    readable: access.contains(&"read"),
                    writable,
                    unit: raw["unit"].as_str().unwrap_or_default().into(),
                    range,
                    choices,
                });
            }
            for action in service["actions"].as_array().into_iter().flatten() {
                let name = urn_name(&action["type"]);
                let Some(input_ids) = action["in"].as_array() else {
                    continue;
                };
                let kitchen = service_name == urn_name(&document["type"])
                    && ["start-cook", "cancel-cooking", "pause", "resume-cook"]
                        .contains(&name.as_str());
                let vacuum = urn_name(&document["type"]) == "vacuum"
                    && crate::vacuum::supports_action(&service_name, &name)
                    && input_ids.is_empty();
                if !kitchen && !vacuum {
                    continue;
                }
                if name.is_empty()
                    || ["reset", "factory", "calibrate", "diagnosis"]
                        .iter()
                        .any(|s| name.contains(s))
                {
                    continue;
                }
                let Some(aiid) = action["iid"].as_u64().filter(|id| *id <= u32::MAX as u64) else {
                    continue;
                };
                let mut inputs = Vec::new();
                let mut supported = true;
                for id in input_ids {
                    let Some(mut input) = properties
                        .iter()
                        .find(|p| p.siid == siid as u32 && Some(p.piid as u64) == id.as_u64())
                        .cloned()
                    else {
                        supported = false;
                        break;
                    };
                    input.writable = input.format == "bool"
                        || input.range.is_some()
                        || !input.choices.is_empty();
                    supported &= input.writable;
                    inputs.push(input);
                }
                if supported {
                    actions.push(Action {
                        siid: siid as u32,
                        aiid: aiid as u32,
                        service: service_name.clone(),
                        name,
                        description: action["comment"]
                            .as_str()
                            .or(action["description"].as_str())
                            .unwrap_or("执行动作")
                            .into(),
                        description_en: action["description"].as_str().unwrap_or("Action").into(),
                        inputs,
                    });
                }
            }
        }
        Ok(Self {
            kind: urn_name(&document["type"]),
            properties,
            actions,
            compatibility: Vec::new(),
        })
    }

    /// Attach support explanations only after applying the model's LAN adapter.
    pub fn for_model(model: &str, text: &str) -> Result<Self> {
        let mut spec = Self::parse(text)?;
        let legacy = crate::legacy::Profile::for_model(model);
        if let Some(profile) = &legacy {
            profile.adapt_spec(&mut spec);
        }
        spec.compatibility = crate::compatibility::inspect(text, &spec, legacy.as_ref())?;
        Ok(spec)
    }

    pub fn bundled(model: &str) -> Option<&'static str> {
        match model {
            "chuangmi.plug.m3" => Some(include_str!(
                "../../../resources/specs/chuangmi.plug.m3.json"
            )),
            "cuco.plug.v3" => Some(include_str!("../../../resources/specs/cuco.plug.v3.json")),
            "zhimi.airpurifier.rma1" => Some(include_str!(
                "../../../resources/specs/zhimi.airpurifier.rma1.json"
            )),
            "yeelink.light.lamp4" => Some(include_str!(
                "../../../resources/specs/yeelink.light.lamp4.json"
            )),
            "lumi.acpartner.mcn02" => Some(include_str!(
                "../../../resources/specs/lumi.acpartner.mcn02.json"
            )),
            "dmaker.fan.p28" => Some(include_str!("../../../resources/specs/dmaker.fan.p28.json")),
            "chunmi.juicer.a1" => Some(include_str!(
                "../../../resources/specs/chunmi.juicer.a1.json"
            )),
            "chunmi.health_pot.a1" => Some(include_str!(
                "../../../resources/specs/chunmi.health_pot.a1.json"
            )),
            "careli.fryer.maf07c" => Some(include_str!(
                "../../../resources/specs/careli.fryer.maf07c.json"
            )),
            "chunmi.cooker.cmwy3" => Some(include_str!(
                "../../../resources/specs/chunmi.cooker.cmwy3.json"
            )),
            _ => None,
        }
    }
}

pub fn fetch(model: &str) -> Result<String> {
    let http = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| Error::Spec(e.to_string()))?;
    let instances: Value = http
        .get("https://miot-spec.org/miot-spec-v2/instances?status=all")
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.json())
        .map_err(|e| Error::Spec(e.to_string()))?;
    let kind = instances["instances"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|i| i["model"].as_str() == Some(model))
        .max_by_key(|i| i["version"].as_u64().unwrap_or(0))
        .and_then(|i| i["type"].as_str())
        .ok_or_else(|| Error::Spec(format!("{model} 暂无 MIoT 规格")))?;
    let text = http
        .get("https://miot-spec.org/miot-spec-v2/instance")
        .query(&[("type", kind)])
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.text())
        .map_err(|e| Error::Spec(e.to_string()))?;
    Specification::parse(&text)?;
    Ok(text)
}
