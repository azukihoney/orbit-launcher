use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::{fs, io::Write};

type Extra = BTreeMap<String, serde_json::Value>;

#[cfg(test)]
mod icon_only_contract {
    use super::*;

    #[test]
    fn icon_only_default_is_explicit_false() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            serde_json::to_value(settings).unwrap()["app_icon_only"],
            false
        );
    }

    #[test]
    fn icon_only_rejects_non_boolean() {
        for value in ["null", "0", "\"true\""] {
            let input = format!("{{\"app_icon_only\":{value}}}");
            assert!(serde_json::from_str::<Settings>(&input).is_err(), "{input}");
        }
    }
    #[test]
    fn icon_only_checked_save_preserves_future_keys_and_rejects_external_change() {
        let folder = std::env::temp_dir().join(format!(
            "Orbit-icon-model-{}",
            Entry::new("", "", "launch").id
        ));
        fs::create_dir(&folder).unwrap();
        let path = folder.join("config.json");
        let mut config = Config {
            settings: serde_json::from_str(r#"{"app_icon_only":true,"future":42}"#).unwrap(),
            ..Config::default()
        };
        let (saved, snapshot) = save_checked(&path, &config, &Snapshot::Missing).unwrap();
        assert!(saved.settings.app_icon_only);
        assert_eq!(saved.settings.extra["future"], 42);
        let bytes = fs::read(&path).unwrap();
        assert_eq!(
            String::from_utf8(bytes.clone())
                .unwrap()
                .matches("\"app_icon_only\"")
                .count(),
            1
        );
        fs::write(&path, b"{}\n").unwrap();
        config.settings.app_icon_only = false;
        assert!(save_checked(&path, &config, &snapshot).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"{}\n");
        assert!(saved.settings.app_icon_only);
        fs::remove_dir_all(folder).unwrap();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub turns: f64,
    pub timeout: f64,
    pub min_radius: f64,
    pub max_radius: f64,
    pub direction: String,
    pub gesture_enabled: bool,
    pub auto_hide: bool,
    pub skip_fullscreen: bool,
    pub language: String,
    pub font_family: String,
    pub theme: String,
    pub app_font_px: i32,
    pub app_icon_only: bool,
    pub folder_font_px: i32,
    pub app_rows: usize,
    pub app_columns: usize,
    pub folder_rows: usize,
    pub folder_columns: usize,
    #[serde(flatten)]
    pub extra: Extra,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            turns: 1.5,
            timeout: 1.0,
            min_radius: 12.,
            max_radius: 800.,
            direction: "ccw".into(),
            gesture_enabled: true,
            auto_hide: true,
            skip_fullscreen: true,
            language: "ja".into(),
            font_family: "UDEV Gothic".into(),
            theme: "light".into(),
            app_font_px: 11,
            app_icon_only: false,
            folder_font_px: 10,
            app_rows: 2,
            app_columns: 6,
            folder_rows: 2,
            folder_columns: 6,
            extra: Extra::new(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub target: String,
    pub kind: String,
    #[serde(default)]
    pub args: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub cell: Option<[usize; 2]>,
    #[serde(flatten)]
    pub extra: Extra,
}
impl Entry {
    pub fn new(name: &str, target: &str, kind: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self {
            id: format!("{time:x}-{:x}", NEXT.fetch_add(1, Ordering::Relaxed)),
            name: name.into(),
            target: target.into(),
            kind: kind.into(),
            args: String::new(),
            cwd: String::new(),
            cell: None,
            extra: Extra::new(),
        }
    }
    pub fn recycle(&self) -> bool {
        self.kind.starts_with("recycle_")
    }
    pub fn label(&self, en: bool) -> String {
        let path = expand(&self.target);
        if self.kind == "folder" {
            return path
                .trim_end_matches(['\\', '/'])
                .rsplit(['\\', '/'])
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or(&path)
                .into();
        }
        let target = path.replace('/', "\\").to_lowercase();
        let windir = std::env::var("WINDIR")
            .unwrap_or_else(|_| r"C:\Windows".into())
            .to_lowercase();
        if let Some((parent, exe)) = target.rsplit_once('\\')
            && [
                windir.clone(),
                format!(r"{windir}\system32"),
                format!(r"{windir}\syswow64"),
            ]
            .contains(&parent.to_string())
        {
            let names = match exe {
                "notepad.exe" => Some(("メモ帳", "Notepad")),
                "calc.exe" => Some(("電卓", "Calculator")),
                "cmd.exe" => Some(("コマンドプロンプト", "Command Prompt")),
                "explorer.exe" => Some(("エクスプローラー", "File Explorer")),
                "mspaint.exe" => Some(("ペイント", "Paint")),
                "taskmgr.exe" => Some(("タスク マネージャー", "Task Manager")),
                "regedit.exe" => Some(("レジストリ エディター", "Registry Editor")),
                "control.exe" => Some(("コントロール パネル", "Control Panel")),
                _ => None,
            };
            if let Some((ja, english)) = names
                && (self.name == ja
                    || self.name == english
                    || (exe == "cmd.exe" && self.name == "コマンド"))
            {
                return if en { english } else { ja }.into();
            }
        }
        self.name.clone()
    }
}
pub fn expand(path: &str) -> String {
    let mut result = path.to_string();
    for (key, value) in std::env::vars() {
        // Windows environment variables are case insensitive.
        let token = format!("%{key}%");
        while let Some(index) = result
            .to_ascii_lowercase()
            .find(&token.to_ascii_lowercase())
        {
            result.replace_range(index..index + token.len(), &value);
            if value
                .to_ascii_lowercase()
                .contains(&token.to_ascii_lowercase())
            {
                break;
            }
        }
    }
    if let Some(tail) = result.strip_prefix('~') {
        result = format!(
            "{}{}",
            std::env::var("USERPROFILE").unwrap_or_default(),
            tail
        );
    }
    result
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Page {
    pub id: String,
    pub name: String,
    pub items: Vec<Entry>,
    #[serde(flatten)]
    pub extra: Extra,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    #[serde(default = "layout_version")]
    pub layout_version: u32,
    #[serde(default)]
    pub settings: Settings,
    pub pages: Vec<Page>,
    pub common: Vec<Entry>,
    #[serde(flatten)]
    pub extra: Extra,
}
fn layout_version() -> u32 {
    2
}
impl Default for Config {
    fn default() -> Self {
        let win = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
        let work = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            version: 1,
            layout_version: 2,
            settings: Settings::default(),
            pages: vec![Page {
                id: "main".into(),
                name: "アプリ".into(),
                extra: Extra::new(),
                items: vec![
                    Entry::new("メモ帳", &format!(r"{win}\System32\notepad.exe"), "launch"),
                    Entry::new("電卓", &format!(r"{win}\System32\calc.exe"), "launch"),
                    Entry::new("コマンド", &format!(r"{win}\System32\cmd.exe"), "launch"),
                    Entry::new(
                        "エクスプローラー",
                        &format!(r"{win}\explorer.exe"),
                        "launch",
                    ),
                ],
            }],
            common: vec![Entry::new("Work", &work.to_string_lossy(), "folder")],
            extra: Extra::new(),
        }
    }
}
impl Config {
    pub fn en(&self) -> bool {
        self.settings.language == "en"
    }
    pub fn group(&self, page: usize, common: bool) -> &Vec<Entry> {
        if common {
            &self.common
        } else {
            &self.pages[page].items
        }
    }
    pub fn group_mut(&mut self, page: usize, common: bool) -> &mut Vec<Entry> {
        if common {
            &mut self.common
        } else {
            &mut self.pages[page].items
        }
    }
    pub fn dimensions(&self, common: bool) -> (usize, usize) {
        let s = &self.settings;
        if common {
            (s.folder_rows, s.folder_columns)
        } else {
            (s.app_rows, s.app_columns)
        }
    }
    pub fn reserved(&self, common: bool, cell: [usize; 2]) -> bool {
        !common && cell[0] == 0 && cell[1] >= self.settings.app_columns.saturating_sub(2)
    }
    pub fn normalize(&mut self) -> Result<(), String> {
        if self.version != 1 || self.layout_version != 2 || self.pages.is_empty() {
            return Err("Unsupported configuration or missing page / 未対応の設定形式です".into());
        }
        let s = &self.settings;
        // Order is stable for both the editor and the importing binary.
        for (valid, message) in [
            (
                (1..=16).contains(&s.app_rows),
                "App rows must be between 1 and 16 / アプリの行数は1〜16の範囲で入力してください",
            ),
            (
                (2..=32).contains(&s.app_columns),
                "App columns must be between 2 and 32 / アプリの列数は2〜32の範囲で入力してください",
            ),
            (
                (1..=16).contains(&s.folder_rows),
                "Folder rows must be between 1 and 16 / フォルダの行数は1〜16の範囲で入力してください",
            ),
            (
                (1..=32).contains(&s.folder_columns),
                "Folder columns must be between 1 and 32 / フォルダの列数は1〜32の範囲で入力してください",
            ),
            (
                (8..=16).contains(&s.app_font_px),
                "App font size (px) must be between 8 and 16 / アプリ文字サイズ（px）は8〜16の範囲で入力してください",
            ),
            (
                (8..=16).contains(&s.folder_font_px),
                "Folder font size (px) must be between 8 and 16 / フォルダ文字サイズ（px）は8〜16の範囲で入力してください",
            ),
            (
                ["ja", "en"].contains(&s.language.as_str()),
                "Language must be ja or en / 表示言語はjaまたはenを指定してください",
            ),
            (
                ["light", "dark"].contains(&s.theme.as_str()),
                "Theme must be light or dark / テーマはlightまたはdarkを指定してください",
            ),
            (
                ["cw", "ccw", "both"].contains(&s.direction.as_str()),
                "Direction must be ccw, cw or both / 回転方向はccw、cw、bothのいずれかを指定してください",
            ),
            (
                !s.font_family.trim().is_empty() && s.font_family.chars().count() <= 128,
                "Font must contain 1 to 128 characters and not be blank / フォントは空白以外を含む1〜128文字で入力してください",
            ),
            (
                (1.5..=4.).contains(&s.turns),
                "Number of turns must be between 1.5 and 4 / 円を描く回数は1.5〜4の範囲で入力してください",
            ),
            (
                (0.3..=6.).contains(&s.timeout),
                "Time limit (seconds) must be between 0.3 and 6 / 制限時間（秒）は0.3〜6の範囲で入力してください",
            ),
            (
                (5.0..=400.).contains(&s.min_radius),
                "Minimum radius (screen px) must be between 5 and 400 / 最小半径（画面px）は5〜400の範囲で入力してください",
            ),
            (
                (30.0..=1500.).contains(&s.max_radius),
                "Maximum radius (screen px) must be between 30 and 1500 / 最大半径（画面px）は30〜1500の範囲で入力してください",
            ),
            (
                s.min_radius < s.max_radius,
                "Minimum radius (screen px) must be less than Maximum radius (screen px) / 最小半径（画面px）は最大半径（画面px）未満にしてください",
            ),
        ] {
            if !valid {
                return Err(message.into());
            }
        }
        let mut ids = HashSet::new();
        for e in self.pages.iter().flat_map(|p| &p.items).chain(&self.common) {
            if e.id.is_empty()
                || !ids.insert(e.id.clone())
                || !["launch", "folder", "recycle_open", "recycle_empty"].contains(&e.kind.as_str())
            {
                return Err(
                    "Duplicate ID or unsupported entry kind / 項目IDか種類が不正です".into(),
                );
            }
        }
        for common in [false, true] {
            let (rows, cols) = self.dimensions(common);
            let count = if common { 1 } else { self.pages.len() };
            for page in 0..count {
                let mut used = HashSet::new();
                for e in self.group(page, common).iter().filter(|e| !e.recycle()) {
                    if let Some(cell) = e.cell
                        && (cell[0] >= rows
                            || cell[1] >= cols
                            || self.reserved(common, cell)
                            || !used.insert(cell))
                    {
                        return Err("Occupied cell conflicts or falls outside grid. Move buttons before shrinking. / 配置が重複・範囲外です。縮小前に移動してください。".into());
                    }
                }
                let available: Vec<_> = (0..rows)
                    .flat_map(|r| (0..cols).map(move |c| [r, c]))
                    .filter(|c| !used.contains(c) && !self.reserved(common, *c))
                    .collect();
                let mut available = available.into_iter();
                for e in self
                    .group_mut(page, common)
                    .iter_mut()
                    .filter(|e| !e.recycle() && e.cell.is_none())
                {
                    e.cell = Some(
                        available
                            .next()
                            .ok_or("Grid is full / 設定で行・列を増やしてください")?,
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Snapshot {
    Missing,
    Present { length: usize, sha256: [u8; 32] },
}
const EXTERNAL_CHANGE: &str = "Settings were changed outside Orbit. Restart Orbit to load the changes. Your input is preserved. / 設定ファイルが外部で変更されています。Orbitを再起動すると変更を読み込みます。入力内容は残っています。";
impl Snapshot {
    fn bytes(raw: &[u8]) -> Result<Self, String> {
        Ok(Self::Present {
            length: raw.len(),
            sha256: crate::identity::sha256(raw)?,
        })
    }
    fn current(path: &Path) -> Result<Self, String> {
        match fs::read(path) {
            Ok(raw) => Self::bytes(&raw),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::Missing),
            Err(_) => Err("Unable to verify settings. Save cancelled; input is preserved. / 設定を確認できません。保存を中止しました。入力内容は残っています。".into()),
        }
    }
    fn verify(&self, path: &Path) -> Result<(), String> {
        if &Self::current(path)? != self {
            return Err(EXTERNAL_CHANGE.into());
        }
        Ok(())
    }
}
pub fn load_snapshot(path: &Path) -> Result<(Config, Snapshot), String> {
    let (mut config, snapshot) = match fs::read(path) {
        Ok(raw) => (
            serde_json::from_slice(&raw).map_err(
                |_| "Invalid JSON. Original retained / 設定JSONが不正です。原本は変更しません",
            )?,
            Snapshot::bytes(&raw)?,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            (Config::default(), Snapshot::Missing)
        }
        Err(_) => return Err("Unable to read configuration / 設定を読めません".into()),
    };
    config.normalize()?;
    Ok((config, snapshot))
}
#[cfg(test)]
pub fn load(path: &Path) -> Result<Config, String> {
    load_snapshot(path).map(|(c, _)| c)
}
#[cfg(test)]
pub fn save(path: &Path, config: &Config) -> Result<Config, String> {
    let expected = Snapshot::current(path)?;
    save_checked(path, config, &expected).map(|(c, _)| c)
}
pub fn save_checked(
    path: &Path,
    config: &Config,
    expected: &Snapshot,
) -> Result<(Config, Snapshot), String> {
    let mut next = config.clone();
    next.normalize()?;
    expected.verify(path)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|_| "Unable to create settings directory")?;
    let bytes = serde_json::to_vec_pretty(&next).map_err(|_| "JSON serialization failed")?;
    let next_snapshot = Snapshot::bytes(&bytes)?;
    // Stage the exact previous bytes in memory; do not publish a backup before
    // the final conflict check. A detected conflict must leave .bak untouched.
    let previous = if matches!(expected, Snapshot::Present { .. }) {
        let raw = fs::read(path).map_err(|_| "Unable to stage original settings")?;
        if Snapshot::bytes(&raw)? != *expected {
            return Err(EXTERNAL_CHANGE.into());
        }
        let mut prior: Config = serde_json::from_slice(&raw).map_err(
            |_| "Invalid JSON. Original retained / 設定JSONが不正です。原本は変更しません",
        )?;
        prior.normalize()?;
        Some(raw)
    } else {
        None
    };
    let temporary = parent.join(format!(
        ".orbit-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let result: Result<(), String> = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| "Temporary file failed")?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Settings flush failed")?;
        drop(file);
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let a: Vec<_> = temporary
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain([0])
            .collect();
        let b: Vec<_> = path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain([0])
            .collect();
        expected.verify(path)?;
        if let Some(raw) = &previous {
            let backup = path.with_extension("json.bak");
            fs::write(&backup, raw).map_err(|_| "Backup failed; original retained")?;
            if fs::read(&backup).ok().as_ref() != Some(raw) {
                return Err("Backup verification failed; original retained".into());
            }
        }
        // Both nul-terminated buffers live for the call; replacement is atomic on this volume.
        if unsafe {
            MoveFileExW(
                a.as_ptr(),
                b.as_ptr(),
                if *expected == Snapshot::Missing {
                    MOVEFILE_WRITE_THROUGH
                } else {
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH
                },
            )
        } == 0
        {
            use windows_sys::Win32::Foundation::{
                ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, GetLastError,
            };
            let error = unsafe { GetLastError() };
            if *expected == Snapshot::Missing
                && matches!(error, ERROR_ALREADY_EXISTS | ERROR_FILE_EXISTS)
            {
                return Err(EXTERNAL_CHANGE.into());
            }
            return Err("Atomic replacement failed; original retained".into());
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map(|_| (next, next_snapshot))
}

#[cfg(test)]
#[path = "settings_tuning_tests.rs"]
mod tuning_tests;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expansion_preserves_unicode_byte_boundaries() {
        let windir = std::env::var("WINDIR").expect("Windows test environment");
        assert_eq!(expand("İ%wInDiR%"), format!("İ{windir}"));
        assert_eq!(expand("日本語%WINDIR%"), format!("日本語{windir}"));
    }
    #[test]
    fn saves_are_atomic_and_invalid_previous_file_is_never_overwritten() {
        let folder = std::env::temp_dir().join(format!(
            "orbit-rust-test-{}",
            Entry::new("", "", "launch").id
        ));
        fs::create_dir(&folder).unwrap();
        let path = folder.join("config.json");
        let mut c = Config::default();
        c.normalize().unwrap();
        save(&path, &c).unwrap();
        let first = fs::read(&path).unwrap();
        c.settings.language = "en".into();
        save(&path, &c).unwrap();
        assert_eq!(fs::read(path.with_extension("json.bak")).unwrap(), first);
        assert_eq!(load(&path).unwrap().settings.language, "en");
        let before = fs::read(&path).unwrap();
        c.settings.app_rows = 0;
        assert!(save(&path, &c).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::write(&path, b"{broken").unwrap();
        c.settings.app_rows = 2;
        assert!(save(&path, &c).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"{broken");
        // Only this freshly created test directory is removed.
        fs::remove_dir_all(&folder).unwrap();
    }
    #[test]
    fn extra_fields_and_explicit_cells_roundtrip_without_loss() {
        let mut c = Config::default();
        c.normalize().unwrap();
        c.pages[0].items[0]
            .extra
            .insert("future".into(), serde_json::json!({"preserve":true}));
        let v = serde_json::to_value(&c).unwrap();
        let mut restored: Config = serde_json::from_value(v).unwrap();
        restored.normalize().unwrap();
        assert_eq!(
            restored.pages[0].items[0].extra["future"],
            serde_json::json!({"preserve":true})
        );
        restored.pages[0].items[0].cell = Some([0, 4]);
        assert!(restored.normalize().is_err());
    }
    #[test]
    fn holes_and_reserved_cells_survive_dimension_changes() {
        let mut c = Config::default();
        c.normalize().unwrap();
        c.pages[0].items[0].cell = Some([1, 5]);
        c.normalize().unwrap();
        c.settings.app_columns = 7;
        c.normalize().unwrap();
        assert_eq!(c.pages[0].items[0].cell, Some([1, 5]));
        c.settings.app_rows = 1;
        assert!(c.normalize().is_err());
    }
    #[test]
    fn custom_names_are_never_translated() {
        let mut e = Entry::new("自分のメモ", r"C:\Windows\System32\notepad.exe", "launch");
        assert_eq!(e.label(true), "自分のメモ");
        e.name = "メモ帳".into();
        assert_eq!(e.label(true), "Notepad");
        e.target = r"C:\Example\notepad.exe".into();
        assert_eq!(e.label(true), "メモ帳");
    }
}
