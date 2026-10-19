//! Typed access to JSON tool arguments, with errors that say what was expected.

use serde_json::Value;

pub(crate) fn str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    opt_str(args, key)?.ok_or_else(|| format!("missing required string argument \"{key}\""))
}

pub(crate) fn opt_str<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(other) => Err(format!("argument \"{key}\" must be a string, got {other}")),
    }
}

pub(crate) fn u64(args: &Value, key: &str) -> Result<u64, String> {
    opt_u64(args, key)?.ok_or_else(|| format!("missing required non-negative integer argument \"{key}\""))
}

pub(crate) fn opt_u64(args: &Value, key: &str) -> Result<Option<u64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| format!("argument \"{key}\" must be a non-negative integer, got {v}")),
    }
}

pub(crate) fn opt_f64(args: &Value, key: &str) -> Result<Option<f64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|f| f.is_finite()).map(Some).ok_or_else(|| format!("argument \"{key}\" must be a number, got {v}")),
    }
}

pub(crate) fn opt_bool(args: &Value, key: &str) -> Result<Option<bool>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(other) => Err(format!("argument \"{key}\" must be true or false, got {other}")),
    }
}
