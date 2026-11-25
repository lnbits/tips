use serde_json::Value;

pub fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
pub fn num(v: &Value, default: f64) -> f64 {
    match v {
        Value::Null => default,
        Value::Bool(b) => u8::from(*b) as f64,
        Value::String(s) if s.trim().is_empty() => 0.0,
        Value::String(s) => s.trim().parse().unwrap_or(f64::NAN),
        _ => v.as_f64().unwrap_or(f64::NAN),
    }
}
pub fn i(v: &Value) -> i64 {
    num(v, 0.0) as i64
}
pub fn alias<'a>(v: &'a Value, a: &str, b: &str) -> &'a Value {
    if v[a].is_null() {
        &v[b]
    } else {
        &v[a]
    }
}
pub fn clean(v: &Value, max: usize) -> String {
    s(v).split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}
pub fn required(v: &Value, name: &str, max: usize) -> Result<String, String> {
    let text = clean(v, max);
    if text.is_empty() {
        Err(format!("{name} is required."))
    } else {
        Ok(text)
    }
}
pub fn integer(v: &Value, default: i64, min: i64, max: i64) -> Result<i64, String> {
    let n = num(v, default as f64);
    if !n.is_finite() || n.fract() != 0. {
        return Err("value must be an integer.".into());
    }
    if n < (min as f64) {
        return Err(format!("value must be at least {min}."));
    }
    if n > (max as f64) {
        return Err(format!("value must be at most {max}."));
    }
    Ok(n as i64)
}
pub fn pagination(req: &Value, default: u32) -> (u32, u32) {
    let positive = |v: &Value, d: u32| {
        let n = num(v, d.into());
        if n.is_finite() && n >= 1. && n.fract() == 0. {
            n as u32
        } else {
            d
        }
    };
    let size = positive(&req["rowsPerPage"], default).min(1000);
    (size, (positive(&req["page"], 1) - 1).saturating_mul(size))
}
pub fn string_or<'a>(v: &'a Value, default: &'a str) -> &'a str {
    v.as_str().filter(|s| !s.is_empty()).unwrap_or(default)
}
pub fn first<'a>(values: &[&'a Value]) -> &'a Value {
    values
        .iter()
        .copied()
        .find(|v| !v.is_null() && v.as_str() != Some(""))
        .unwrap_or(&Value::Null)
}
pub fn merge(target: &mut Value, fields: Value) {
    if let (Some(row), Value::Object(fields)) = (target.as_object_mut(), fields) {
        row.extend(fields)
    }
}
