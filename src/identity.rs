//! Frozen v1 identity protocol. DefaultHasher is deliberately excluded here.
use std::{
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Globalization::*, Security::Cryptography::*, Storage::FileSystem::GetFullPathNameW,
};

pub fn sha256(bytes: &[u8]) -> Result<[u8; 32], String> {
    let length = u32::try_from(bytes.len()).map_err(|_| "Input too large for SHA256")?;
    let mut result = [0u8; 32];
    let status = unsafe {
        BCryptHash(
            BCRYPT_SHA256_ALG_HANDLE,
            null(),
            0,
            bytes.as_ptr(),
            length,
            result.as_mut_ptr(),
            32,
        )
    };
    if status < 0 {
        return Err(format!("SHA256 failed: 0x{:08X}", status as u32));
    }
    Ok(result)
}
pub struct Identity {
    pub path: PathBuf,
    pub class: String,
    pub mutex: String,
}
pub fn unprefixed(path: &Path) -> String {
    let text = path.as_os_str().to_string_lossy().replace('/', "\\");
    if let Some(tail) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{tail}")
    } else {
        text.strip_prefix(r"\\?\").unwrap_or(&text).into()
    }
}
impl Identity {
    pub fn new(path: &Path) -> Result<Self, String> {
        let raw: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
        let size = unsafe { GetFullPathNameW(raw.as_ptr(), 0, null_mut(), null_mut()) };
        if size == 0 {
            return Err("Unable to normalize settings path / 設定パスを正規化できません".into());
        }
        let mut full = vec![0u16; size as usize + 1];
        let n = unsafe {
            GetFullPathNameW(
                raw.as_ptr(),
                full.len() as u32,
                full.as_mut_ptr(),
                null_mut(),
            )
        };
        if n == 0 || n as usize >= full.len() {
            return Err("Settings path normalization failed".into());
        }
        let mut ancestor = PathBuf::from(OsString::from_wide(&full[..n as usize]));
        let mut tail = vec![];
        let resolved = loop {
            match canonicalize_path(&ancestor) {
                Ok(path) => break path,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let Some(name) = ancestor.file_name() else {
                        return Err(resolution_error(&e));
                    };
                    tail.push(name.to_os_string());
                    if !ancestor.pop() {
                        return Err(resolution_error(&e));
                    }
                }
                Err(e) => {
                    return Err(resolution_error(&e));
                }
            }
        };
        let mut path = resolved;
        for part in tail.into_iter().rev() {
            path.push(part);
        }
        let plain = unprefixed(&path);
        let input: Vec<_> = plain.encode_utf16().collect();
        let length = unsafe {
            LCMapStringEx(
                LOCALE_NAME_INVARIANT,
                LCMAP_UPPERCASE,
                input.as_ptr(),
                input.len() as i32,
                null_mut(),
                0,
                null(),
                null(),
                0,
            )
        };
        if length == 0 {
            return Err("Unable to normalize settings path case".into());
        }
        let mut mapped = vec![0u16; length as usize];
        let count = unsafe {
            LCMapStringEx(
                LOCALE_NAME_INVARIANT,
                LCMAP_UPPERCASE,
                input.as_ptr(),
                input.len() as i32,
                mapped.as_mut_ptr(),
                length,
                null(),
                null(),
                0,
            )
        };
        if count != length {
            return Err("Settings path case conversion failed".into());
        }
        let mut bytes = b"OrbitLauncher.ConfigIdentity.v1\0".to_vec();
        for unit in mapped {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let digest: String = sha256(&bytes)?.iter().map(|v| format!("{v:02x}")).collect();
        Ok(Self {
            path: PathBuf::from(plain),
            class: format!("OrbitRust-v1-{digest}"),
            mutex: format!("Local\\OrbitLauncher.Config.v1.{digest}"),
        })
    }
}
fn canonicalize_path(path: &Path) -> std::io::Result<PathBuf> {
    #[cfg(feature = "review-test")]
    {
        crate::stage3_probe::canonicalize(path)
    }
    #[cfg(not(feature = "review-test"))]
    {
        std::fs::canonicalize(path)
    }
}
fn resolution_error(error: &std::io::Error) -> String {
    let code = error
        .raw_os_error()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "unavailable".into());
    format!(
        "Unable to resolve the settings real path. OS error {code}: {error}. Startup cancelled. / 設定の実パスを解決できません。OSエラー番号: {code}。設定を変更せず起動を中止します。"
    )
}
