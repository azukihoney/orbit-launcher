//! Observation only. This module is absent from normal builds.
use serde_json::json;
use std::{
    fs::OpenOptions,
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    UI::{HiDpi::*, WindowsAndMessaging::*},
};

pub unsafe fn message(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) {
    let Some(path) = std::env::var_os("ORBIT_DPI_FOLLOW_LOG") else {
        return;
    };
    unsafe {
        let mut rect = RECT::default();
        let rect_ok = GetWindowRect(hwnd, &mut rect) != 0;
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let (mut dpi_x, mut dpi_y) = (0, 0);
        let monitor_ok = !monitor.is_null()
            && GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) >= 0;
        let setting = if msg == WM_SETTINGCHANGE && l != 0 {
            let mut raw = Vec::new();
            for index in 0..512 {
                let unit = *(l as *const u16).add(index);
                if unit == 0 {
                    break;
                }
                raw.push(unit);
            }
            Some(String::from_utf16_lossy(&raw))
        } else {
            None
        };
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let record = json!({
            "timestamp_ms": timestamp_ms, "pid": std::process::id(),
            "message": msg, "wparam": w, "lparam": l,
            "setting": setting, "visible": IsWindowVisible(hwnd) != 0,
            "window_dpi": GetDpiForWindow(hwnd),
            "monitor_dpi": if monitor_ok { Some([dpi_x,dpi_y]) } else { None },
            "monitor": monitor as usize,
            "rect": if rect_ok { Some([rect.left,rect.top,rect.right,rect.bottom]) } else { None },
        });
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{record}");
        }
    }
}
