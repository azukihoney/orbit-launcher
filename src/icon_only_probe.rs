//! Local measurements only, compiled out of distribution builds.
use serde_json::{Value, json};
use std::{io::Write, sync::OnceLock};

pub fn fixture() -> &'static Value {
    static VALUE: OnceLock<Value> = OnceLock::new();
    VALUE.get_or_init(|| {
        std::env::var_os("ORBIT_ICON_ONLY_FIXTURE")
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .unwrap_or(Value::Null)
    })
}

pub fn event(kind: &str, value: Value) {
    // UI and the acquisition worker may append concurrently. Serialize one
    // complete JSON line, rather than letting Value::fmt interleave fragments.
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(path) = std::env::var_os("ORBIT_ICON_ONLY_PROBE")
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let line = format!("{}\n", json!({"kind": kind, "value": value}));
        let _ = file.write_all(line.as_bytes());
    }
}

pub fn paint_time(milliseconds: f64) {
    event("paint", json!({"ms": milliseconds}));
}

pub fn io(operation: &str, path: &str) -> bool {
    event("icon_io", json!({"operation":operation,"path":path}));
    // A test observing an accidentally unguarded UNC call must never contact it.
    let fail = &fixture()["fail_operation"];
    !crate::path_decisions::unc(path)
        && fail.as_str() != Some(operation)
        && !fail
            .as_array()
            .is_some_and(|values| values.iter().any(|value| value.as_str() == Some(operation)))
}
