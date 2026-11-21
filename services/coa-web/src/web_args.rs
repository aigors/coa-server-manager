//! Arguments and results of a desktop command over HTTP, as Tauri passes them: one JSON object of named arguments, the
//! result as JSON, an error as the value the command rejects with (a `UiError` or a message).

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

/// The argument `key`: a missing one reads as `null`, which is what an `Option` argument expects.
pub fn arg<T: DeserializeOwned>(args: &Value, key: &str) -> Result<T, Value> {
    let v = args.get(key).cloned().unwrap_or(Value::Null);
    serde_json::from_value(v).map_err(|e| json!(format!("invalid argument `{key}`: {e}")))
}

pub fn ok<T: Serialize>(v: T) -> Result<Value, Value> {
    serde_json::to_value(v).map_err(|e| json!(e.to_string()))
}

pub fn res<T: Serialize, E: Serialize>(r: Result<T, E>) -> Result<Value, Value> {
    match r {
        Ok(v) => ok(v),
        Err(e) => Err(serde_json::to_value(e).unwrap_or_else(|e| json!(e.to_string()))),
    }
}
