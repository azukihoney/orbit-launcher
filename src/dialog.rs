use crate::{
    app::{App, Shared},
    model::{Entry, Page, expand},
    os,
    paint::{self, Font, R, wide},
    runtime::{DialogToken, Runtime, Scope},
};
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ops::Deref,
    ptr::{null, null_mut},
    rc::Rc,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{Com::*, LibraryLoader::*},
    UI::{
        Controls::{Dialogs::*, SetScrollInfo},
        Input::KeyboardAndMouse::*,
        Shell::*,
        WindowsAndMessaging::*,
    },
};
const CLASS: &str = "OrbitRustDialog";
#[derive(Clone)]
pub enum Mode {
    Settings,
    Entry(Option<Entry>),
    Page(bool),
}
enum Kind {
    Text,
    Integer,
    Float,
    Bool,
    Choice(Vec<&'static str>),
}
struct Field {
    key: &'static str,
    label: String,
    control: HWND,
    kind: Kind,
}
struct Dialog {
    shared: *const Shared,
    target: Target,
    mode: Mode,
    font: Font,
    background: HBRUSH,
    dark: bool,
    en: bool,
    scale: f64,
    fields: Vec<Field>,
    positions: Vec<(HWND, R)>,
    scroll: i32,
    wheel: crate::wheel::Wheel,
    content: i32,
    view: i32,
    height: i32,
}
struct DialogHost {
    inner: RefCell<Dialog>,
    runtime: Rc<Runtime>,
    generation: u64,
    window_ref: Cell<bool>,
}
impl Deref for DialogHost {
    type Target = RefCell<Dialog>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
#[derive(Clone)]
struct Target {
    common: bool,
    page_id: Option<String>,
    entry_id: Option<String>,
    cell: Option<[usize; 2]>,
    revision: u64,
}
impl Target {
    fn page(&self, config: &crate::model::Config) -> Result<usize, String> {
        let id = self.page_id.as_ref().ok_or("Missing target page ID")?;
        if id.is_empty() {
            return Err("Missing target page ID".into());
        }
        let indices: Vec<_> = config
            .pages
            .iter()
            .enumerate()
            .filter(|(_, p)| &p.id == id)
            .map(|(i, _)| i)
            .collect();
        if indices.len() != 1 {
            return Err(
                "Target page was removed or duplicated / 対象ページが存在しないか重複しています"
                    .into(),
            );
        }
        Ok(indices[0])
    }
    fn entry_index(&self, entries: &[Entry]) -> Result<usize, String> {
        let indices: Vec<_> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| Some(&e.id) == self.entry_id.as_ref())
            .map(|(i, _)| i)
            .collect();
        if indices.len() != 1 {
            return Err(
                "Target button was removed or duplicated / 対象ボタンが存在しないか重複しています"
                    .into(),
            );
        }
        Ok(indices[0])
    }
}
pub fn register(instance: HINSTANCE) -> Result<(), String> {
    unsafe {
        let class = wide(CLASS);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            ..std::mem::zeroed()
        };
        if RegisterClassW(&wc) == 0 {
            return Err("Dialog class registration failed".into());
        }
    }
    Ok(())
}
impl Dialog {
    fn text<'a>(&self, ja: &'a str, en: &'a str) -> &'a str {
        if self.en { en } else { ja }
    }
    fn child(&self, parent: HWND, class: &str, text: &str, style: u32, r: R, id: usize) -> HWND {
        let r = r.scale(self.scale);
        unsafe {
            let h = CreateWindowExW(
                0,
                wide(class).as_ptr(),
                wide(text).as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                r.x,
                r.y,
                r.w,
                r.h,
                parent,
                id as HMENU,
                GetModuleHandleW(null()),
                null(),
            );
            SendMessageW(h, WM_SETFONT, self.font.0 as usize, 1);
            h
        }
    }
    fn field(
        &mut self,
        parent: HWND,
        key: &'static str,
        label: &str,
        kind: Kind,
        value: &serde_json::Value,
        options: &[&str],
    ) {
        let y = 12 + self.fields.len() as i32 * 32;
        // Keep established control IDs stable when adding the optional appearance row.
        let field_id = if key == "app_icon_only" {
            199
        } else {
            100 + self
                .fields
                .iter()
                .filter(|field| field.key != "app_icon_only")
                .count()
        };
        let label_control = self.child(parent, "STATIC", label, 0, R::new(16, y + 6, 210, 22), 0);
        self.positions
            .push((label_control, R::new(16, y + 6, 210, 22)));
        let r = R::new(234, y, 278, 26);
        let control = match &kind {
            Kind::Bool | Kind::Choice(_) => {
                let h = self.child(
                    parent,
                    "COMBOBOX",
                    "",
                    CBS_DROPDOWNLIST as u32 | WS_TABSTOP | WS_VSCROLL,
                    R::new(r.x, r.y, r.w, 240),
                    field_id,
                );
                for option in options {
                    unsafe {
                        SendMessageW(h, CB_ADDSTRING, 0, wide(option).as_ptr() as isize);
                    }
                }
                let index = match &kind {
                    Kind::Bool => usize::from(value.as_bool().unwrap_or(false)),
                    Kind::Choice(values) => values
                        .iter()
                        .position(|v| Some(*v) == value.as_str())
                        .unwrap_or(0),
                    _ => 0,
                };
                unsafe {
                    SendMessageW(h, CB_SETCURSEL, index, 0);
                }
                h
            }
            _ => {
                let text = value
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| value.to_string());
                self.child(
                    parent,
                    "EDIT",
                    &text,
                    WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
                    r,
                    field_id,
                )
            }
        };
        self.positions.push((control, r));
        self.fields.push(Field {
            key,
            label: label.into(),
            control,
            kind,
        });
    }
    fn arrange(&mut self, hwnd: HWND) {
        unsafe {
            let max = (self.content - self.view).max(0);
            self.scroll = self.scroll.clamp(0, max);
            for (control, base) in &self.positions {
                let mut r = *base;
                r.y -= self.scroll;
                let visible = r.y >= 8 && r.y + r.h <= self.view;
                let r = r.scale(self.scale);
                let mut name = [0u16; 32];
                let n = GetClassNameW(*control, name.as_mut_ptr(), 32);
                let combo =
                    String::from_utf16_lossy(&name[..n as usize]).eq_ignore_ascii_case("ComboBox");
                SetWindowPos(
                    *control,
                    null_mut(),
                    r.x,
                    r.y,
                    r.w,
                    if combo {
                        (240. * self.scale) as i32
                    } else {
                        r.h
                    },
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                ShowWindow(*control, if visible { SW_SHOW } else { SW_HIDE });
            }
            let info = SCROLLINFO {
                cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                fMask: SIF_ALL,
                nMin: 0,
                nMax: self.content,
                nPage: self.view as u32,
                nPos: self.scroll,
                nTrackPos: 0,
            };
            SetScrollInfo(hwnd, SB_VERT, &info, 1);
            InvalidateRect(hwnd, null(), 1);
        }
    }
    fn value(&self, field: &Field) -> Result<serde_json::Value, String> {
        let text = control_text(field.control);
        match &field.kind {
            Kind::Text => Ok(text.into()),
            Kind::Integer => text
                .trim()
                .parse::<i64>()
                .map(serde_json::Value::from)
                .map_err(|_| format!("{}: Enter an integer / 整数を入力してください", field.label)),
            Kind::Float => text
                .trim()
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(serde_json::Value::Number)
                .ok_or_else(|| format!("{}: Enter a number / 数値を入力してください", field.label)),
            Kind::Bool => {
                Ok((unsafe { SendMessageW(field.control, CB_GETCURSEL, 0, 0) } == 1).into())
            }
            Kind::Choice(choices) => {
                let index = unsafe { SendMessageW(field.control, CB_GETCURSEL, 0, 0) } as usize;
                choices
                    .get(index)
                    .map(|v| serde_json::Value::from(*v))
                    .ok_or_else(|| "Choose an option / 選択してください".into())
            }
        }
    }
    fn accept(&self) -> Result<bool, String> {
        let shared = unsafe { &*self.shared };
        let mut app = shared
            .app
            .try_borrow_mut()
            .map_err(|_| "Another operation is running")?;
        let mut config = app.config.clone();
        if app.revision != self.target.revision {
            return Err("Settings changed while editing. Input retained / 編集中に設定が変わりました。入力は保持しています".into());
        }
        let values: std::collections::BTreeMap<_, _> = self
            .fields
            .iter()
            .map(|f| self.value(f).map(|v| (f.key, v)))
            .collect::<Result<_, _>>()?;
        match &self.mode {
            Mode::Settings => {
                let face = values["font_family"].as_str().ok_or("Invalid font name")?;
                if face != config.settings.font_family
                    && (face.encode_utf16().count() > 31 || face.contains('\0'))
                {
                    return Err("Font names must fit within 31 UTF-16 units / フォント名はUTF-16で31要素以内にしてください".into());
                }
                let mut settings = serde_json::to_value(&config.settings)
                    .map_err(|_| "Settings serialization failed")?;
                for (key, value) in values {
                    settings[key] = value;
                }
                config.settings = serde_json::from_value(settings)
                    .map_err(|_| "Invalid settings / 設定値が不正です")?;
            }
            Mode::Entry(original) => {
                let mut entry = original
                    .clone()
                    .unwrap_or_else(|| Entry::new("", "", "launch"));
                entry.name = values["name"].as_str().unwrap().trim().into();
                entry.target = values["target"].as_str().unwrap().trim().into();
                entry.args = values["args"].as_str().unwrap().into();
                entry.cwd = values["cwd"].as_str().unwrap().trim().into();
                entry.kind = values["kind"].as_str().unwrap().into();
                if entry.name.is_empty() || entry.target.is_empty() {
                    return Err(
                        "Name and target are required / 表示名と対象を入力してください".into(),
                    );
                }
                let page = if self.target.common {
                    0
                } else {
                    self.target.page(&config)?
                };
                let entries = config.group_mut(page, self.target.common);
                if original.is_some() {
                    let index = self.target.entry_index(entries)?;
                    entries[index] = entry;
                } else {
                    entry.cell = self.target.cell;
                    entries.push(entry);
                }
            }
            Mode::Page(rename) => {
                let name = values["name"].as_str().unwrap().trim();
                if name.is_empty() {
                    return Err("Enter a page name / ページ名を入力してください".into());
                }
                if *rename {
                    let page = self.target.page(&config)?;
                    config.pages[page].name = name.into();
                } else {
                    config.pages.push(Page {
                        id: Entry::new("", "", "launch").id,
                        name: name.into(),
                        items: vec![],
                        extra: Default::default(),
                    });
                }
            }
        }
        config.normalize()?;
        let success = app.commit(config);
        if success && matches!(self.mode, Mode::Page(false)) {
            app.page = app.config.pages.len() - 1;
            app.relayout();
        }
        Ok(success)
    }
    fn browse(&self, hwnd: HWND, folder: bool) {
        let Some(field) = self.fields.iter().find(|f| f.key == "target") else {
            return;
        };
        // Read the edit control now, not the saved Entry captured when it opened.
        let target =
            crate::path_decisions::browse_field_value(&control_text(field.control), expand);
        let start = if crate::path_decisions::drive_absolute(&target) {
            let path = std::path::Path::new(&target);
            let existing = if path.is_file() {
                crate::path_decisions::Existing::File
            } else if path.is_dir() {
                crate::path_decisions::Existing::Folder
            } else {
                crate::path_decisions::Existing::Missing
            };
            crate::path_decisions::browse_start(
                &target,
                existing,
                path.parent().is_some_and(std::path::Path::is_dir),
            )
        } else {
            None
        };
        unsafe {
            if let Some(result) = choose_path(hwnd, folder, start.as_ref(), self.en) {
                SetWindowTextW(field.control, wide(&result).as_ptr());
            }
            if let Some(name) = self.fields.iter().find(|f| f.key == "name")
                && control_text(name.control).is_empty()
            {
                let text = control_text(field.control);
                let name_text = std::path::Path::new(&text)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy();
                SetWindowTextW(name.control, wide(&name_text).as_ptr());
            }
        }
    }
}
// IFileDialog vtable slots, counted from IUnknown (0..=2).
const CLSID_FILE_OPEN_DIALOG: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0xdc1c5a9c,
    data2: 0xe88a,
    data3: 0x4dde,
    data4: [0xa5, 0xa1, 0x60, 0xf8, 0x2a, 0x20, 0xae, 0xf7],
};
const IID_FILE_OPEN_DIALOG: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0xd57c7288,
    data2: 0xd4ad,
    data3: 0x4768,
    data4: [0xbe, 0x02, 0x9d, 0x96, 0x95, 0x32, 0xd9, 0x60],
};
const IID_SHELL_ITEM: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0x43826d1e,
    data2: 0xe718,
    data3: 0x42ee,
    data4: [0xbc, 0x55, 0xa1, 0xe2, 0x61, 0xc3, 0x7b, 0xfe],
};
const FOS_PICKFOLDERS: u32 = 0x20;
const FOS_FORCEFILESYSTEM: u32 = 0x40;
const FOS_NOCHANGEDIR: u32 = 0x8;
const FOS_PATHMUSTEXIST: u32 = 0x800;
const FOS_FILEMUSTEXIST: u32 = 0x1000;
const FOS_NODEREFERENCELINKS: u32 = 0x100000;
const SIGDN_FILESYSPATH: u32 = 0x80058000;

struct PickerPtr(*mut c_void);
impl PickerPtr {
    unsafe fn method(&self, index: usize) -> usize {
        unsafe { *(*(self.0 as *mut *mut usize)).add(index) }
    }
}
impl Drop for PickerPtr {
    fn drop(&mut self) {
        unsafe {
            let release: unsafe extern "system" fn(*mut c_void) -> u32 =
                std::mem::transmute(self.method(2));
            release(self.0);
        }
    }
}

unsafe fn choose_path(
    hwnd: HWND,
    folder: bool,
    start: Option<&crate::path_decisions::BrowseStart>,
    en: bool,
) -> Option<String> {
    unsafe {
        let mut raw = null_mut();
        if CoCreateInstance(
            &CLSID_FILE_OPEN_DIALOG,
            null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_FILE_OPEN_DIALOG,
            &mut raw,
        ) < 0
            || raw.is_null()
        {
            return None;
        }
        let dialog = PickerPtr(raw);
        let get_options: unsafe extern "system" fn(*mut c_void, *mut u32) -> i32 =
            std::mem::transmute(dialog.method(10));
        let set_options: unsafe extern "system" fn(*mut c_void, u32) -> i32 =
            std::mem::transmute(dialog.method(9));
        let mut options = 0;
        if get_options(dialog.0, &mut options) < 0 {
            return None;
        }
        options |= FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR;
        if folder {
            options |= FOS_PICKFOLDERS;
        } else {
            options |= FOS_FILEMUSTEXIST | FOS_NODEREFERENCELINKS;
        }
        if set_options(dialog.0, options) < 0 {
            return None;
        }
        if folder {
            let set_title: unsafe extern "system" fn(*mut c_void, *const u16) -> i32 =
                std::mem::transmute(dialog.method(17));
            set_title(
                dialog.0,
                wide(if en {
                    "Choose folder"
                } else {
                    "フォルダを選択"
                })
                .as_ptr(),
            );
        }
        let mut folder_set = false;
        let mut file_name_set = false;
        #[cfg(feature = "review-test")]
        let force_item_failure = std::env::var_os("ORBIT_REVIEW_PICKER_ITEM_FAILURE").is_some();
        #[cfg(not(feature = "review-test"))]
        let force_item_failure = false;
        if let Some(start) = start {
            let mut item_raw = null_mut();
            if !force_item_failure
                && SHCreateItemFromParsingName(
                    wide(&start.folder).as_ptr(),
                    null_mut(),
                    &IID_SHELL_ITEM,
                    &mut item_raw,
                ) >= 0
                && !item_raw.is_null()
            {
                let item = PickerPtr(item_raw);
                let set_folder: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32 =
                    std::mem::transmute(dialog.method(12));
                folder_set = set_folder(dialog.0, item.0) >= 0;
            }
            if !folder && let Some(name) = &start.file_name {
                let set_name: unsafe extern "system" fn(*mut c_void, *const u16) -> i32 =
                    std::mem::transmute(dialog.method(15));
                file_name_set = set_name(dialog.0, wide(name).as_ptr()) >= 0;
            }
        }
        #[cfg(feature = "review-test")]
        if let Some(output) = std::env::var_os("ORBIT_REVIEW_PICKER_OUTPUT") {
            let record = serde_json::json!({"folder": folder, "start": start.map(|s| &s.folder),
                "folder_set": folder_set, "file_name": if file_name_set { start.and_then(|s| s.file_name.as_deref()) } else { None }, "options": options});
            let _ = std::fs::write(output, record.to_string());
        }
        #[cfg(not(feature = "review-test"))]
        let _ = (folder_set, file_name_set);
        let show: unsafe extern "system" fn(*mut c_void, HWND) -> i32 =
            std::mem::transmute(dialog.method(3));
        if show(dialog.0, hwnd) < 0 {
            return None;
        }
        #[cfg(feature = "review-test")]
        if std::env::var_os("ORBIT_REVIEW_PICKER_RESULT_FAILURE").is_some() {
            return None;
        }
        let get_result: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32 =
            std::mem::transmute(dialog.method(20));
        let mut item_raw = null_mut();
        if get_result(dialog.0, &mut item_raw) < 0 || item_raw.is_null() {
            return None;
        }
        let item = PickerPtr(item_raw);
        let get_name: unsafe extern "system" fn(*mut c_void, u32, *mut *mut u16) -> i32 =
            std::mem::transmute(item.method(5));
        let mut name = null_mut();
        if get_name(item.0, SIGDN_FILESYSPATH, &mut name) < 0 || name.is_null() {
            return None;
        }
        let len = (0..32768).position(|i| *name.add(i) == 0).unwrap_or(32768);
        let result = String::from_utf16_lossy(std::slice::from_raw_parts(name, len));
        CoTaskMemFree(name.cast());
        Some(result)
    }
}
impl Drop for Dialog {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.background);
        }
    }
}
pub fn control_text(hwnd: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(hwnd);
        let mut v = vec![0u16; n as usize + 1];
        GetWindowTextW(hwnd, v.as_mut_ptr(), n + 1);
        String::from_utf16_lossy(&v[..n as usize])
    }
}
pub fn open(app: &mut App, mode: Mode) {
    app.hide_tooltip();
    unsafe {
        if !app.dialog.is_null() {
            os::raise_window(app.dialog);
            return;
        }
        app.reset_wheels();
        app.clear_icon_only_hover();
        let s = &app.config.settings;
        let (common, _, cell) = app.context.unwrap_or((false, None, None));
        let target = Target {
            common,
            page_id: app.config.pages.get(app.page).map(|p| p.id.clone()),
            entry_id: match &mode {
                Mode::Entry(Some(e)) => Some(e.id.clone()),
                _ => None,
            },
            cell,
            revision: app.revision,
        };
        let mut d = Dialog {
            shared: app.shared,
            target,
            mode: mode.clone(),
            font: Font::new(&s.font_family, (12. * app.scale) as i32, false),
            background: CreateSolidBrush(paint::colorref(paint::palette(s.theme == "dark").bottom)),
            dark: s.theme == "dark",
            en: app.en(),
            scale: app.scale,
            fields: vec![],
            positions: vec![],
            scroll: 0,
            wheel: crate::wheel::Wheel::default(),
            content: 0,
            view: 0,
            height: 0,
        };
        let count = match mode {
            Mode::Settings => 21,
            Mode::Entry(_) => 7,
            Mode::Page(_) => 1,
        };
        let work = os::work_area(app.hwnd);
        let height = (count * 32 + 80)
            .min(((work.bottom - work.top) as f64 / app.scale) as i32 - 110)
            .max(150);
        d.height = height;
        d.view = height - 48;
        let title = match mode {
            Mode::Settings => app.tr("ジェスチャー・表示設定", "Gesture and appearance settings"),
            Mode::Entry(_) => app.tr("ボタンを追加・編集", "Add / edit button"),
            Mode::Page(_) => app.tr("ページ名", "Page name"),
        };
        app.dialog_generation += 1;
        let state = Rc::new(DialogHost {
            inner: RefCell::new(d),
            runtime: app.runtime.clone(),
            generation: app.dialog_generation,
            window_ref: Cell::new(true),
        });
        // Keep a creation reference too: CreateWindowEx may send NCDESTROY
        // before returning failure, in which case the window reference is gone.
        let raw = Rc::into_raw(state.clone());
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN | WS_VSCROLL;
        let mut rect = R::new(0, 0, 538, height).scale(app.scale).rect();
        AdjustWindowRectEx(&mut rect, style, 0, WS_EX_DLGMODALFRAME);
        let w = rect.right - rect.left;
        let h = rect.bottom - rect.top;
        let hwnd = CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            wide(CLASS).as_ptr(),
            wide(title).as_ptr(),
            style,
            work.left + (work.right - work.left - w) / 2,
            work.top + (work.bottom - work.top - h) / 2,
            w,
            h,
            app.hwnd,
            null_mut(),
            GetModuleHandleW(null()),
            raw.cast(),
        );
        if hwnd.is_null() {
            if state.window_ref.replace(false) {
                drop(Rc::from_raw(raw));
            }
            app.message("Unable to create settings window", true);
            return;
        }
        app.dialog = hwnd;
        app.runtime.outside_cancel.set(true);
        app.runtime.dialog.set(Some(DialogToken {
            hwnd,
            generation: app.dialog_generation,
        }));
        EnableWindow(app.hwnd, 0);
        {
            let mut d = (*raw).borrow_mut();
            match mode {
                Mode::Settings => {
                    let settings = serde_json::to_value(s).unwrap();
                    let yes = [
                        d.text("いいえ", "No").to_string(),
                        d.text("はい", "Yes").to_string(),
                    ];
                    let yes = [yes[0].as_str(), yes[1].as_str()];
                    let specs: Vec<(&str, &str, &str, Kind, Vec<&str>)> = vec![
                        (
                            "language",
                            "表示言語 / Language",
                            "Language / 表示言語",
                            Kind::Choice(vec!["ja", "en"]),
                            vec!["日本語", "English"],
                        ),
                        ("font_family", "フォント", "Font", Kind::Text, vec![]),
                        (
                            "app_icon_only",
                            "アプリをアイコンだけで表示",
                            "App icons only",
                            Kind::Bool,
                            yes.to_vec(),
                        ),
                        (
                            "app_font_px",
                            "アプリ文字サイズ（px）",
                            "App font size (px)",
                            Kind::Integer,
                            vec![],
                        ),
                        (
                            "folder_font_px",
                            "フォルダ文字サイズ（px）",
                            "Folder font size (px)",
                            Kind::Integer,
                            vec![],
                        ),
                        (
                            "theme",
                            "テーマ",
                            "Theme",
                            Kind::Choice(vec!["light", "dark"]),
                            vec![app.tr("ライト", "Light"), app.tr("ダーク", "Dark")],
                        ),
                        (
                            "app_rows",
                            "アプリの行数",
                            "App rows",
                            Kind::Integer,
                            vec![],
                        ),
                        (
                            "app_columns",
                            "アプリの列数",
                            "App columns",
                            Kind::Integer,
                            vec![],
                        ),
                        (
                            "folder_rows",
                            "フォルダの行数",
                            "Folder rows",
                            Kind::Integer,
                            vec![],
                        ),
                        (
                            "folder_columns",
                            "フォルダの列数",
                            "Folder columns",
                            Kind::Integer,
                            vec![],
                        ),
                        (
                            "turns",
                            "円を描く回数",
                            "Number of turns",
                            Kind::Float,
                            vec![],
                        ),
                        (
                            "timeout",
                            "制限時間（秒）",
                            "Time limit (seconds)",
                            Kind::Float,
                            vec![],
                        ),
                        (
                            "min_radius",
                            "最小半径（画面px）",
                            "Minimum radius (screen px)",
                            Kind::Float,
                            vec![],
                        ),
                        (
                            "max_radius",
                            "最大半径（画面px）",
                            "Maximum radius (screen px)",
                            Kind::Float,
                            vec![],
                        ),
                        (
                            "direction",
                            "回転方向",
                            "Direction",
                            Kind::Choice(vec!["ccw", "cw", "both"]),
                            vec![
                                app.tr("反時計回りだけ", "Counterclockwise only"),
                                app.tr("時計回りだけ", "Clockwise only"),
                                app.tr("両方向", "Both"),
                            ],
                        ),
                        (
                            "gesture_enabled",
                            "円描画を有効にする",
                            "Enable circle gesture",
                            Kind::Bool,
                            yes.to_vec(),
                        ),
                        (
                            "auto_hide",
                            "マウスが外へ出たら隠す",
                            "Hide when pointer leaves",
                            Kind::Bool,
                            yes.to_vec(),
                        ),
                        (
                            "skip_fullscreen",
                            "全画面アプリ中は停止",
                            "Pause over fullscreen apps",
                            Kind::Bool,
                            yes.to_vec(),
                        ),
                    ];
                    for (key, ja, en, kind, options) in specs {
                        let label = if d.en { en } else { ja };
                        d.field(hwnd, key, label, kind, &settings[key], &options);
                    }
                    let y = 12 + d.fields.len() as i32 * 32;
                    let label=d.child(hwnd,"STATIC",d.text("文字サイズ 8～16px、アプリ列数 2～32。\nフォント欄には導入済みフォント名を入力できます。","Font sizes 8–16 px; app columns 2–32.\nEnter an installed font name, or choose a font below."),0,R::new(16,y,496,42),0);
                    d.positions.push((label, R::new(16, y, 496, 42)));
                    let b = d.child(
                        hwnd,
                        "BUTTON",
                        d.text("フォントを選択…", "Choose font…"),
                        WS_TABSTOP,
                        R::new(234, y + 46, 180, 27),
                        310,
                    );
                    d.positions.push((b, R::new(234, y + 46, 180, 27)));
                    d.content = y + 80;
                }
                Mode::Entry(original) => {
                    let e = original.unwrap_or_else(|| Entry::new("", "", "launch"));
                    for (key, ja, en, value) in [
                        ("name", "表示名", "Name", e.name),
                        ("target", "対象", "Target", e.target),
                        ("args", "引数", "Arguments", e.args),
                        ("cwd", "作業フォルダ", "Working folder", e.cwd),
                    ] {
                        let label = if d.en { en } else { ja };
                        d.field(hwnd, key, label, Kind::Text, &value.into(), &[]);
                    }
                    let choices = [
                        app.tr("アプリ・bat・cmd・ファイル", "App / bat / cmd / file"),
                        app.tr("フォルダ", "Folder"),
                    ];
                    d.field(
                        hwnd,
                        "kind",
                        app.tr("種類", "Type"),
                        Kind::Choice(vec!["launch", "folder"]),
                        &e.kind.into(),
                        &choices,
                    );
                    let y = 12 + d.fields.len() as i32 * 32;
                    let file = d.child(
                        hwnd,
                        "BUTTON",
                        d.text("ファイルを参照…", "Browse file…"),
                        WS_TABSTOP,
                        R::new(234, y, 132, 28),
                        301,
                    );
                    let folder = d.child(
                        hwnd,
                        "BUTTON",
                        d.text("フォルダを参照…", "Browse folder…"),
                        WS_TABSTOP,
                        R::new(374, y, 138, 28),
                        302,
                    );
                    d.positions.push((file, R::new(234, y, 132, 28)));
                    d.positions.push((folder, R::new(374, y, 138, 28)));
                    d.content = y + 36;
                }
                Mode::Page(rename) => {
                    let name = if rename {
                        app.config.pages[app.page].name.clone()
                    } else {
                        String::new()
                    };
                    d.field(
                        hwnd,
                        "name",
                        app.tr("ページ名", "Page name"),
                        Kind::Text,
                        &name.into(),
                        &[],
                    );
                    d.content = 60;
                }
            }
            d.child(
                hwnd,
                "BUTTON",
                "OK",
                WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
                R::new(350, height - 36, 76, 27),
                1,
            );
            d.child(
                hwnd,
                "BUTTON",
                d.text("キャンセル", "Cancel"),
                WS_TABSTOP,
                R::new(436, height - 36, 76, 27),
                2,
            );
            d.arrange(hwnd);
        }
        ShowWindow(hwnd, SW_SHOW);
        os::raise_window(hwnd);
        SetFocus((*raw).borrow().fields[0].control);
    }
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            let create = &*(l as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            return 1;
        }
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const DialogHost;
        if ptr.is_null() {
            return DefWindowProcW(hwnd, msg, w, l);
        }
        // Keep state alive across reentrant NCDESTROY even if a native chooser
        // still holds a RefMut. Window ownership and Rust ownership are separate.
        Rc::increment_strong_count(ptr);
        let host = Rc::from_raw(ptr);
        let token = DialogToken {
            hwnd,
            generation: host.generation,
        };
        if msg == WM_NCDESTROY {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            host.runtime.destroyed_dialog.set(Some(token));
            if host.window_ref.replace(false) {
                drop(Rc::from_raw(ptr));
            }
            return DefWindowProcW(hwnd, msg, w, l);
        }
        if msg == WM_CLOSE || (msg == WM_COMMAND && w & 0xffff == 2) {
            host.runtime.close_dialog.set(Some(token));
            return 0;
        }
        let Ok(mut d) = host.try_borrow_mut() else {
            if msg == WM_COMMAND {
                return 0;
            }
            return DefWindowProcW(hwnd, msg, w, l);
        };
        match msg {
            WM_COMMAND => {
                if host.runtime.nested() {
                    return 0;
                }
                let _guard = host.runtime.enter(Scope::Modal);
                match w & 0xffff {
                    1 => {
                        #[cfg(feature = "review-test")]
                        crate::icon_only_probe::event(
                            "dialog_accept_started",
                            serde_json::Value::Null,
                        );
                        match d.accept() {
                            Ok(true) => {
                                host.runtime.close_dialog.set(Some(token));
                            }
                            Ok(false) => {}
                            Err(e) => os::message(hwnd, &e, true),
                        }
                    }
                    301 => d.browse(hwnd, false),
                    302 => d.browse(hwnd, true),
                    310 => {
                        let mut log: LOGFONTW = std::mem::zeroed();
                        if let Some(f) = d.fields.iter().find(|f| f.key == "font_family") {
                            log.lfFaceName = paint::font_face(&control_text(f.control));
                            log.lfHeight = -12;
                            #[cfg(feature = "review-test")]
                            crate::stage2_probe::font("choose_font", &log.lfFaceName);
                            let mut choose: CHOOSEFONTW = std::mem::zeroed();
                            choose.lStructSize = std::mem::size_of::<CHOOSEFONTW>() as u32;
                            choose.hwndOwner = hwnd;
                            choose.lpLogFont = &mut log;
                            choose.Flags = CF_SCREENFONTS | CF_INITTOLOGFONTSTRUCT;
                            if ChooseFontW(&mut choose) != 0 {
                                SetWindowTextW(f.control, log.lfFaceName.as_ptr());
                            }
                        }
                    }
                    _ => {}
                }
                0
            }
            WM_VSCROLL => {
                d.wheel.reset();
                let mut info: SCROLLINFO = std::mem::zeroed();
                info.cbSize = std::mem::size_of::<SCROLLINFO>() as u32;
                info.fMask = SIF_TRACKPOS;
                GetScrollInfo(hwnd, SB_VERT, &mut info);
                match (w & 0xffff) as i32 {
                    SB_LINEUP => d.scroll -= 32,
                    SB_LINEDOWN => d.scroll += 32,
                    SB_PAGEUP => d.scroll -= d.view,
                    SB_PAGEDOWN => d.scroll += d.view,
                    SB_THUMBTRACK => d.scroll = info.nTrackPos,
                    _ => {}
                }
                d.arrange(hwnd);
                0
            }
            WM_MOUSEWHEEL => {
                d.scroll -= d.wheel.feed(((w >> 16) as i16) as i32) * 64;
                d.arrange(hwnd);
                #[cfg(feature = "review-test")]
                crate::stage3_probe::event("dialog_wheel", serde_json::json!(d.scroll));
                0
            }
            WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
                let c = paint::palette(d.dark);
                SetTextColor(w as HDC, paint::colorref(c.text));
                SetBkColor(w as HDC, paint::colorref(c.bottom));
                d.background as isize
            }
            WM_ERASEBKGND => {
                let mut r: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut r);
                FillRect(w as HDC, &r, d.background);
                1
            }
            _ => DefWindowProcW(hwnd, msg, w, l),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editing_resolves_ids_after_reordering_and_refuses_missing_or_duplicate_targets() {
        let mut config = crate::model::Config::default();
        let target = Target {
            common: false,
            page_id: Some(config.pages[0].id.clone()),
            entry_id: Some(config.pages[0].items[0].id.clone()),
            cell: Some([0, 0]),
            revision: 0,
        };
        config.pages[0].items.swap(0, 1);
        assert_eq!(target.entry_index(&config.pages[0].items).unwrap(), 1);
        config.pages.push(Page {
            id: "other-page".into(),
            name: "Other".into(),
            items: vec![],
            extra: Default::default(),
        });
        config.pages.swap(0, 1);
        assert_eq!(target.page(&config).unwrap(), 1);
        let duplicate = config.pages[1].items[1].clone();
        config.pages[1].items.push(duplicate);
        assert!(target.entry_index(&config.pages[1].items).is_err());
        config.pages[1]
            .items
            .retain(|e| Some(&e.id) != target.entry_id.as_ref());
        assert!(target.entry_index(&config.pages[1].items).is_err());
        let duplicate = config.pages[1].clone();
        config.pages.push(duplicate);
        assert!(target.page(&config).is_err());
        config
            .pages
            .retain(|p| Some(&p.id) != target.page_id.as_ref());
        assert!(target.page(&config).is_err());
    }
}
