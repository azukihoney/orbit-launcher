//! Measurement only, compiled exclusively with review-test.
use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};
pub fn event(kind: &str, value: serde_json::Value) {
    if let Some(path) = std::env::var_os("ORBIT_STAGE2_PROBE")
        && let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path)
    {
        let _ = writeln!(f, "{}", serde_json::json!({"kind":kind,"value":value}));
    }
}
pub fn font(kind: &str, units: &[u16]) {
    event(kind, serde_json::json!(units));
}
pub fn checkpoint(name: &str) {
    if std::env::var("ORBIT_STAGE2_PAUSE").ok().as_deref() != Some(name) {
        return;
    }
    let Some(base) = std::env::var_os("ORBIT_STAGE2_PROBE") else {
        return;
    };
    let base = PathBuf::from(base);
    let _ = std::fs::write(base.with_extension("paused"), name);
    let start = Instant::now();
    while !base.with_extension("resume").exists() && start.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(20));
    }
}
