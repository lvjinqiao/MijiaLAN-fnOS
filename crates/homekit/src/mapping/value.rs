//! Keep every emitted value within its advertised HAP format and constraints.
use serde_json::{Value, json};

pub(super) fn normalize_read(attribute: &Value, value: Value) -> Result<Value, i32> {
    normalize(attribute, value, false).map_err(|_| -70402)
}

pub(crate) fn validate_write(attribute: &Value, value: &Value) -> Result<Value, i32> {
    if !attribute["perms"]
        .as_array()
        .is_some_and(|p| p.iter().any(|p| p == "pw"))
    {
        return Err(-70404);
    }
    normalize(attribute, value.clone(), true).map_err(|_| -70410)
}

fn normalize(attribute: &Value, value: Value, writing: bool) -> Result<Value, ()> {
    let format = attribute["format"].as_str().ok_or(())?;
    if format == "bool" {
        return value
            .as_bool()
            .or_else(|| match value.as_f64() {
                Some(0.) => Some(false),
                Some(1.) => Some(true),
                _ => None,
            })
            .map(|v| json!(v))
            .ok_or(());
    }
    if format == "string" {
        let text = value.as_str().ok_or(())?;
        let limit = attribute["maxLen"].as_u64().unwrap_or(64) as usize;
        return if text.len() <= limit {
            Ok(value)
        } else {
            Err(())
        };
    }
    let (type_min, type_max, integral) = match format {
        "float" => (-(f32::MAX as f64), f32::MAX as f64, false),
        "int" => (i32::MIN as f64, i32::MAX as f64, true),
        "uint8" => (0., u8::MAX as f64, true),
        "uint16" => (0., u16::MAX as f64, true),
        "uint32" => (0., u32::MAX as f64, true),
        _ => return Err(()),
    };
    let min = attribute["minValue"]
        .as_f64()
        .unwrap_or(type_min)
        .max(type_min);
    let max = attribute["maxValue"]
        .as_f64()
        .unwrap_or(type_max)
        .min(type_max);
    let step = attribute["minStep"]
        .as_f64()
        .unwrap_or(if integral { 1. } else { 0. });
    if !min.is_finite() || !max.is_finite() || min > max || !step.is_finite() || step < 0. {
        return Err(());
    }
    let mut number = value.as_f64().filter(|v| v.is_finite()).ok_or(())?;
    if number < min || number > max {
        return Err(());
    }
    if let Some(valid) = attribute["valid-values"].as_array()
        && !valid.iter().any(|v| v.as_f64() == Some(number))
    {
        return Err(());
    }
    let base = attribute["minValue"].as_f64().unwrap_or(0.);
    if writing {
        if integral && number.fract().abs() > 1e-6 {
            return Err(());
        }
        if step > 0. {
            let units = (number - base) / step;
            if (units - units.round()).abs() > 1e-6 {
                return Err(());
            }
        }
    } else if step > 0. {
        number = base + ((number - base) / step).round() * step;
        if number > max {
            number = base + ((max - base) / step).floor() * step;
        }
        number = number.max(min);
    }
    if integral {
        number = number.round();
        if format == "int" {
            Ok(json!(number as i64))
        } else {
            Ok(json!(number as u64))
        }
    } else {
        Ok(json!(number))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incoming_values_are_checked_before_conversion_or_device_clamping() {
        let brightness = json!({"format":"int","perms":["pr","pw","ev"],"minValue":0,"maxValue":100,"minStep":1});
        for value in [json!(2.5), json!(101), json!("50"), Value::Null] {
            assert_eq!(validate_write(&brightness, &value), Err(-70410));
        }
        assert_eq!(validate_write(&brightness, &json!(50)), Ok(json!(50)));
        let modes = json!({"format":"uint8","perms":["pr","pw"],"minValue":0,"maxValue":2,"valid-values":[2]});
        assert_eq!(validate_write(&modes, &json!(0)), Err(-70410));
        assert_eq!(validate_write(&modes, &json!(2)), Ok(json!(2)));
    }
    #[test]
    fn outgoing_numeric_values_use_the_advertised_step_and_type() {
        let position = json!({"format":"uint8","minValue":0,"maxValue":100,"minStep":1});
        assert_eq!(
            normalize_read(&position, json!(73. / 255. * 100.)),
            Ok(json!(29))
        );
        let temperature = json!({"format":"float","minValue":-40,"maxValue":100,"minStep":0.1});
        let value = normalize_read(&temperature, json!(21.56))
            .unwrap()
            .as_f64()
            .unwrap();
        assert!((value - 21.6).abs() < 1e-8);
        assert_eq!(normalize_read(&temperature, json!(-3276.8)), Err(-70402));
        assert_eq!(normalize_read(&position, json!("50")), Err(-70402));
    }
}
