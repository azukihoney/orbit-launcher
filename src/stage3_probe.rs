//! Stage3 boundary instrumentation; absent from default builds.
use serde_json::{Value, json};
use std::cell::Cell;
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::HMONITOR,
    UI::{Shell::*, WindowsAndMessaging::*},
};
thread_local! {
    static NOTIFIES: Cell<u32> = const { Cell::new(0) };
    static MOVED_WORK: Cell<Option<RECT>> = const { Cell::new(None) };
    static NUDGE_WINDOW_DPI: Cell<Option<u32>> = const { Cell::new(None) };
}
pub fn fixture() -> Value {
    std::env::var("ORBIT_STAGE3_FIXTURE")
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|v| serde_json::from_slice(&v).ok())
        .unwrap_or(Value::Null)
}
pub fn event(kind: &str, value: Value) {
    use std::io::Write;
    if let Some(path) = std::env::var_os("ORBIT_STAGE3_PROBE")
        && let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let _ = writeln!(f, "{}", json!({"kind":kind,"value":value}));
    }
}
pub fn rect(key: &str) -> Option<RECT> {
    let v = fixture();
    let a = v.get(key)?.as_array()?;
    Some(RECT {
        left: a[0].as_i64()? as i32,
        top: a[1].as_i64()? as i32,
        right: a[2].as_i64()? as i32,
        bottom: a[3].as_i64()? as i32,
    })
}
pub fn cursor(real: POINT) -> POINT {
    let v = fixture();
    if let Some(a) = v.get("cursor").and_then(|v| v.as_array()) {
        POINT {
            x: a[0].as_i64().unwrap() as i32,
            y: a[1].as_i64().unwrap() as i32,
        }
    } else {
        real
    }
}
pub fn initial_dpi(real: u32) -> u32 {
    fixture()["initial_dpi"]
        .as_u64()
        .map(|v| v as u32)
        .unwrap_or(real)
}
pub fn layout(layout: &crate::app::Layout, scale: f64) {
    fn r(r: crate::paint::R) -> Value {
        json!([r.x, r.y, r.w, r.h])
    }
    event(
        "layout",
        json!({"width":layout.width,"height":layout.height,"apps":r(layout.apps),
        "folders":r(layout.folders),"divider":layout.divider,"controls":r(layout.controls),"rail":layout.rail,"scale":scale}),
    );
}
pub fn window_work() -> Option<RECT> {
    MOVED_WORK.with(|v| v.get()).or_else(|| rect("window_work"))
}
pub fn placement(hwnd: HWND, x: i32, y: i32, w: i32, h: i32) {
    event("placement", json!([x, y, w, h]));
    let f = fixture();
    if f["follow_placement"].as_bool().unwrap_or(false)
        && let Some(work) = rect("cursor_work")
        && x + w / 2 >= work.left
        && x + w / 2 < work.right
        && y + h / 2 >= work.top
        && y + h / 2 < work.bottom
    {
        MOVED_WORK.with(|v| v.set(Some(work)));
    }
    if let Some(dpi) = f["during_reveal_dpi"].as_u64() {
        unsafe {
            send_dpi(hwnd, dpi as u32);
        }
    }
}
pub unsafe fn notify(op: u32, data: *const NOTIFYICONDATAW) -> i32 {
    let count = NOTIFIES.with(|n| {
        let v = n.get() + 1;
        n.set(v);
        v
    });
    let v = fixture();
    let fail = v["fail_restore"].as_bool().unwrap_or(false);
    let result = if fail && count > 1 {
        0
    } else {
        unsafe { Shell_NotifyIconW(op, data) }
    };
    event("tray", json!({"op":op,"result":result,"count":count}));
    result
}
pub fn canonicalize(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    if let Some(code) = fixture()["canonical_error"].as_i64() {
        return Err(std::io::Error::from_raw_os_error(code as i32));
    }
    std::fs::canonicalize(path)
}
pub unsafe fn send_dpi(hwnd: HWND, dpi: u32) {
    let r = rect("suggested").unwrap_or(RECT {
        left: 100,
        top: 100,
        right: 800,
        bottom: 600,
    });
    unsafe {
        crate::app::window_proc(
            hwnd,
            WM_DPICHANGED,
            (dpi | dpi << 16) as usize,
            &r as *const RECT as isize,
        );
    }
}
pub fn monitor_dpi(real: Option<u32>) -> Option<u32> {
    let fixture = fixture();
    if let Some(value) = fixture.get("nudge_monitor_dpi") {
        value.as_u64().map(|n| n as u32)
    } else {
        real
    }
}
pub fn monitor_device(real: [u16; 32], monitor: HMONITOR) -> [u16; 32] {
    let fixture = fixture();
    let handle = fixture["nudge_monitor_handle"]
        .as_u64()
        .unwrap_or(monitor as usize as u64);
    let name = fixture["nudge_monitor_device"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| String::from_utf16_lossy(&real));
    event("monitor_identity", json!({"device":name,"handle":handle}));
    let mut device = [0u16; 32];
    for (index, unit) in name.encode_utf16().take(31).enumerate() {
        device[index] = unit;
    }
    device
}
pub fn window_dpi(real: u32) -> u32 {
    fixture()
        .get("nudge_window_dpi")
        .and_then(|value| value.as_u64())
        .map(|value| value as u32)
        .unwrap_or_else(|| NUDGE_WINDOW_DPI.with(|value| value.get().unwrap_or(real)))
}
pub unsafe fn nudge_notice(hwnd: HWND, target: u32) {
    if fixture()["nudge_notice"].as_bool().unwrap_or(false) {
        NUDGE_WINDOW_DPI.with(|value| value.set(Some(target)));
        unsafe { send_dpi(hwnd, target) };
    }
}
pub fn monitor_point(point: POINT) -> (RECT, Option<u32>) {
    let _ = point;
    let v = fixture();
    (
        rect("cursor_work")
            .or_else(|| rect("window_work"))
            .unwrap_or(RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            }),
        v["target_dpi"].as_u64().map(|v| v as u32),
    )
}
