use crate::{
    model::{Entry, expand},
    paint::{Icon, wide},
    path_decisions::{self, Existing, IconSource, Location},
};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::result::Result::Ok;
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Gdi::*, GdiPlus::*},
    Storage::FileSystem::*,
    System::Com::*,
    UI::{Shell::*, WindowsAndMessaging::*},
};
pub fn raise_window(hwnd: HWND) {
    unsafe {
        // A disabled owner must never cover its enabled modal popup.
        let mut target = hwnd;
        for _ in 0..16 {
            if windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(target) != 0 {
                break;
            }
            let popup = GetWindow(target, GW_ENABLEDPOPUP);
            if popup.is_null() || popup == target || IsWindowVisible(popup) == 0 {
                break;
            }
            target = popup;
        }
        if IsIconic(target) != 0 {
            ShowWindow(target, SW_RESTORE);
        }
        if target != hwnd {
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        // Foreground activation may be denied by Windows for a background gesture
        // watcher. TOPMOST changes visibility order independently of that decision.
        SetWindowPos(
            target,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE,
        );
        SetForegroundWindow(target);
    }
}

pub fn message(hwnd: HWND, text: &str, error: bool) {
    unsafe {
        MessageBoxW(
            hwnd,
            wide(text).as_ptr(),
            wide("Orbit Launcher").as_ptr(),
            MB_OK
                | if error {
                    MB_ICONERROR
                } else {
                    MB_ICONINFORMATION
                },
        );
    }
}
pub fn confirm(hwnd: HWND, text: &str) -> bool {
    unsafe {
        MessageBoxW(
            hwnd,
            wide(text).as_ptr(),
            wide("Orbit Launcher").as_ptr(),
            MB_YESNO | MB_ICONQUESTION,
        ) == IDYES
    }
}
pub fn owner_available(hwnd: HWND, tooltip: HWND) -> bool {
    #[cfg(feature = "review-test")]
    crate::stage2_probe::event("owner_scan", serde_json::Value::Null);
    unsafe {
        if windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(hwnd) == 0 {
            return false;
        }
        let mut owned = GetWindow(hwnd, GW_HWNDFIRST);
        while !owned.is_null() {
            // Our noninteractive tracking tooltip is an owned window too, but
            // must not prevent clicks, diagnostics or auto-hide.
            if owned != tooltip && GetWindow(owned, GW_OWNER) == hwnd && IsWindowVisible(owned) != 0
            {
                return false;
            }
            owned = GetWindow(owned, GW_HWNDNEXT);
        }
        true
    }
}

pub fn mouse_buttons() -> u8 {
    #[cfg(feature = "review-test")]
    if let Some(sample) = crate::outside_click_probe::sample() {
        return sample.buttons;
    }
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON, VK_XBUTTON1, VK_XBUTTON2]
        .into_iter()
        .enumerate()
        .fold(0, |bits, (i, key)| {
            bits | if unsafe { GetAsyncKeyState(key as i32) } < 0 {
                1 << i
            } else {
                0
            }
        })
}

pub fn orbit_at_point(point: POINT) -> bool {
    #[cfg(feature = "review-test")]
    if let Some(sample) = crate::outside_click_probe::sample() {
        return sample.inside;
    }
    unsafe {
        let hit = WindowFromPoint(point);
        if hit.is_null() {
            return false;
        }
        let root = GetAncestor(hit, GA_ROOTOWNER);
        if root.is_null() {
            return false;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(root, &mut pid);
        pid == windows_sys::Win32::System::Threading::GetCurrentProcessId()
    }
}

pub fn own_tray_at_point(hwnd: HWND, point: POINT) -> bool {
    #[cfg(feature = "review-test")]
    if let Some(sample) = crate::outside_click_probe::sample() {
        return sample.tray;
    }
    unsafe {
        let mut identifier: NOTIFYICONIDENTIFIER = std::mem::zeroed();
        identifier.cbSize = std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32;
        identifier.hWnd = hwnd;
        identifier.uID = 1;
        let mut rect: RECT = std::mem::zeroed();
        // Failed lookup does not exempt the tray, as explicitly approved.
        Shell_NotifyIconGetRect(&identifier, &mut rect) == 0
            && point.x >= rect.left
            && point.x < rect.right
            && point.y >= rect.top
            && point.y < rect.bottom
    }
}
pub fn open(entry: &Entry) -> Result<(), String> {
    let target = if entry.kind == "recycle_open" {
        "shell:RecycleBinFolder".into()
    } else {
        expand(&entry.target)
    };
    let cwd = if !entry.cwd.is_empty() {
        expand(&entry.cwd)
    } else {
        std::path::Path::new(&target)
            .parent()
            .filter(|p| p.is_absolute())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let (t, a, c) = (wide(&target), wide(&entry.args), wide(&cwd));
    // Windows owns the launched process. All argument buffers live for this synchronous call.
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            wide("open").as_ptr(),
            t.as_ptr(),
            if entry.args.is_empty() {
                null()
            } else {
                a.as_ptr()
            },
            if cwd.is_empty() { null() } else { c.as_ptr() },
            SW_SHOWNORMAL,
        )
    };
    if result as usize <= 32 {
        Err(format!(
            "Windows launch error {} / パス・引数・作業フォルダを確認してください",
            result as usize
        ))
    } else {
        Ok(())
    }
}
pub fn fullscreen() -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return false;
        }
        let mut class = [0u16; 128];
        let n = GetClassNameW(hwnd, class.as_mut_ptr(), 128);
        let class = String::from_utf16_lossy(&class[..n as usize]);
        if [
            "Progman",
            "WorkerW",
            "Shell_TrayWnd",
            "Shell_SecondaryTrayWnd",
        ]
        .contains(&class.as_str())
        {
            return false;
        }
        let mut rect: RECT = std::mem::zeroed();
        let mut monitor: MONITORINFO = std::mem::zeroed();
        monitor.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetWindowRect(hwnd, &mut rect) == 0
            || GetMonitorInfoW(
                MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            ) == 0
        {
            return false;
        }
        let m = monitor.rcMonitor;
        (rect.left - m.left).abs() <= 1
            && (rect.top - m.top).abs() <= 1
            && (rect.right - m.right).abs() <= 1
            && (rect.bottom - m.bottom).abs() <= 1
    }
}
pub fn work_area(hwnd: HWND) -> RECT {
    #[cfg(feature = "review-test")]
    if let Some(work) = crate::stage3_probe::window_work() {
        return work;
    }
    unsafe {
        let mut m: MONITORINFO = std::mem::zeroed();
        m.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut m) != 0 {
            m.rcWork
        } else {
            RECT {
                left: 0,
                top: 0,
                right: GetSystemMetrics(SM_CXSCREEN),
                bottom: GetSystemMetrics(SM_CYSCREEN),
            }
        }
    }
}
/// Query the DPI of the monitor containing this window, independently of its
/// possibly stale GetDpiForWindow value after a display-settings change.
pub fn window_monitor_dpi(hwnd: HWND) -> Option<(u32, [u16; 32])> {
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let (mut x, mut y) = (0, 0);
        let dpi = if !monitor.is_null()
            && windows_sys::Win32::UI::HiDpi::GetDpiForMonitor(
                monitor,
                windows_sys::Win32::UI::HiDpi::MDT_EFFECTIVE_DPI,
                &mut x,
                &mut y,
            ) >= 0
            && x > 0
        {
            Some(x)
        } else {
            None
        };
        let mut info: MONITORINFOEXW = std::mem::zeroed();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        let device = if !monitor.is_null() && GetMonitorInfoW(monitor, &mut info.monitorInfo) != 0 {
            Some(info.szDevice)
        } else {
            None
        };
        #[cfg(feature = "review-test")]
        return crate::stage3_probe::monitor_dpi(dpi)
            .zip(device.map(|d| crate::stage3_probe::monitor_device(d, monitor)));
        #[cfg(not(feature = "review-test"))]
        return dpi.zip(device);
    }
}

pub fn window_dpi(hwnd: HWND) -> u32 {
    let real = unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd) };
    #[cfg(feature = "review-test")]
    return crate::stage3_probe::window_dpi(real);
    #[cfg(not(feature = "review-test"))]
    return real;
}
/// Only reveal uses the pointer's monitor. Relayout continues to use work_area.
pub fn cursor_monitor(point: POINT) -> (RECT, Option<u32>) {
    #[cfg(feature = "review-test")]
    if !crate::stage3_probe::fixture().is_null() {
        return crate::stage3_probe::monitor_point(point);
    }
    unsafe {
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        let work = if GetMonitorInfoW(monitor, &mut info) != 0 {
            info.rcWork
        } else {
            RECT {
                left: 0,
                top: 0,
                right: GetSystemMetrics(SM_CXSCREEN),
                bottom: GetSystemMetrics(SM_CYSCREEN),
            }
        };
        let (mut x, mut y) = (0, 0);
        let result = windows_sys::Win32::UI::HiDpi::GetDpiForMonitor(
            monitor,
            windows_sys::Win32::UI::HiDpi::MDT_EFFECTIVE_DPI,
            &mut x,
            &mut y,
        );
        (work, if result >= 0 && x > 0 { Some(x) } else { None })
    }
}
pub fn file_icon(entry: &Entry) -> Icon {
    unsafe {
        let com = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32);
        let target = expand(&entry.target);
        let remote = target.starts_with(r"\\") || target.starts_with("//");
        if com >= 0
            && path_decisions::may_read_link(&entry.kind, &target)
            && let Some(link) = read_link_for_icon(&target)
        {
            let icon_path = expand(&link.icon_path);
            let link_target = expand(&link.target);
            let icon_exists =
                !path_decisions::unc(&icon_path) && std::path::Path::new(&icon_path).is_file();
            let target_exists =
                !path_decisions::unc(&link_target) && std::path::Path::new(&link_target).exists();
            let source = path_decisions::icon_source(
                &entry.kind,
                &target,
                Some((&icon_path, icon_exists)),
                Some((&link_target, target_exists)),
            );
            if source == IconSource::Explicit {
                let mut large = null_mut();
                let count = ExtractIconExW(
                    wide(&icon_path).as_ptr(),
                    link.icon_index,
                    &mut large,
                    null_mut(),
                    1,
                );
                if count > 0 && !large.is_null() {
                    if com >= 0 {
                        CoUninitialize();
                    }
                    return Icon(large);
                }
                if !large.is_null() {
                    DestroyIcon(large);
                }
            }
            if (source == IconSource::LinkTarget || source == IconSource::Explicit)
                && let Some(icon) = shell_file_icon(&link_target, FILE_ATTRIBUTE_NORMAL, false)
            {
                if com >= 0 {
                    CoUninitialize();
                }
                return Icon(icon);
            }
        }
        let mut info: SHFILEINFOW = std::mem::zeroed();
        let attributes = if entry.kind == "folder" {
            FILE_ATTRIBUTE_DIRECTORY
        } else {
            FILE_ATTRIBUTE_NORMAL
        };
        let flags = SHGFI_ICON | SHGFI_LARGEICON | if remote { SHGFI_USEFILEATTRIBUTES } else { 0 };
        let mut result = SHGetFileInfoW(
            wide(&target).as_ptr(),
            attributes,
            &mut info,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        );
        if result == 0 {
            result = SHGetFileInfoW(
                wide(if entry.kind == "folder" {
                    "folder"
                } else {
                    "file.exe"
                })
                .as_ptr(),
                attributes,
                &mut info,
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON | SHGFI_USEFILEATTRIBUTES,
            );
        }
        if com >= 0 {
            CoUninitialize();
        }
        Icon(if result == 0 { null_mut() } else { info.hIcon })
    }
}
#[path = "large_icon.rs"]
mod large_icon;
pub fn large_file_icon(entry: &Entry, pixels: u32) -> Icon {
    large_icon::acquire(entry, pixels)
}

pub fn location_enabled(entry: &Entry) -> bool {
    let target = expand(&entry.target);
    path_decisions::location_eligible(&target)
}

pub fn location_for_entry(entry: &Entry) -> Location {
    let target = expand(&entry.target);
    if !path_decisions::location_eligible(&target) {
        return Location::Disabled;
    }
    // UNC metadata is queried only after the user selected the command.
    let path = std::path::Path::new(&target);
    let existing = if path.is_file() {
        Existing::File
    } else if path.is_dir() {
        Existing::Folder
    } else {
        Existing::Missing
    };
    path_decisions::location(&target, &entry.kind, existing)
}

pub fn open_location(location: &Location) -> Result<(), String> {
    let (folder, item) = match location {
        Location::Select { folder, item } => (folder.as_str(), Some(item.as_str())),
        Location::Open { folder } => (folder.as_str(), None),
        _ => return Err("File or folder not found / ファイルまたはフォルダが見つかりません。パスを確認してください。".into()),
    };
    #[cfg(feature = "review-test")]
    {
        let output = std::env::var_os("ORBIT_REVIEW_LOCATION_OUTPUT")
            .ok_or("Review location output is not set")?;
        let value = serde_json::json!({"folder": folder, "item": item});
        std::fs::write(output, value.to_string()).map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(feature = "review-test"))]
    unsafe {
        let mut folder_pidl = null_mut();
        if SHParseDisplayName(
            wide(folder).as_ptr(),
            null_mut(),
            &mut folder_pidl,
            0,
            null_mut(),
        ) < 0
        {
            return Err(format!(
                "Could not open file location / ファイルの場所を開けませんでした (Windows error {})",
                GetLastError()
            ));
        }
        let mut item_pidl = null_mut();
        let result = if let Some(item) = item {
            if SHParseDisplayName(
                wide(item).as_ptr(),
                null_mut(),
                &mut item_pidl,
                0,
                null_mut(),
            ) < 0
            {
                -1
            } else {
                let child = ILFindLastID(item_pidl) as *const _;
                SHOpenFolderAndSelectItems(folder_pidl, 1, &child, 0)
            }
        } else {
            SHOpenFolderAndSelectItems(folder_pidl, 0, null(), 0)
        };
        if !item_pidl.is_null() {
            CoTaskMemFree(item_pidl.cast());
        }
        CoTaskMemFree(folder_pidl.cast());
        if result < 0 {
            Err(format!(
                "Could not open file location / ファイルの場所を開けませんでした (Windows HRESULT {result:#x})"
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(feature = "review-test")]
pub fn record_location_menu(menu: HMENU) {
    let Some(output) = std::env::var_os("ORBIT_REVIEW_MENU_OUTPUT") else {
        return;
    };
    unsafe {
        let count = GetMenuItemCount(menu);
        let rows: Vec<_> = (0..count)
            .map(|index| {
                let state = GetMenuState(menu, index as u32, MF_BYPOSITION);
                let mut label = [0u16; 256];
                let length = GetMenuStringW(
                    menu,
                    index as u32,
                    label.as_mut_ptr(),
                    label.len() as i32,
                    MF_BYPOSITION,
                );
                let child = GetSubMenu(menu, index);
                let children: Vec<_> = if child.is_null() {
                    Vec::new()
                } else {
                    (0..GetMenuItemCount(child))
                        .map(|i| {
                            let state = GetMenuState(child, i as u32, MF_BYPOSITION);
                            let mut label = [0u16; 256];
                            let length = GetMenuStringW(
                                child,
                                i as u32,
                                label.as_mut_ptr(),
                                label.len() as i32,
                                MF_BYPOSITION,
                            );
                            serde_json::json!({"id":GetMenuItemID(child, i),
                            "label":String::from_utf16_lossy(&label[..length as usize]),
                            "checked":state & MF_CHECKED != 0,
                            "enabled":state & (MF_GRAYED | MF_DISABLED) == 0})
                        })
                        .collect()
                };
                serde_json::json!({
                    "id": GetMenuItemID(menu, index) as u32,
                    "enabled": state & (MF_GRAYED | MF_DISABLED) == 0,
                    "label": String::from_utf16_lossy(&label[..length as usize]),
                    "checked": state & MF_CHECKED != 0,
                    "children": children,
                })
            })
            .collect();
        let _ = std::fs::write(output, serde_json::Value::Array(rows).to_string());
    }
}

unsafe fn shell_file_icon(path: &str, attributes: u32, remote: bool) -> Option<HICON> {
    unsafe {
        let mut info: SHFILEINFOW = std::mem::zeroed();
        let result = SHGetFileInfoW(
            wide(path).as_ptr(),
            attributes,
            &mut info,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON | if remote { SHGFI_USEFILEATTRIBUTES } else { 0 },
        );
        (result != 0 && !info.hIcon.is_null()).then_some(info.hIcon)
    }
}

const SHELL_LINK: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0x00021401,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};
const SHELL_LINK_W: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0x000214f9,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};
const PERSIST_FILE: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0x0000010b,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};

struct ComPtr(*mut c_void);
impl ComPtr {
    unsafe fn method(&self, index: usize) -> usize {
        unsafe { *(*(self.0 as *mut *mut usize)).add(index) }
    }
}
impl Drop for ComPtr {
    fn drop(&mut self) {
        unsafe {
            let release: unsafe extern "system" fn(*mut c_void) -> u32 =
                std::mem::transmute(self.method(2));
            release(self.0);
        }
    }
}
struct LinkDetails {
    icon_path: String,
    icon_index: i32,
    target: String,
}

unsafe fn read_link_for_icon(path: &str) -> Option<LinkDetails> {
    #[cfg(feature = "review-test")]
    if let Some(output) = std::env::var_os("ORBIT_REVIEW_LINK_READ_OUTPUT") {
        let _ = std::fs::write(output, path);
        return None;
    }
    unsafe { read_link(path) }
}

unsafe fn read_link(path: &str) -> Option<LinkDetails> {
    unsafe {
        let mut raw = null_mut();
        if CoCreateInstance(
            &SHELL_LINK,
            null_mut(),
            CLSCTX_INPROC_SERVER,
            &SHELL_LINK_W,
            &mut raw,
        ) < 0
            || raw.is_null()
        {
            return None;
        }
        let link = ComPtr(raw);
        let mut persist_raw = null_mut();
        let query: unsafe extern "system" fn(
            *mut c_void,
            *const windows_sys::core::GUID,
            *mut *mut c_void,
        ) -> i32 = std::mem::transmute(link.method(0));
        if query(link.0, &PERSIST_FILE, &mut persist_raw) < 0 || persist_raw.is_null() {
            return None;
        }
        let persist = ComPtr(persist_raw);
        let load: unsafe extern "system" fn(*mut c_void, *const u16, u32) -> i32 =
            std::mem::transmute(persist.method(5));
        if load(persist.0, wide(path).as_ptr(), 0) < 0 {
            return None;
        }
        let mut icon_path = [0u16; 32768];
        let mut icon_index = 0;
        // IShellLinkW::GetIconLocation and IShellLinkW::GetPath use fixed vtable slots.
        let get_icon: unsafe extern "system" fn(*mut c_void, *mut u16, i32, *mut i32) -> i32 =
            std::mem::transmute(link.method(16));
        if get_icon(
            link.0,
            icon_path.as_mut_ptr(),
            icon_path.len() as i32,
            &mut icon_index,
        ) < 0
        {
            icon_path[0] = 0;
        }
        let mut target = [0u16; 32768];
        let get_path: unsafe extern "system" fn(
            *mut c_void,
            *mut u16,
            i32,
            *mut WIN32_FIND_DATAW,
            u32,
        ) -> i32 = std::mem::transmute(link.method(3));
        if get_path(
            link.0,
            target.as_mut_ptr(),
            target.len() as i32,
            null_mut(),
            0,
        ) < 0
        {
            target[0] = 0;
        }
        let from_wide = |v: &[u16]| {
            String::from_utf16_lossy(&v[..v.iter().position(|&c| c == 0).unwrap_or(v.len())])
        };
        Some(LinkDetails {
            icon_path: from_wide(&icon_path),
            icon_index,
            target: from_wide(&target),
        })
    }
}
pub fn tray_icon() -> Icon {
    unsafe {
        let mut bitmap = null_mut();
        let mut graphics = null_mut();
        let mut brush = null_mut();
        let mut pen = null_mut();
        GdipCreateBitmapFromScan0(32, 32, 0, 0x26200a, null(), &mut bitmap);
        GdipGetImageGraphicsContext(bitmap.cast(), &mut graphics);
        GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias);
        GdipCreateSolidFill(0xff3979c5, &mut brush);
        GdipFillEllipse(graphics, brush.cast(), 1., 1., 30., 30.);
        GdipCreatePen1(0xffffffff, 1.8, UnitPixel, &mut pen);
        GdipDrawEllipse(graphics, pen, 8., 8., 16., 16.);
        GdipDeleteBrush(brush.cast());
        GdipCreateSolidFill(0xffffffff, &mut brush);
        GdipFillEllipse(graphics, brush.cast(), 22., 6., 4., 4.);
        let mut icon = null_mut();
        GdipCreateHICONFromBitmap(bitmap, &mut icon);
        GdipDeleteBrush(brush.cast());
        GdipDeletePen(pen);
        GdipDeleteGraphics(graphics);
        GdipDisposeImage(bitmap.cast());
        Icon(icon)
    }
}
#[cfg(not(feature = "review-test"))]
pub fn empty_bin_async(hwnd: HWND, callback: u32, generation: u64) {
    let owner = hwnd as usize;
    std::thread::spawn(move || unsafe {
        let initialized = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) >= 0;
        let result = SHEmptyRecycleBinW(owner as HWND, null(), 0);
        if initialized {
            CoUninitialize();
        }
        // Native confirmation remains enabled. Completion is never automatically retried.
        PostMessageW(
            owner as HWND,
            callback,
            result as usize,
            generation as isize,
        );
    });
}
// This entire replacement is absent from default/release distribution builds.
// It calls no deletion API. Its native confirmation exercises disabled-owner
// and cross-thread popup behavior; completion is asynchronous like the real API.
#[cfg(feature = "review-test")]
pub fn empty_bin_async(hwnd: HWND, callback: u32, generation: u64) {
    let owner = hwnd as usize;
    std::thread::spawn(move || unsafe {
        let result = MessageBoxW(
            owner as HWND,
            wide("Review-only recycle substitute. Yes: success; No: failure; Cancel: cancelled. No files are deleted.").as_ptr(),
            wide("Orbit review recycle substitute").as_ptr(),
            MB_YESNOCANCEL,
        );
        std::thread::sleep(std::time::Duration::from_millis(600));
        PostMessageW(
            owner as HWND,
            callback,
            match result {
                IDYES => 0,
                IDNO => 0x80004005u32 as usize,
                _ => 0x800704c7u32 as usize,
            },
            generation as isize,
        );
    });
}
pub fn capture(
    hwnd: HWND,
    path: &std::path::Path,
    draw: impl FnOnce(&crate::paint::Canvas),
) -> Result<(), String> {
    unsafe {
        let mut rect: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut rect);
        let dc = GetDC(hwnd);
        let mem = CreateCompatibleDC(dc);
        let bitmap = CreateCompatibleBitmap(dc, rect.right, rect.bottom);
        let old = SelectObject(mem, bitmap);
        {
            let canvas = crate::paint::Canvas::new(mem);
            canvas.clear(0xfff6f7fa);
            draw(&canvas);
        }
        let mut image = null_mut();
        GdipCreateBitmapFromHBITMAP(bitmap, null_mut(), &mut image);
        let encoder = windows_sys::core::GUID {
            data1: 0x557cf406,
            data2: 0x1a04,
            data3: 0x11d3,
            data4: [0x9a, 0x73, 0, 0, 0xf8, 0x1e, 0xf3, 0x2e],
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let status = GdipSaveImageToFile(
            image.cast(),
            wide(&path.to_string_lossy()).as_ptr(),
            &encoder,
            null(),
        );
        GdipDisposeImage(image.cast());
        SelectObject(mem, old);
        DeleteObject(bitmap);
        DeleteDC(mem);
        ReleaseDC(hwnd, dc);
        if status == 0 {
            Ok(())
        } else {
            Err("Preview capture failed".into())
        }
    }
}

#[cfg(test)]
mod link_tests {
    use super::*;

    unsafe fn pixels(icon: HICON) -> Vec<u8> {
        unsafe {
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = 32;
            bmi.bmiHeader.biHeight = -32;
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            let mut bits = null_mut();
            let bitmap =
                CreateDIBSection(null_mut(), &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            assert!(!bitmap.is_null());
            let dc = CreateCompatibleDC(null_mut());
            assert!(!dc.is_null());
            let old = SelectObject(dc, bitmap);
            std::ptr::write_bytes(bits as *mut u8, 0, 32 * 32 * 4);
            assert_ne!(
                DrawIconEx(dc, 0, 0, icon, 32, 32, 0, null_mut(), DI_NORMAL),
                0
            );
            let result = std::slice::from_raw_parts(bits as *const u8, 32 * 32 * 4).to_vec();
            SelectObject(dc, old);
            DeleteObject(bitmap);
            DeleteDC(dc);
            result
        }
    }

    fn red_icon_bytes() -> Vec<u8> {
        let mut bytes = vec![0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 32, 0];
        bytes.extend_from_slice(&48u32.to_le_bytes());
        bytes.extend_from_slice(&22u32.to_le_bytes());
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(&1i32.to_le_bytes());
        bytes.extend_from_slice(&2i32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&32u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 16]);
        bytes.extend_from_slice(&[0, 0, 255, 255, 0, 0, 0, 0]);
        bytes
    }

    unsafe fn make_link(path: &std::path::Path, target: &str, icon: &str, index: i32) {
        unsafe {
            let mut raw = null_mut();
            assert!(
                CoCreateInstance(
                    &SHELL_LINK,
                    null_mut(),
                    CLSCTX_INPROC_SERVER,
                    &SHELL_LINK_W,
                    &mut raw
                ) >= 0
            );
            let link = ComPtr(raw);
            let set_path: unsafe extern "system" fn(*mut c_void, *const u16) -> i32 =
                std::mem::transmute(link.method(20));
            assert!(set_path(link.0, wide(target).as_ptr()) >= 0);
            if !icon.is_empty() {
                let set_icon: unsafe extern "system" fn(*mut c_void, *const u16, i32) -> i32 =
                    std::mem::transmute(link.method(17));
                assert!(set_icon(link.0, wide(icon).as_ptr(), index) >= 0);
            }
            let mut persist_raw = null_mut();
            let query: unsafe extern "system" fn(
                *mut c_void,
                *const windows_sys::core::GUID,
                *mut *mut c_void,
            ) -> i32 = std::mem::transmute(link.method(0));
            assert!(query(link.0, &PERSIST_FILE, &mut persist_raw) >= 0);
            let persist = ComPtr(persist_raw);
            let save: unsafe extern "system" fn(*mut c_void, *const u16, i32) -> i32 =
                std::mem::transmute(persist.method(6));
            assert!(save(persist.0, wide(&path.to_string_lossy()).as_ptr(), 1) >= 0);
        }
    }

    fn icon_only_test_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("Orbit-{name}-{}", Entry::new("", "", "launch").id));
        std::fs::create_dir(&dir).unwrap();
        dir
    }

    // Icon-only: a .lnk whose explicit icon is UNC must fall back to the generic
    // icon (safe side), not to the local target's icon.
    #[test]
    fn icon_only_unc_explicit_icon_uses_generic_not_target() {
        unsafe {
            assert!(CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) >= 0);
            let _gdi = crate::paint::GdiPlus::new().unwrap();
            let dir = icon_only_test_dir("icon-only-unc-explicit");
            let cmd = format!(r"{}\System32\cmd.exe", std::env::var("SystemRoot").unwrap());
            let guarded_link = dir.join("unc-explicit.lnk");
            let plain_link = dir.join("plain.lnk");
            make_link(
                &guarded_link,
                &cmd,
                r"\\orbit-test-unreachable.invalid\share\icon.ico",
                0,
            );
            make_link(&plain_link, &cmd, "", 0);
            let entry = |path: &std::path::Path| Entry::new("t", &path.to_string_lossy(), "launch");
            let guarded = large_file_icon(&entry(&guarded_link), 48);
            let target = large_file_icon(&entry(&plain_link), 48);
            let generic = large_file_icon(
                &Entry::new(
                    "t",
                    r"\\orbit-test-unreachable.invalid\share\app.exe",
                    "launch",
                ),
                48,
            );
            assert!(!guarded.0.is_null() && !target.0.is_null() && !generic.0.is_null());
            assert_eq!(pixels(guarded.0), pixels(generic.0));
            assert_ne!(pixels(guarded.0), pixels(target.0));
            std::fs::remove_dir_all(&dir).unwrap();
            CoUninitialize();
        }
    }

    // Icon-only: Shell-list icons (bat etc.) must come at the requested size,
    // not the 48px EXTRALARGE stretched to 60/72.
    #[test]
    fn icon_only_shell_list_icon_has_requested_pixels() {
        use windows_sys::Win32::Graphics::Gdi::{BITMAP, GetObjectW};
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetIconInfo, ICONINFO};
        unsafe {
            assert!(CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) >= 0);
            let _gdi = crate::paint::GdiPlus::new().unwrap();
            let dir = icon_only_test_dir("icon-only-shell-size");
            let bat = dir.join("t.bat");
            std::fs::write(&bat, b"@echo off\r\n").unwrap();
            for px in [48u32, 60, 72] {
                let icon = large_file_icon(&Entry::new("t", &bat.to_string_lossy(), "launch"), px);
                assert!(!icon.0.is_null(), "{px}");
                let mut info: ICONINFO = std::mem::zeroed();
                assert_ne!(GetIconInfo(icon.0, &mut info), 0);
                let mut bitmap: BITMAP = std::mem::zeroed();
                let source = if info.hbmColor.is_null() {
                    info.hbmMask
                } else {
                    info.hbmColor
                };
                assert_ne!(
                    GetObjectW(
                        source,
                        std::mem::size_of::<BITMAP>() as i32,
                        (&mut bitmap as *mut BITMAP).cast()
                    ),
                    0
                );
                if !info.hbmColor.is_null() {
                    DeleteObject(info.hbmColor);
                }
                DeleteObject(info.hbmMask);
                assert_eq!(
                    (bitmap.bmWidth, bitmap.bmHeight),
                    (px as i32, px as i32),
                    "{px}"
                );
            }
            std::fs::remove_dir_all(&dir).unwrap();
            CoUninitialize();
        }
    }

    #[test]
    fn created_shortcuts_read_explicit_and_target_paths_without_resolve() {
        unsafe {
            assert!(CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) >= 0);
            let dir = std::env::temp_dir().join(format!(
                "Orbit-link-test-{}",
                Entry::new("", "", "launch").id
            ));
            std::fs::create_dir(&dir).unwrap();
            let target = dir.join(format!("link-target-{}.txt", std::process::id()));
            let link = dir.join(format!("link-test-{}.lnk", std::process::id()));
            std::fs::write(&target, b"test").unwrap();
            let system_root = std::env::var("SystemRoot").unwrap();
            let shell32 = format!(r"{}\System32\shell32.dll", system_root);
            let ico = dir.join(format!("link-icon-{}.ico", std::process::id()));
            std::fs::write(&ico, red_icon_bytes()).unwrap();
            make_link(&link, &target.to_string_lossy(), &ico.to_string_lossy(), 0);
            let actual = file_icon(&Entry::new("test", &link.to_string_lossy(), "launch"));
            let mut direct = null_mut();
            assert_eq!(
                ExtractIconExW(
                    wide(&ico.to_string_lossy()).as_ptr(),
                    0,
                    &mut direct,
                    null_mut(),
                    1
                ),
                1
            );
            assert!(!direct.is_null());
            assert_eq!(pixels(actual.0), pixels(direct));
            DestroyIcon(direct);
            make_link(&link, &target.to_string_lossy(), &shell32, 43);
            let details = read_link(&link.to_string_lossy()).unwrap();
            assert_eq!(
                details.icon_path.to_ascii_lowercase(),
                shell32.to_ascii_lowercase()
            );
            assert_eq!(details.icon_index, 43);
            let forty_three = file_icon(&Entry::new("test", &link.to_string_lossy(), "launch"));
            let mut direct_43 = null_mut();
            let mut direct_0 = null_mut();
            assert_eq!(
                ExtractIconExW(wide(&shell32).as_ptr(), 43, &mut direct_43, null_mut(), 1),
                1
            );
            assert_eq!(
                ExtractIconExW(wide(&shell32).as_ptr(), 0, &mut direct_0, null_mut(), 1),
                1
            );
            assert_eq!(pixels(forty_three.0), pixels(direct_43));
            assert_ne!(pixels(direct_43), pixels(direct_0));
            DestroyIcon(direct_43);
            DestroyIcon(direct_0);
            make_link(&link, &target.to_string_lossy(), &shell32, -43);
            assert_eq!(read_link(&link.to_string_lossy()).unwrap().icon_index, -43);
            assert!(
                !file_icon(&Entry::new("test", &link.to_string_lossy(), "launch"))
                    .0
                    .is_null()
            );
            assert_eq!(
                details.target.to_ascii_lowercase(),
                target
                    .to_string_lossy()
                    .replace('/', "\\")
                    .to_ascii_lowercase()
            );
            let entry = Entry::new("test", &link.to_string_lossy(), "launch");
            assert!(!file_icon(&entry).0.is_null());
            make_link(&link, &target.to_string_lossy(), "", 0);
            let details = read_link(&link.to_string_lossy()).unwrap();
            assert!(details.icon_path.is_empty());
            assert!(!file_icon(&entry).0.is_null());
            make_link(
                &link,
                &target.to_string_lossy(),
                r"%SystemRoot%\System32\shell32.dll",
                43,
            );
            let details = read_link(&link.to_string_lossy()).unwrap();
            assert!(std::path::Path::new(&expand(&details.icon_path)).is_file());
            let expanded_icon = file_icon(&entry);
            let mut expected = null_mut();
            assert_eq!(
                ExtractIconExW(wide(&shell32).as_ptr(), 43, &mut expected, null_mut(), 1),
                1
            );
            assert_eq!(pixels(expanded_icon.0), pixels(expected));
            DestroyIcon(expected);
            let missing_icon = dir.join("missing-icon.ico");
            make_link(
                &link,
                &target.to_string_lossy(),
                &missing_icon.to_string_lossy(),
                0,
            );
            let details = read_link(&link.to_string_lossy()).unwrap();
            assert!(!std::path::Path::new(&details.icon_path).exists());
            assert!(!file_icon(&entry).0.is_null());
            make_link(&link, &shell32, &target.to_string_lossy(), 0);
            let fallback = file_icon(&entry);
            let from_target = file_icon(&Entry::new("test", &shell32, "launch"));
            assert!(
                pixels(fallback.0) == pixels(from_target.0),
                "iconless file did not fall back to target"
            );
            make_link(&link, &target.to_string_lossy(), &shell32, 43);
            std::fs::remove_file(&target).unwrap();
            assert!(!file_icon(&entry).0.is_null());
            let process = windows_sys::Win32::System::Threading::GetCurrentProcess();
            let before_gdi = windows_sys::Win32::System::Threading::GetGuiResources(process, 0);
            let before_user = windows_sys::Win32::System::Threading::GetGuiResources(process, 1);
            for _ in 0..64 {
                assert!(!file_icon(&entry).0.is_null());
            }
            let after_gdi = windows_sys::Win32::System::Threading::GetGuiResources(process, 0);
            let after_user = windows_sys::Win32::System::Threading::GetGuiResources(process, 1);
            assert!(
                after_gdi <= before_gdi + 4,
                "GDI objects: {before_gdi} -> {after_gdi}"
            );
            assert!(
                after_user <= before_user + 4,
                "USER objects: {before_user} -> {after_user}"
            );
            std::fs::remove_file(&link).unwrap();
            std::fs::remove_file(&ico).unwrap();
            std::fs::remove_dir(&dir).unwrap();
            CoUninitialize();
        }
    }
}

#[cfg(all(test, feature = "review-test"))]
mod location_substitute_tests {
    use super::*;
    #[test]
    fn records_folder_and_selected_item_without_opening_explorer() {
        let path = std::env::temp_dir().join(format!(
            "Orbit-location-test-{}.json",
            Entry::new("", "", "launch").id
        ));
        unsafe {
            std::env::set_var("ORBIT_REVIEW_LOCATION_OUTPUT", &path);
        }
        open_location(&Location::Select {
            folder: r"C:\demo".into(),
            item: r"C:\demo\x.lnk".into(),
        })
        .unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["folder"], r"C:\demo");
        assert_eq!(value["item"], r"C:\demo\x.lnk");
        std::fs::remove_file(path).unwrap();
        unsafe {
            std::env::remove_var("ORBIT_REVIEW_LOCATION_OUTPUT");
        }
    }
}
