//! Vacuum action and status semantics, matching the native MijiaLAN client.
//! Status numbers vary by model; only the specification's descriptions give
//! them meaning. Room/zone actions require vendor parameters and stay hidden.
use crate::{Property, Specification};
use serde_json::Value;

pub fn supports_action(service: &str, name: &str) -> bool {
    match service {
        "vacuum" => matches!(
            name,
            "start-sweep"
                | "start-only-sweep"
                | "start-sweep-mop"
                | "start-mop"
                | "stop-sweeping"
                | "pause-sweeping"
                | "start-charge"
        ),
        "battery" => name == "start-charge",
        _ => false,
    }
}

fn normalized(description: &str) -> String {
    description
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphabetic())
        .collect()
}

pub fn is_cleaning(description: &str) -> bool {
    let d = normalized(description);
    (d.contains("sweep") && !d.contains("summon"))
        || ["mopping", "cleaning", "remoteclean", "twocleaning"].contains(&d.as_str())
}

pub fn is_docking(description: &str) -> bool {
    let d = normalized(description);
    (d.contains("charg") || d.contains("dock"))
        && !["error", "pause", "exit"]
            .iter()
            .any(|word| d.contains(word))
}

fn is_resting(description: &str) -> bool {
    let d = normalized(description);
    [
        "idle",
        "pause",
        "sleep",
        "dormant",
        "standby",
        "hibernate",
        "updat",
        "upgrad",
        "error",
        "fault",
        "shutdown",
    ]
    .iter()
    .any(|word| d.contains(word))
        || ["off", "none", "null"].contains(&d.as_str())
}

pub fn confirms(action: &str, status: Option<&str>, before: Option<&str>) -> bool {
    let Some(status) = status else { return false };
    match action {
        "start-sweep" | "start-only-sweep" | "start-sweep-mop" | "start-mop" => {
            is_cleaning(status)
                || (Some(status) != before && !is_resting(status) && !is_docking(status))
        }
        "start-charge" => is_docking(status),
        "stop-sweeping" | "pause-sweeping" | "pause" => !is_cleaning(status),
        _ => false,
    }
}

pub fn status_property(spec: &Specification) -> Option<&Property> {
    spec.properties.iter().find(|p| {
        p.service == "vacuum" && p.name == "status" && p.readable && !p.choices.is_empty()
    })
}

pub fn status_description<'a>(property: &'a Property, value: &Value) -> Option<&'a str> {
    property.choices.iter().find_map(|choice| {
        let matches = choice.value == *value
            || choice
                .value
                .as_f64()
                .zip(value.as_f64())
                .is_some_and(|(a, b)| a == b);
        matches.then_some(if choice.description_en.is_empty() {
            choice.description.as_str()
        } else {
            choice.description_en.as_str()
        })
    })
}
