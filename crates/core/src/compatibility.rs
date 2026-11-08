//! Descriptive support information, retained separately from executable controls.
use crate::{Error, Property, Result, Specification, legacy::Profile, spec::urn_name};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    LegacyMapping,
    IncompleteParameters,
    UnavailableOperation,
    UnavailableFeature,
    PowerSwitch,
    BrightnessSlider,
    ColorTemperatureControl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub id: String,
    pub service: String,
    pub name: String,
    pub description: String,
    pub description_en: String,
    pub is_action: bool,
    pub reason: Reason,
}

impl Capability {
    pub fn title(&self, english: bool) -> &str {
        if self.is_action
            && let Some(label) = crate::spec::action_label(&self.name, english)
        {
            return label;
        }
        let (zh, en) = match self.name.as_str() {
            "mode" if self.service == "light" => ("灯光场景", "Light Scene"),
            "toggle" => ("切换开关状态", "Toggle Power"),
            "brightness-delta" => ("增减亮度", "Adjust Brightness"),
            "ct-delta" => ("增减色温", "Adjust Color Temperature"),
            "ct-adjust-alexa" => ("Alexa 色温调节", "Alexa Color Temperature"),
            "start-cook" if self.is_action => ("开始烹饪", "Start Cooking"),
            "cancel-cooking" if self.is_action => ("停止烹饪", "Stop Cooking"),
            "pause" if self.is_action => ("暂停", "Pause"),
            "resume-cook" if self.is_action => ("继续", "Resume"),
            _ if self.is_action => {
                return if english {
                    &self.description_en
                } else {
                    &self.description
                };
            }
            _ => {
                return Property::label_for(
                    &self.service,
                    &self.name,
                    &self.description,
                    &self.description_en,
                    english,
                );
            }
        };
        if english { en } else { zh }
    }

    pub fn explanation(&self, english: bool) -> &str {
        let (zh, en) = match self.reason {
            Reason::LegacyMapping => (
                "当前版本暂未适配局域网控制",
                "LAN control is not supported in this version yet.",
            ),
            Reason::IncompleteParameters => (
                "暂不支持设置，设备未提供完整的参数说明",
                "This setting is unavailable because the device does not provide a complete parameter definition.",
            ),
            Reason::UnavailableOperation => (
                "此操作未在 App 中开放",
                "This operation is not available in the app.",
            ),
            Reason::UnavailableFeature => (
                "当前版本暂不支持此功能",
                "This feature is not supported in this version yet.",
            ),
            Reason::PowerSwitch => ("请使用顶部的电源开关", "Use the power switch at the top."),
            Reason::BrightnessSlider => ("请使用亮度滑条", "Use the brightness slider."),
            Reason::ColorTemperatureControl => (
                "请使用色温滑条或数值输入",
                "Use the color temperature slider or numeric input.",
            ),
        };
        if english { en } else { zh }
    }
}

/// Traverse the original declaration order, including actions omitted from the
/// executable specification. A device being offline does not change support.
pub(crate) fn inspect(
    text: &str,
    spec: &Specification,
    legacy: Option<&Profile>,
) -> Result<Vec<Capability>> {
    let document: Value =
        serde_json::from_str(text).map_err(|_| Error::Spec("无法解析设备规格".into()))?;
    let mut result = Vec::new();
    for service in document["services"].as_array().into_iter().flatten() {
        let service_name = urn_name(&service["type"]);
        if [
            "device-information",
            "self-check",
            "air-condition-outlet-matching",
            "aqi",
        ]
        .contains(&service_name.as_str())
        {
            continue;
        }
        let Some(siid) = iid(service) else {
            continue;
        };
        for raw in service["properties"].as_array().into_iter().flatten() {
            let Some(piid) = iid(raw) else {
                continue;
            };
            let name = urn_name(&raw["type"]);
            if name.is_empty() {
                continue;
            }
            let property = spec
                .properties
                .iter()
                .find(|p| p.siid == siid && p.piid == piid);
            let mapping_missing = legacy.is_some_and(|profile| {
                !profile
                    .properties
                    .iter()
                    .any(|p| p.siid == siid && p.piid == piid)
            });
            let writes = raw["access"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v == "write"));
            let reason = if mapping_missing {
                Some(Reason::LegacyMapping)
            } else if property.is_none() {
                Some(Reason::UnavailableFeature)
            } else if writes && !property.is_some_and(|p| p.writable) {
                Some(if legacy.is_some() {
                    Reason::LegacyMapping
                } else {
                    Reason::IncompleteParameters
                })
            } else {
                None
            };
            if let Some(reason) = reason {
                result.push(capability(
                    raw,
                    siid,
                    piid,
                    &service_name,
                    false,
                    alternate_control(&name, reason, spec),
                ));
            }
        }
        for raw in service["actions"].as_array().into_iter().flatten() {
            let Some(aiid) = iid(raw) else {
                continue;
            };
            let name = urn_name(&raw["type"]);
            if name.is_empty() {
                continue;
            }
            if spec
                .actions
                .iter()
                .any(|a| a.siid == siid && a.aiid == aiid)
            {
                continue;
            }
            let admitted = service_name == spec.kind
                && ["start-cook", "cancel-cooking", "pause", "resume-cook"]
                    .contains(&name.as_str())
                || spec.kind == "vacuum" && crate::vacuum::supports_action(&service_name, &name);
            let reason = if admitted && legacy.is_some() {
                Reason::LegacyMapping
            } else if admitted {
                Reason::IncompleteParameters
            } else {
                Reason::UnavailableOperation
            };
            result.push(capability(
                raw,
                siid,
                aiid,
                &service_name,
                true,
                alternate_control(&name, reason, spec),
            ));
        }
    }
    Ok(result)
}

fn iid(value: &Value) -> Option<u32> {
    u32::try_from(value["iid"].as_u64()?).ok()
}
fn capability(
    raw: &Value,
    siid: u32,
    iid: u32,
    service: &str,
    is_action: bool,
    reason: Reason,
) -> Capability {
    let name = urn_name(&raw["type"]);
    let en = raw["description"].as_str().unwrap_or(&name).to_owned();
    Capability {
        id: format!("{}:{siid}:{iid}", if is_action { "a" } else { "p" }),
        service: service.into(),
        name,
        description: raw["comment"].as_str().unwrap_or(&en).into(),
        description_en: en,
        is_action,
        reason,
    }
}
fn alternate_control(name: &str, reason: Reason, spec: &Specification) -> Reason {
    let writable = |service: &str, name: &str| {
        spec.properties
            .iter()
            .any(|p| p.service == service && p.name == name && p.writable)
    };
    match name {
        "toggle"
            if writable(&spec.kind, "on")
                || (spec.kind == "outlet" && writable("switch", "on")) =>
        {
            Reason::PowerSwitch
        }
        "brightness-delta" if writable("light", "brightness") => Reason::BrightnessSlider,
        "ct-delta" | "ct-adjust-alexa" if writable("light", "color-temperature") => {
            Reason::ColorTemperatureControl
        }
        _ => reason,
    }
}
