//! A mutex guards configuration before it is read, and throughout residency.
use crate::{
    identity::{Identity, unprefixed},
    paint::wide,
};
use std::{
    path::Path,
    ptr::null,
    time::{Duration, Instant},
};
use windows_sys::Win32::{Foundation::*, System::Threading::*, UI::WindowsAndMessaging::*};
pub const READY: &str = "OrbitLauncher.Ready.v1";
pub struct Instance {
    handle: HANDLE,
    owned: bool,
}
pub enum Start {
    Owner(Instance),
    Revealed,
}
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            if self.owned {
                ReleaseMutex(self.handle);
            }
            CloseHandle(self.handle);
        }
    }
}
// Migration insurance only. These are exactly stage1's rules, not the v1
// identity protocol. Compatibility is only verified with Rust 1.99.1.
fn legacy_class(path: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.to_lowercase().hash(&mut h);
    format!("OrbitRust-{:x}", h.finish())
}
fn reject_legacy(raw: &Path, id: &Identity) -> Result<(), String> {
    for path in [
        raw.as_os_str().to_string_lossy().into_owned(),
        unprefixed(&id.path),
    ] {
        if !unsafe { FindWindowW(wide(&legacy_class(&path)).as_ptr(), null()) }.is_null() {
            return Err("An older Orbit is running. Quit the older version before starting this version. / 旧版のOrbitが起動しています。旧版を終了してから新版を起動してください。".into());
        }
    }
    Ok(())
}
pub fn acquire(id: &Identity, raw: &Path) -> Result<Start, String> {
    reject_legacy(raw, id)?;
    let handle = unsafe { CreateMutexW(null(), 0, wide(&id.mutex).as_ptr()) };
    if handle.is_null() {
        return Err(format!(
            "Unable to create/open Orbit instance mutex ({}). Startup cancelled. / 起動ロックを作成・取得できません。起動を中止します。",
            unsafe { GetLastError() }
        ));
    }
    let mut guard = Instance {
        handle,
        owned: false,
    };
    let start = Instant::now();
    loop {
        let code = unsafe { WaitForSingleObject(handle, 100) };
        match code {
            WAIT_OBJECT_0 | WAIT_ABANDONED => {
                guard.owned = true;
                reject_legacy(raw, id)?;
                return Ok(Start::Owner(guard));
            }
            WAIT_TIMEOUT => {
                let hwnd = unsafe { FindWindowW(wide(&id.class).as_ptr(), null()) };
                if !hwnd.is_null() && unsafe { GetPropW(hwnd, wide(READY).as_ptr()) } as usize == 1
                {
                    let mut result = 0;
                    if unsafe {
                        SendMessageTimeoutW(
                            hwnd,
                            crate::app::REVEAL,
                            0,
                            0,
                            SMTO_ABORTIFHUNG | SMTO_BLOCK,
                            250,
                            &mut result,
                        )
                    } != 0
                    {
                        return Ok(Start::Revealed);
                    }
                }
                if start.elapsed() >= Duration::from_secs(10) {
                    return Err("Another Orbit instance is using these settings, but its window could not be shown within 10 seconds. Startup cancelled. / 別のOrbitが設定を使用中で、10秒以内に画面を再表示できませんでした。起動を中止します。".into());
                }
            }
            _ => {
                return Err(format!(
                    "Unable to acquire Orbit instance mutex ({}). Startup cancelled. / 起動ロックの待機に失敗しました。起動を中止します。",
                    unsafe { GetLastError() }
                ));
            }
        }
    }
}
