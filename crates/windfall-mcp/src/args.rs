//! Reading a tool's arguments. Every tool checks its own: the schema a
//! client is shown documents them, and these say exactly what was wrong,
//! so the agent can correct the call.

use serde_json::{Map, Value};

pub type Args = Map<String, Value>;

static EMPTY: std::sync::OnceLock<Args> = std::sync::OnceLock::new();

/// The arguments object. Missing arguments are an empty object.
pub fn object(value: Option<&Value>) -> Result<&Args, String> {
    match value {
        None | Some(Value::Null) => Ok(EMPTY.get_or_init(Map::new)),
        Some(Value::Object(map)) => Ok(map),
        Some(other) => Err(format!("the arguments must be an object, not {other}")),
    }
}

/// Refuses arguments the tool does not take.
pub fn known(args: &Args, allowed: &[&str]) -> Result<(), String> {
    let unknown: Vec<&str> = args
        .keys()
        .map(String::as_str)
        .filter(|key| !allowed.contains(key))
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "unknown arguments {unknown:?}; this tool takes {allowed:?}"
        ))
    }
}

fn present<'a>(args: &'a Args, key: &str) -> Option<&'a Value> {
    args.get(key).filter(|value| !value.is_null())
}

pub fn id(args: &Args, key: &str) -> Result<u32, String> {
    optional_id(args, key)?.ok_or_else(|| format!("argument {key} is required"))
}

pub fn optional_id(args: &Args, key: &str) -> Result<Option<u32>, String> {
    present(args, key)
        .map(|value| {
            value
                .as_u64()
                .and_then(|id| u32::try_from(id).ok())
                .ok_or_else(|| format!("argument {key} must be an id, a whole number, not {value}"))
        })
        .transpose()
}

pub fn optional_number(args: &Args, key: &str) -> Result<Option<f64>, String> {
    present(args, key)
        .map(|value| {
            value
                .as_f64()
                .filter(|number| number.is_finite())
                .ok_or_else(|| format!("argument {key} must be a number, not {value}"))
        })
        .transpose()
}

pub fn optional_u32(
    args: &Args,
    key: &str,
    range: std::ops::RangeInclusive<u32>,
) -> Result<Option<u32>, String> {
    present(args, key)
        .map(|value| {
            value
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| range.contains(n))
                .ok_or_else(|| {
                    format!(
                        "argument {key} must be a whole number from {} to {}, not {value}",
                        range.start(),
                        range.end()
                    )
                })
        })
        .transpose()
}

pub fn optional_bool(args: &Args, key: &str) -> Result<Option<bool>, String> {
    present(args, key)
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| format!("argument {key} must be true or false, not {value}"))
        })
        .transpose()
}

pub fn text<'a>(args: &'a Args, key: &str, max: usize) -> Result<&'a str, String> {
    optional_text(args, key, max)?.ok_or_else(|| format!("argument {key} is required"))
}

pub fn optional_text<'a>(args: &'a Args, key: &str, max: usize) -> Result<Option<&'a str>, String> {
    present(args, key)
        .map(|value| {
            let text = value
                .as_str()
                .ok_or_else(|| format!("argument {key} must be text, not {value}"))?;
            if text.len() > max || text.chars().any(char::is_control) {
                return Err(format!(
                    "argument {key} must be at most {max} bytes of text with no control characters"
                ));
            }
            Ok(text)
        })
        .transpose()
}

pub fn value<'a>(args: &'a Args, key: &str) -> Result<&'a Value, String> {
    present(args, key).ok_or_else(|| format!("argument {key} is required"))
}

pub fn optional_value<'a>(args: &'a Args, key: &str) -> Option<&'a Value> {
    present(args, key)
}

pub fn list<'a>(args: &'a Args, key: &str, max: usize) -> Result<&'a [Value], String> {
    optional_list(args, key, max)?.ok_or_else(|| format!("argument {key} is required"))
}

pub fn optional_list<'a>(
    args: &'a Args,
    key: &str,
    max: usize,
) -> Result<Option<&'a [Value]>, String> {
    present(args, key)
        .map(|value| {
            let items = value
                .as_array()
                .ok_or_else(|| format!("argument {key} must be a list"))?;
            if items.is_empty() || items.len() > max {
                return Err(format!(
                    "argument {key} must hold 1 to {max} items, not {}",
                    items.len()
                ));
            }
            Ok(items.as_slice())
        })
        .transpose()
}

/// One item of a list argument, which must itself be an object.
pub fn item<'a>(value: &'a Value, key: &str, index: usize) -> Result<&'a Args, String> {
    value
        .as_object()
        .ok_or_else(|| format!("item {index} of {key} must be an object"))
}

/// A list of ids.
pub fn ids(args: &Args, key: &str, max: usize) -> Result<Vec<u32>, String> {
    list(args, key, max)?
        .iter()
        .map(|value| {
            value
                .as_u64()
                .and_then(|id| u32::try_from(id).ok())
                .ok_or_else(|| format!("{key} must hold ids, whole numbers, not {value}"))
        })
        .collect()
}
