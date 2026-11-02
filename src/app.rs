use crate::{
    dialog,
    gesture::Detector,
    model::{self, Config, Entry},
    os,
    paint::{self, Fonts, Icon, R, wide},
    runtime::{Dpi, Runtime, Scope},
};
use std::{
    cell::RefCell,
    path::PathBuf,
    ptr::{null, null_mut},
    rc::Rc,
    time::Instant,
};
use windows_sys::Win32::UI::Controls::{
    ICC_WIN95_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, TTF_ABSOLUTE, TTF_TRACK,
    TTM_ADDTOOLW, TTM_SETMAXTIPWIDTH, TTM_TRACKACTIVATE, TTM_TRACKPOSITION, TTM_UPDATETIPTEXTW,
    TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW as TOOLINFOW, WM_MOUSELEAVE,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{Com::*, LibraryLoader::*},
    UI::{HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};

pub const RECYCLE_DONE: u32 = WM_APP + 2;
const TRAY: u32 = WM_APP + 3;
pub const REVEAL: u32 = WM_APP + 4;
pub const SETTINGS: usize = 100;
pub const ADD_APP: usize = 101;
pub const ADD_FOLDER: usize = 102;
pub const ADD_PAGE: usize = 103;
pub const RENAME_PAGE: usize = 104;
pub const REMOVE_PAGE: usize = 105;
pub const QUIT: usize = 107;
pub const GESTURE: usize = 108;
pub const OPEN_FILE_LOCATION: usize = 204;
const COLOR_NONE: usize = 210;
const COLOR_LAST: usize = COLOR_NONE + crate::folder_color::COLORS.len();
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Entry(bool, usize),
    Cell(bool, [usize; 2]),
    Tool(usize),
    Page(usize),
    Nothing,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub width: i32,
    pub height: i32,
    pub apps: R,
    pub folders: R,
    pub divider: i32,
    pub controls: R,
    pub rail: i32,
}
pub struct App {
    pub hwnd: HWND,
    pub shared: *const Shared,
    pub runtime: Rc<Runtime>,
    pub revision: u64,
    pub dialog_generation: u64,
    recycle_generation: u64,
    pub config: Config,
    pub snapshot: model::Snapshot,
    pub path: PathBuf,
    pub page: usize,
    pub dialog: HWND,
    pub pinned: bool,
    pub scale: f64,
    pub fonts: Fonts,
    pub layout: Layout,
    pub context: Option<(bool, Option<usize>, Option<[usize; 2]>)>,
    icons: crate::icon_store::Store,
    tray_icon: Icon,
    tray_added: bool,
    tray_retry: Option<(Instant, u8)>,
    tray_warning: bool,
    reveal_generation: u64,
    wheel_app: crate::wheel::Wheel,
    wheel_folder: crate::wheel::Wheel,
    hover: Hit,
    pressed: Hit,
    down: (i32, i32),
    dragging: bool,
    pub scroll_app: i32,
    pub scroll_folder: i32,
    detector: Detector,
    start: Instant,
    last_pos: Option<(i32, i32)>,
    last_move: f64,
    last_inside: f64,
    last_fullscreen: f64,
    in_fullscreen: bool,
    poll_ms: u32,
    outside_click: crate::outside_click::State,
    pub preview: Option<PathBuf>,
    captured: bool,
    pub exiting: bool,
    runtime_gesture: bool,
    capture: Option<crate::diagnostics::Capture>,
    tooltip: HWND,
    tooltip_text: Vec<u16>,
    tooltip_active: bool,
    hover_at: f64,
}
pub struct Shared {
    pub app: RefCell<App>,
    pub runtime: Rc<Runtime>,
}
impl App {
    fn interaction_blocked(&self) -> bool {
        self.runtime.editing_blocked() || !os::owner_available(self.hwnd, self.tooltip)
    }
    pub(crate) fn message(&self, text: &str, error: bool) {
        let _guard = self.runtime.enter(Scope::Modal);
        os::message(self.dialog_or_main(), text, error);
    }
    fn confirm(&self, text: &str) -> bool {
        let _guard = self.runtime.enter(Scope::Modal);
        os::confirm(self.dialog_or_main(), text)
    }
    fn drain_notifications(&mut self) {
        self.restore_tray_if_due();
        if self.runtime.nested() {
            return;
        }
        if let Some(token) = self.runtime.close_dialog.take()
            && self.runtime.dialog.get() == Some(token)
        {
            unsafe {
                // Enable before destroy, after all inner modal calls have returned.
                EnableWindow(self.hwnd, 1);
                if IsWindow(token.hwnd) != 0 {
                    DestroyWindow(token.hwnd);
                }
            }
            self.runtime.destroyed_dialog.set(Some(token));
        }
        if let Some(token) = self.runtime.destroyed_dialog.take()
            && self.runtime.dialog.get() == Some(token)
        {
            self.runtime.dialog.set(None);
            self.dialog = null_mut();
            self.reset_wheels();
            unsafe {
                EnableWindow(self.hwnd, 1);
            }
            self.touch();
            self.invalidate();
            os::raise_window(self.hwnd);
        }
        if let Some(dpi) = self.runtime.dpi.take()
            && dpi
                .reveal
                .map(|p| p.generation == self.reveal_generation)
                .unwrap_or(true)
        {
            // A delivered DPI change ends the previous nudge attempt, including
            // changes applied after a move or during an explicit reveal.
            self.runtime.dpi_attempted.set(None);
            let moving_anchor = if dpi.moving {
                let mut rect = RECT::default();
                if unsafe { GetWindowRect(self.hwnd, &mut rect) } != 0 {
                    Some((rect.left, rect.top))
                } else {
                    None
                }
            } else {
                None
            };
            self.scale = dpi.value as f64 / 96.;
            let work = os::work_area(self.hwnd);
            let layout = crate::geometry::calculate(
                &self.config.settings,
                self.config.pages.len(),
                work,
                dpi.value,
            );
            if let Some(plan) = dpi.reveal {
                let rect = crate::geometry::place(
                    &layout,
                    (plan.cursor.x, plan.cursor.y),
                    work,
                    dpi.value,
                );
                self.install_layout(layout, false);
                unsafe {
                    SetWindowPos(
                        self.hwnd,
                        null_mut(),
                        rect.x,
                        rect.y,
                        rect.w,
                        rect.h,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            } else {
                self.install_layout(layout, true);
                let (x, y) = if let Some((left, top)) = moving_anchor {
                    let work = os::work_area(self.hwnd);
                    let mut resized = RECT::default();
                    if unsafe { GetWindowRect(self.hwnd, &mut resized) } != 0 {
                        let width = resized.right - resized.left;
                        let height = resized.bottom - resized.top;
                        (
                            left.min(work.right - width).max(work.left),
                            top.min(work.bottom - height).max(work.top),
                        )
                    } else {
                        (left, top)
                    }
                } else {
                    (dpi.rect.left, dpi.rect.top)
                };
                unsafe {
                    SetWindowPos(
                        self.hwnd,
                        null_mut(),
                        x,
                        y,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            }
        }
        if let Some((generation, result)) = self.runtime.recycle_done.take()
            && self.runtime.recycle.get() == Some(generation)
        {
            self.runtime.recycle.set(None);
            self.touch();
            if result < 0 && result as u32 != 0x800704c7 && result as u32 != 0x80004004 {
                self.message(&format!("Recycle Bin error 0x{:08X}", result as u32), true);
            }
        }
        if self.runtime.close_main.replace(false) {
            self.hide();
        }
    }
    pub fn en(&self) -> bool {
        self.config.en()
    }
    pub fn tr<'a>(&self, ja: &'a str, en: &'a str) -> &'a str {
        if self.en() { en } else { ja }
    }
    pub fn invalidate(&self) {
        unsafe {
            InvalidateRect(self.hwnd, null(), 0);
        }
    }
    pub fn touch(&mut self) {
        self.last_inside = self.start.elapsed().as_secs_f64();
    }
    pub fn hide_tooltip(&mut self) {
        if self.tooltip_active {
            unsafe {
                let info = self.tip_info();
                SendMessageW(
                    self.tooltip,
                    TTM_TRACKACTIVATE,
                    0,
                    &info as *const _ as isize,
                );
            }
            self.tooltip_active = false;
        }
    }
    fn tip_info(&self) -> TOOLINFOW {
        let mut info: TOOLINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<TOOLINFOW>() as u32;
        info.uFlags = TTF_TRACK | TTF_ABSOLUTE;
        info.hwnd = self.hwnd;
        info.uId = 1;
        info.lpszText = self.tooltip_text.as_ptr().cast_mut();
        info
    }
    fn init_tooltip(&mut self) {
        unsafe {
            self.tooltip = CreateWindowExW(
                WS_EX_TOPMOST,
                wide("tooltips_class32").as_ptr(),
                null(),
                WS_POPUP | TTS_NOPREFIX | TTS_ALWAYSTIP,
                0,
                0,
                0,
                0,
                self.hwnd,
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            );
            let info = self.tip_info();
            SendMessageW(self.tooltip, TTM_ADDTOOLW, 0, &info as *const _ as isize);
            SendMessageW(
                self.tooltip,
                TTM_SETMAXTIPWIDTH,
                0,
                (420. * self.scale) as isize,
            );
            SendMessageW(self.tooltip, WM_SETFONT, self.fonts.ui.0 as usize, 0);
        }
    }
    fn show_tip(&mut self, p: POINT) {
        let text = match self.hover {
            Hit::Entry(common, i) => self
                .config
                .group(self.page, common)
                .get(i)
                .map(|e| format!("{}\n{}", e.label(self.en()), e.target)),
            Hit::Tool(id) => Some(
                match id {
                    1 => self.tr("表示を固定", "Keep open"),
                    ADD_APP => self.tr("アプリを追加", "Add app"),
                    ADD_FOLDER => self.tr("共通ボタンを追加", "Add shared button"),
                    2 => self.tr("メニュー", "Menu"),
                    3 => self.tr("隠す", "Hide"),
                    4 => self.tr("ゴミ箱を表示", "Open Recycle Bin"),
                    _ => self.tr("ゴミ箱を空にする", "Empty Recycle Bin"),
                }
                .to_string(),
            ),
            _ => None,
        };
        if let Some(text) = text {
            let next = wide(&text);
            let mut info = self.tip_info();
            info.lpszText = next.as_ptr().cast_mut();
            unsafe {
                SendMessageW(
                    self.tooltip,
                    TTM_UPDATETIPTEXTW,
                    0,
                    &info as *const _ as isize,
                );
                self.tooltip_text = next;
                SendMessageW(
                    self.tooltip,
                    TTM_TRACKPOSITION,
                    0,
                    (((p.y + 18) as u32 & 0xffff) << 16 | ((p.x + 12) as u32 & 0xffff)) as isize,
                );
                SendMessageW(
                    self.tooltip,
                    TTM_TRACKACTIVATE,
                    1,
                    &info as *const _ as isize,
                );
            }
            self.tooltip_active = true;
        }
    }
    fn icon_pixels(&self, common: bool) -> u32 {
        if !common && self.config.settings.app_icon_only {
            (48. * self.scale).round() as u32
        } else {
            0
        }
    }
    pub fn new(config: Config, path: PathBuf, preview: Option<PathBuf>) -> Self {
        let icons = crate::icon_store::Store::new();
        let fonts = Fonts::new(&config.settings, 1.);
        let initial_layout = crate::geometry::calculate(
            &config.settings,
            config.pages.len(),
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1200,
            },
            96,
        );
        Self {
            hwnd: null_mut(),
            shared: null(),
            snapshot: model::Snapshot::Missing,
            runtime: Rc::new(Runtime::default()),
            revision: 0,
            dialog_generation: 0,
            recycle_generation: 0,
            config,
            path,
            page: 0,
            dialog: null_mut(),
            pinned: false,
            scale: 1.,
            fonts,
            layout: initial_layout,
            context: None,
            icons,
            tray_icon: os::tray_icon(),
            tray_added: false,
            tray_retry: None,
            tray_warning: false,
            reveal_generation: 0,
            wheel_app: crate::wheel::Wheel::default(),
            wheel_folder: crate::wheel::Wheel::default(),
            hover: Hit::Nothing,
            pressed: Hit::Nothing,
            down: (0, 0),
            dragging: false,
            scroll_app: 0,
            scroll_folder: 0,
            detector: Detector::default(),
            start: Instant::now(),
            last_pos: None,
            last_move: 0.,
            last_inside: 0.,
            last_fullscreen: 0.,
            in_fullscreen: false,
            poll_ms: 80,
            outside_click: crate::outside_click::State::default(),
            preview,
            captured: false,
            exiting: false,
            runtime_gesture: true,
            capture: None,
            tooltip: null_mut(),
            tooltip_text: vec![0],
            tooltip_active: false,
            hover_at: 0.,
        }
    }
    fn current_dpi(&self) -> u32 {
        (self.scale * 96.).round().max(1.) as u32
    }
    pub(crate) fn reset_wheels(&mut self) {
        self.wheel_app.reset();
        self.wheel_folder.reset();
    }
    pub fn relayout(&mut self) {
        let work = os::work_area(self.hwnd);
        let layout = crate::geometry::calculate(
            &self.config.settings,
            self.config.pages.len(),
            work,
            self.current_dpi(),
        );
        self.install_layout(layout, true);
    }
    fn install_layout(&mut self, layout: Layout, resize: bool) {
        self.hide_tooltip();
        self.hover = Hit::Nothing;
        self.reset_wheels();
        self.page = self.page.min(self.config.pages.len() - 1);
        self.layout = layout;
        #[cfg(feature = "review-test")]
        crate::stage3_probe::layout(&self.layout, self.scale);
        let s = &self.config.settings;
        let wanted_app = crate::geometry::wanted(s.app_rows, false);
        let wanted_folder = crate::geometry::wanted(s.folder_rows, true);
        self.scroll_app = self.scroll_app.min((wanted_app - layout.apps.h).max(0));
        self.scroll_folder = self
            .scroll_folder
            .min((wanted_folder - layout.folders.h).max(0));
        self.fonts = Fonts::new(s, self.scale);
        if !self.tooltip.is_null() {
            unsafe {
                SendMessageW(self.tooltip, WM_SETFONT, self.fonts.ui.0 as usize, 0);
            }
        }
        let r = R::new(0, 0, layout.width, layout.height).scale(self.scale);
        unsafe {
            if resize {
                SetWindowPos(
                    self.hwnd,
                    null_mut(),
                    0,
                    0,
                    r.w,
                    r.h,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            let corner = (36. * self.scale) as i32;
            let region = CreateRoundRectRgn(
                (6. * self.scale) as i32,
                (4. * self.scale) as i32,
                r.w - (6. * self.scale) as i32 + 1,
                r.h - (6. * self.scale) as i32 + 1,
                corner,
                corner,
            );
            if SetWindowRgn(self.hwnd, region, if resize { 1 } else { 0 }) == 0 {
                DeleteObject(region);
            }
        }
        let mut entries: Vec<_> = self
            .config
            .pages
            .iter()
            .flat_map(|p| &p.items)
            .filter(|e| !e.recycle())
            .map(|entry| (entry.clone(), self.icon_pixels(false)))
            .collect();
        entries.extend(
            self.config
                .common
                .iter()
                .filter(|entry| !entry.recycle())
                .map(|entry| (entry.clone(), 0)),
        );
        self.icons.sync(entries, self.icon_pixels(false));
        self.invalidate();
    }
    pub fn commit(&mut self, candidate: Config) -> bool {
        match model::save_checked(&self.path, &candidate, &self.snapshot) {
            Ok((c, snapshot)) => {
                self.config = c;
                self.snapshot = snapshot;
                self.revision += 1;
                self.relayout();
                true
            }
            Err(e) => {
                self.message(&e, true);
                false
            }
        }
    }
    pub fn dialog_or_main(&self) -> HWND {
        if self.dialog.is_null() {
            self.hwnd
        } else {
            self.dialog
        }
    }
    pub(crate) fn clear_icon_only_hover(&mut self) {
        if self.config.settings.app_icon_only && matches!(self.hover, Hit::Entry(false, _)) {
            self.hover = Hit::Nothing;
            self.hide_tooltip();
            self.invalidate();
        }
    }
    pub fn reveal(&mut self) {
        self.hide_tooltip();
        unsafe {
            if !self.dialog.is_null() {
                os::raise_window(self.dialog);
                return;
            }
            self.clear_icon_only_hover();
            let mut cursor: POINT = std::mem::zeroed();
            GetCursorPos(&mut cursor);
            #[cfg(feature = "review-test")]
            {
                cursor = crate::stage3_probe::cursor(cursor);
            }
            let (work, queried_dpi) = os::cursor_monitor(cursor);
            let dpi = queried_dpi.unwrap_or_else(|| self.current_dpi());
            let layout = crate::geometry::calculate(
                &self.config.settings,
                self.config.pages.len(),
                work,
                dpi,
            );
            let rect = crate::geometry::place(&layout, (cursor.x, cursor.y), work, dpi);
            self.reveal_generation = self.reveal_generation.wrapping_add(1);
            let _scope = queried_dpi.map(|_| {
                self.runtime.enter_reveal(crate::runtime::RevealPlan {
                    generation: self.reveal_generation,
                    cursor,
                })
            });
            let old_scale = self.scale;
            self.scale = dpi as f64 / 96.;
            self.reset_wheels();
            if old_scale != self.scale || self.layout != layout {
                self.install_layout(layout, false);
            }
            #[cfg(feature = "review-test")]
            crate::stage3_probe::placement(self.hwnd, rect.x, rect.y, rect.w, rect.h);
            SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                SWP_SHOWWINDOW | SWP_NOACTIVATE,
            );
            os::raise_window(self.hwnd);
        }
        self.detector.reset();
        self.last_inside = self.start.elapsed().as_secs_f64();
        self.outside_click = crate::outside_click::State {
            previous: os::mouse_buttons(),
            ..Default::default()
        };
        self.runtime.outside_drop.set(false);
        self.runtime.outside_cancel.set(false);
        self.interval(16);
    }
    pub fn hide(&mut self) {
        self.hide_tooltip();
        if !self.runtime.editing_blocked() && os::owner_available(self.hwnd, self.tooltip) {
            self.clear_icon_only_hover();
            self.reset_wheels();
            unsafe {
                ShowWindow(self.hwnd, SW_HIDE);
            }
            self.outside_click = Default::default();
            self.runtime.outside_drop.set(false);
            self.interval(80);
            self.detector.reset();
        }
    }
    pub fn tool_rects(&self) -> Vec<(usize, R)> {
        let mut c = self.layout.controls;
        c.y -= self.scroll_app;
        let end = c.x + c.w - 5;
        vec![
            (1, R::new(end - 124, c.y + 27, 28, 28)),
            (ADD_APP, R::new(end - 92, c.y + 27, 28, 28)),
            (2, R::new(end - 60, c.y + 27, 28, 28)),
            (3, R::new(end - 28, c.y + 27, 28, 28)),
            (
                ADD_FOLDER,
                R::new(self.layout.rail + 16, self.layout.folders.y, 28, 28),
            ),
            (
                4,
                R::new(self.layout.rail, self.layout.folders.y + 34, 44, 44),
            ),
            (
                5,
                R::new(self.layout.rail, self.layout.folders.y + 84, 44, 44),
            ),
        ]
    }
    pub fn hit(&self, x: i32, y: i32, slot: bool) -> Hit {
        for (id, r) in self.tool_rects() {
            if r.has(x, y) && (id == ADD_FOLDER || id == 4 || id == 5 || self.layout.apps.has(x, y))
            {
                return Hit::Tool(id);
            }
        }
        if self.config.pages.len() > 1
            && (crate::geometry::TOP_MARGIN..crate::geometry::TOP_MARGIN + 30).contains(&y)
        {
            for i in 0..self.config.pages.len() {
                if R::new(
                    crate::geometry::SIDE_MARGIN + i as i32 * 110,
                    crate::geometry::TOP_MARGIN,
                    106,
                    28,
                )
                .has(x, y)
                {
                    return Hit::Page(i);
                }
            }
        }
        for common in [false, true] {
            let area = if common {
                self.layout.folders
            } else {
                self.layout.apps
            };
            if !area.has(x, y) {
                continue;
            }
            let size = crate::geometry::step(common);
            let scroll = if common {
                self.scroll_folder
            } else {
                self.scroll_app
            };
            let cell = [
                ((y - area.y + scroll) / size) as usize,
                ((x - area.x) / size) as usize,
            ];
            let (rows, cols) = self.config.dimensions(common);
            if cell[0] >= rows || cell[1] >= cols || self.config.reserved(common, cell) {
                continue;
            }
            let r = R::new(
                area.x + cell[1] as i32 * size,
                area.y + cell[0] as i32 * size - scroll,
                size,
                size,
            );
            if let Some(index) = self
                .config
                .group(self.page, common)
                .iter()
                .position(|e| !e.recycle() && e.cell == Some(cell))
                && (slot || crate::geometry::card(r).has(x, y))
            {
                return Hit::Entry(common, index);
            }
            return Hit::Cell(common, cell);
        }
        Hit::Nothing
    }
    pub fn draw(&self, c: &paint::Canvas) {
        let p = paint::palette(self.config.settings.theme == "dark");
        let s = self.scale;
        let l = self.layout;
        let rr = |r: R| r.scale(s);
        c.round(
            rr(R::new(6, 4, l.width - 12, l.height - 10)),
            18. * s as f32,
            p.top,
            p.bottom,
            p.border,
            s as f32,
        );
        if self.config.pages.len() > 1 {
            for (i, page) in self.config.pages.iter().enumerate() {
                let r = R::new(
                    crate::geometry::SIDE_MARGIN + i as i32 * 110,
                    crate::geometry::TOP_MARGIN,
                    106,
                    28,
                );
                if i == self.page {
                    c.round(rr(r), 6. * s as f32, p.hover, p.hover_bottom, p.border, 0.);
                }
                c.text(
                    &page.name,
                    rr(r.inset(5)),
                    &self.fonts.ui,
                    if i == self.page { p.accent } else { p.quiet },
                    DT_CENTER | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
                );
            }
        }
        for common in [false, true] {
            let area = if common { l.folders } else { l.apps };
            let cellsize = crate::geometry::step(common);
            let scroll = if common {
                self.scroll_folder
            } else {
                self.scroll_app
            };
            c.clip(rr(area), || {
                for (i, e) in self
                    .config
                    .group(self.page, common)
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| !e.recycle())
                {
                    let cell = e.cell.unwrap();
                    let cell = R::new(
                        area.x + cell[1] as i32 * cellsize,
                        area.y + cell[0] as i32 * cellsize - scroll,
                        cellsize,
                        cellsize,
                    );
                    if cell.y + cell.h < area.y || cell.y > area.y + area.h {
                        continue;
                    }
                    let r = crate::geometry::card(cell);
                    let hover = self.hover == Hit::Entry(common, i);
                    #[cfg(feature = "review-test")]
                    let hover = hover
                        || (!common
                            && crate::icon_only_probe::fixture()["hover"].as_u64()
                                == Some(i as u64))
                        || (common
                            && crate::icon_only_probe::fixture()["common_hover"].as_u64()
                                == Some(i as u64));
                    let (card, card_bottom) = if common {
                        crate::folder_color::selected(e)
                            .map(|index| {
                                let color = &crate::folder_color::COLORS[index];
                                if self.config.settings.theme == "dark" {
                                    color.dark
                                } else {
                                    color.light
                                }
                            })
                            .unwrap_or((p.card, p.card_bottom))
                    } else {
                        (p.card, p.card_bottom)
                    };
                    c.round(
                        rr(r),
                        10. * s as f32,
                        if hover { p.hover } else { card },
                        if hover { p.hover_bottom } else { card_bottom },
                        if hover { p.hover_border } else { p.card_border },
                        s as f32,
                    );
                    if !common && self.config.settings.app_icon_only {
                        let pixels = self.icon_pixels(false);
                        if let Some(icon) =
                            self.icons.get(e, pixels).filter(|icon| !icon.0.is_null())
                        {
                            c.icon(
                                icon,
                                rr(R::new(r.x + (r.w - 48) / 2, r.y + (r.h - 48) / 2, 48, 48)),
                            );
                        } else {
                            c.text(
                                "◇",
                                rr(r),
                                &self.fonts.app,
                                p.quiet,
                                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                            );
                        }
                        let effects = hover;
                        #[cfg(feature = "review-test")]
                        let effects = effects
                            && crate::icon_only_probe::fixture()["skip_overlay"].as_bool()
                                != Some(true);
                        if effects {
                            c.icon_only_hover(
                                &e.label(self.en()),
                                rr(r.inset(3)),
                                rr(r.inset(4)),
                                &self.fonts.app,
                                p,
                                s,
                            );
                        }
                        continue;
                    }
                    let size = if common { 22 } else { 28 };
                    if let Some(icon) = self.icons.get(e, 0) {
                        c.icon(
                            icon,
                            rr(R::new(r.x + (r.w - size) / 2, r.y + 8, size, size)),
                        );
                    }
                    let top = if common { 35 } else { 44 };
                    c.text(
                        &e.label(self.en()),
                        rr(R::new(r.x + 4, r.y + top, r.w - 8, r.h - top - 4)),
                        if common {
                            &self.fonts.folder
                        } else {
                            &self.fonts.app
                        },
                        if hover { p.accent } else { p.text },
                        DT_CENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
                    );
                }
                if !common {
                    let mut r = l.controls;
                    r.y -= scroll;
                    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
                    let end = r.x + r.w - 5;
                    c.text(
                        &version,
                        rr(R::new(end - 43, r.y + 7, 43, 17)),
                        &self.fonts.quiet,
                        p.quiet,
                        DT_RIGHT | DT_SINGLELINE,
                    );
                    c.text(
                        "Launcher",
                        rr(R::new(end - 97, r.y + 7, 49, 17)),
                        &self.fonts.quiet,
                        p.quiet,
                        DT_RIGHT | DT_SINGLELINE,
                    );
                    c.text(
                        "orbit",
                        rr(R::new(end - 137, r.y + 7, 35, 17)),
                        &self.fonts.brand,
                        p.text,
                        DT_RIGHT | DT_SINGLELINE,
                    );
                    for (id, r) in self
                        .tool_rects()
                        .into_iter()
                        .filter(|(id, _)| [1, ADD_APP, 2, 3].contains(id))
                    {
                        self.draw_tool(c, id, r, p);
                    }
                }
            });
        }
        c.line(
            (crate::geometry::SIDE_MARGIN as f64 * s) as f32,
            (l.divider as f64 * s) as f32,
            ((l.width - crate::geometry::SIDE_MARGIN) as f64 * s) as f32,
            (l.divider as f64 * s) as f32,
            p.border,
            s as f32,
        );
        for (id, r) in self
            .tool_rects()
            .into_iter()
            .filter(|(id, _)| [ADD_FOLDER, 4, 5].contains(id))
        {
            self.draw_tool(c, id, r, p);
        }
    }
    fn draw_tool(&self, c: &paint::Canvas, id: usize, r: R, p: paint::Palette) {
        let hover = self.hover == Hit::Tool(id);
        let s = self.scale;
        let r = r.scale(s);
        if id == 4 || id == 5 || hover || (id == 1 && self.pinned) {
            c.round(
                r,
                if id >= 4 {
                    10. * s as f32
                } else {
                    6. * s as f32
                },
                if hover { p.hover } else { p.card_bottom },
                if hover { p.hover_bottom } else { p.card_bottom },
                if hover { p.hover_border } else { p.border },
                if id == 4 || id == 5 { s as f32 } else { 0. },
            );
        }
        if id == 4 || id == 5 {
            c.bin(r, id == 5, p, s);
            return;
        }
        let text = match id {
            1 => {
                if self.pinned {
                    "◆"
                } else {
                    "◇"
                }
            }
            ADD_APP | ADD_FOLDER => "+",
            2 => "⋯",
            _ => "×",
        };
        c.text(
            text,
            r,
            if matches!(id, 1 | ADD_APP | 2 | 3) {
                &self.fonts.tool_glyph
            } else {
                &self.fonts.ui
            },
            if hover || id == 1 && self.pinned {
                p.accent
            } else {
                p.text
            },
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
    }
    pub fn launch(&mut self, e: Entry) {
        if self.interaction_blocked() {
            return;
        }
        if e.kind == "recycle_empty" {
            if self.runtime.recycle.get().is_some() {
                return;
            }
            self.recycle_generation += 1;
            let generation = self.recycle_generation;
            self.runtime.recycle.set(Some(generation));
            os::empty_bin_async(self.hwnd, RECYCLE_DONE, generation);
            return;
        }
        if let Err(error) = os::open(&e) {
            self.message(&error, true);
        } else if !self.pinned {
            self.hide();
        }
    }
    pub fn action(&mut self, id: usize) {
        // Check before touching context, page or any persistent settings.
        if self.interaction_blocked() && id != 2 && id != QUIT {
            os::raise_window(self.dialog_or_main());
            return;
        }
        match id {
            1 => {
                self.pinned = !self.pinned;
                self.invalidate();
            }
            2 => self.menu(false, None),
            3 => self.hide(),
            4 => self.launch(Entry::new("", "", "recycle_open")),
            5 => self.launch(Entry::new("", "", "recycle_empty")),
            SETTINGS => dialog::open(self, dialog::Mode::Settings),
            ADD_APP | ADD_FOLDER => {
                self.context = Some((id == ADD_FOLDER, None, None));
                dialog::open(self, dialog::Mode::Entry(None));
            }
            ADD_PAGE => dialog::open(self, dialog::Mode::Page(false)),
            RENAME_PAGE => dialog::open(self, dialog::Mode::Page(true)),
            REMOVE_PAGE => {
                if self.config.pages.len() > 1 && self.config.pages[self.page].items.is_empty() {
                    let mut c = self.config.clone();
                    c.pages.remove(self.page);
                    self.page = 0;
                    self.commit(c);
                }
            }
            106 => {
                let _ = os::open(&Entry::new(
                    "",
                    &self
                        .path
                        .parent()
                        .unwrap_or(std::path::Path::new("."))
                        .to_string_lossy(),
                    "folder",
                ));
            }
            QUIT => self.quit(),
            GESTURE => {
                self.runtime_gesture = true;
                let mut c = self.config.clone();
                c.settings.gesture_enabled = !c.settings.gesture_enabled;
                if self.commit(c) {
                    self.detector.reset();
                }
            }
            109=>{
                if self.capture.is_none()&&self.confirm(self.tr(
                    "ランチャーを隠し、20秒間だけ相対座標を記録します。キー・画面・絶対座標は保存しません。開始しますか？",
                    "Hide the launcher and record relative pointer motion for 20 seconds? No keys, screenshots or absolute coordinates are saved.")){
                    self.detector.reset();
                    self.capture=Some(crate::diagnostics::Capture::new(self.start.elapsed().as_secs_f64(),self.config.settings.clone()));
                    unsafe{ShowWindow(self.hwnd,SW_HIDE);}self.interval(16);
                }
            }
            200 => {
                if let Some((common, Some(i), _)) = self.context {
                    dialog::open(
                        self,
                        dialog::Mode::Entry(Some(self.config.group(self.page, common)[i].clone())),
                    );
                }
            }
            201 => {
                if let Some((common, Some(i), _)) = self.context
                    && self.confirm(
                        self.tr(
                            "登録ボタンを削除しますか？元のファイルは削除しません。",
                            "Remove this button? The original file is not deleted.",
                        ),
                    )
                {
                    let mut c = self.config.clone();
                    c.group_mut(self.page, common).remove(i);
                    self.commit(c);
                }
            }
            202 => {
                if let Some((common, Some(i), _)) = self.context {
                    let mut c = self.config.clone();
                    let mut e = c.group_mut(self.page, common).remove(i);
                    e.cell = None;
                    c.group_mut(self.page, !common).push(e);
                    self.commit(c);
                }
            }
            203 => dialog::open(self, dialog::Mode::Entry(None)),
            COLOR_NONE..=COLOR_LAST => {
                if let Some((true, Some(index), _)) = self.context {
                    let mut candidate = self.config.clone();
                    // Context comes from hit(), which excludes recycle entries.
                    if let Some(entry) = candidate.common.get_mut(index) {
                        if id == COLOR_NONE {
                            entry.extra.remove("color");
                        } else {
                            entry.extra.insert("color".into(), serde_json::Value::String(
                                crate::folder_color::COLORS[id - COLOR_NONE - 1].name.into(),
                            ));
                        }
                        self.commit(candidate);
                    }
                }
            }
            OPEN_FILE_LOCATION => {
                if let Some((common, Some(index), _)) = self.context
                    && let Some(entry) = self.config.group(self.page, common).get(index).cloned()
                {
                    match os::location_for_entry(&entry) {
                        crate::path_decisions::Location::Disabled => {},
                        crate::path_decisions::Location::Missing => self.message(
                            self.tr("ファイルまたはフォルダが見つかりません。登録先のパスを確認してください。", "File or folder not found. Check the registered path."), true),
                        location => {
                            if let Err(error) = os::open_location(&location) {
                                self.message(&error, true);
                            } else if !self.pinned {
                                self.hide();
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    pub fn menu(&mut self, tray: bool, hit: Option<Hit>) {
        if self.runtime.nested() {
            return;
        }
        self.hide_tooltip();
        unsafe {
            let menu = CreatePopupMenu();
            let add = |id: usize, label: &str, flags: u32| {
                let blocked = (self.runtime.dialog.get().is_some()
                    || !os::owner_available(self.hwnd, self.tooltip))
                    && id != 150
                    || id == QUIT && self.runtime.recycle.get().is_some();
                AppendMenuW(
                    menu,
                    MF_STRING | flags | if blocked { MF_GRAYED } else { 0 },
                    id,
                    wide(label).as_ptr(),
                );
            };
            if let Some(h) = hit {
                if self.runtime.dialog.get().is_some() {
                    DestroyMenu(menu);
                    return;
                }
                self.context = match h {
                    Hit::Entry(common, index) => Some((
                        common,
                        Some(index),
                        self.config.group(self.page, common)[index].cell,
                    )),
                    Hit::Cell(common, cell) => Some((common, None, Some(cell))),
                    _ => Some((false, None, None)),
                };
                add(203, self.tr("ボタンを追加…", "Add button…"), 0);
                if matches!(h, Hit::Entry(..)) {
                    add(200, self.tr("編集…", "Edit…"), 0);
                    if let Hit::Entry(common, index) = h {
                        let entry = &self.config.group(self.page, common)[index];
                        add(
                            OPEN_FILE_LOCATION,
                            self.tr("ファイルの場所を開く", "Open file location"),
                            if os::location_enabled(entry) {
                                0
                            } else {
                                MF_GRAYED
                            },
                        );
                        // Hit::Entry is produced only for non-recycle registrations.
                        if common {
                            // The parent menu owns its submenu after AppendMenuW succeeds.
                            let colors = CreatePopupMenu();
                            if !colors.is_null() {
                                let selected =
                                    crate::folder_color::selected(entry).map_or(0, |i| i + 1);
                                for (index, (ja, en)) in std::iter::once(("なし", "None"))
                                    .chain(crate::folder_color::COLORS.iter().map(|c| (c.ja, c.en)))
                                    .enumerate()
                                {
                                    AppendMenuW(
                                        colors,
                                        MF_STRING | if index == selected { MF_CHECKED } else { 0 },
                                        COLOR_NONE + index,
                                        wide(self.tr(ja, en)).as_ptr(),
                                    );
                                }
                                if AppendMenuW(
                                    menu,
                                    MF_POPUP
                                        | MF_STRING
                                        | if !os::owner_available(self.hwnd, self.tooltip) {
                                            MF_GRAYED
                                        } else {
                                            0
                                        },
                                    colors as usize,
                                    wide(self.tr("色", "Color")).as_ptr(),
                                ) == 0
                                {
                                    DestroyMenu(colors);
                                }
                            }
                        }
                    }
                    add(202, self.tr("上下段へ移動", "Move to other area"), 0);
                    add(201, self.tr("登録を削除…", "Remove button…"), 0);
                }
            } else {
                if tray {
                    add(150, self.tr("表示", "Show"), 0);
                }
                add(
                    SETTINGS,
                    self.tr(
                        "ジェスチャー・表示設定…",
                        "Gesture and appearance settings…",
                    ),
                    0,
                );
                add(ADD_APP, self.tr("アプリを追加…", "Add app…"), 0);
                add(
                    ADD_FOLDER,
                    self.tr("共通ボタンを追加…", "Add shared button…"),
                    0,
                );
                add(ADD_PAGE, self.tr("ページを追加…", "Add page…"), 0);
                add(RENAME_PAGE, self.tr("ページ名を変更…", "Rename page…"), 0);
                if self.config.pages.len() > 1 && self.config.pages[self.page].items.is_empty() {
                    add(
                        REMOVE_PAGE,
                        self.tr("空のページを削除", "Remove empty page"),
                        0,
                    );
                }
                add(
                    GESTURE,
                    self.tr("円描画を有効にする", "Enable circle gesture"),
                    if self.config.settings.gesture_enabled {
                        MF_CHECKED
                    } else {
                        0
                    },
                );
                add(
                    106,
                    self.tr("設定フォルダを開く", "Open settings folder"),
                    0,
                );
                add(
                    109,
                    self.tr("円描画の診断ログ…", "Record gesture diagnostics…"),
                    0,
                );
                AppendMenuW(menu, MF_SEPARATOR, 0, null());
                add(QUIT, self.tr("終了", "Quit"), 0);
            }
            #[cfg(feature = "review-test")]
            os::record_location_menu(menu);
            let mut p: POINT = std::mem::zeroed();
            GetCursorPos(&mut p);
            SetForegroundWindow(self.hwnd);
            let guard = self.runtime.enter(Scope::Menu);
            let selected = TrackPopupMenuEx(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY,
                p.x,
                p.y,
                self.hwnd,
                null(),
            ) as usize;
            drop(guard);
            DestroyMenu(menu);
            PostMessageW(self.hwnd, WM_NULL, 0, 0);
            self.last_inside = self.start.elapsed().as_secs_f64();
            if selected == 150 {
                self.reveal();
            } else {
                self.action(selected);
            }
        }
    }
    pub fn quit(&mut self) {
        if self.runtime.quit_blocked() || !os::owner_available(self.dialog_or_main(), self.tooltip)
        {
            // Avoid opening yet another modal while a native chooser is running.
            if self.runtime.nested() || !os::owner_available(self.dialog_or_main(), self.tooltip) {
                return;
            }
            self.message(
                self.tr(
                    "処理の完了またはキャンセルを待ってください。",
                    "Wait for the operation to finish or cancel.",
                ),
                false,
            );
            return;
        }
        self.exiting = true;
        unsafe {
            DestroyWindow(self.hwnd);
        }
    }
    fn poll(&mut self) {
        self.drain_notifications();
        // A chooser's modal loop may dispatch timers without borrowing App.
        // Finish diagnostics after that chooser returns, rather than stacking
        // another modal window on a disabled owner.
        let visible = unsafe { IsWindowVisible(self.hwnd) != 0 };
        let needs_owner_check = visible
            || !self.dialog.is_null()
            || self.runtime.recycle.get().is_some()
            || self
                .capture
                .as_ref()
                .is_some_and(|c| self.start.elapsed().as_secs_f64() - c.started >= 20.);
        if self.runtime.nested()
            || needs_owner_check && !os::owner_available(self.dialog_or_main(), self.tooltip)
        {
            if visible {
                self.outside_click = crate::outside_click::State {
                    previous: os::mouse_buttons(),
                    ..Default::default()
                };
                self.runtime.outside_drop.set(false);
            }
            return;
        }
        self.follow_monitor_dpi(visible);
        if self.tray_warning {
            self.tray_warning = false;
            self.message(
                self.tr(
                    "トレイアイコンを復旧できませんでした。円またはホットキーで表示できます。",
                    "Unable to restore the tray icon. Use the gesture or hotkey to show Orbit.",
                ),
                true,
            );
        }
        if self.icons.poll() {
            self.invalidate();
        }
        let now = self.start.elapsed().as_secs_f64();
        if self
            .capture
            .as_ref()
            .is_some_and(|c| now - c.started >= 20.)
        {
            let capture = self.capture.take().unwrap();
            let folder = self
                .path
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("captures");
            match capture.save(&folder) {
                Ok(_) => self.message(
                    self.tr(
                        "診断ログを設定フォルダ内のcapturesへ保存しました。",
                        "Gesture log saved in the settings folder's captures directory.",
                    ),
                    false,
                ),
                Err(e) => self.message(&e, true),
            }
        }
        if self.preview.is_some() {
            if !self.captured && ((self.icons.ready() && now > 0.5) || now > 4.) {
                self.captured = true;
                #[cfg(feature = "review-test")]
                {
                    if let Some(index) = crate::icon_only_probe::fixture()["hover"].as_u64() {
                        self.hover = Hit::Entry(false, index as usize);
                    }
                    crate::icon_only_probe::event("icons", self.icons.counts());
                }
                if let Some(path) = &self.preview {
                    let _ = os::capture(self.hwnd, path, |c| self.draw(c));
                }
                self.quit();
            }
            return;
        }
        unsafe {
            let mut p: POINT = std::mem::zeroed();
            let cursor_available = GetCursorPos(&mut p) != 0;
            #[cfg(feature = "review-test")]
            let cursor_available =
                if visible && let Some(sample) = crate::outside_click_probe::sample() {
                    let mut rect: RECT = std::mem::zeroed();
                    GetWindowRect(self.hwnd, &mut rect);
                    p = if sample.inside {
                        POINT {
                            x: rect.left + 10,
                            y: rect.top + 10,
                        }
                    } else {
                        POINT {
                            x: -30000,
                            y: -30000,
                        }
                    };
                    true
                } else {
                    cursor_available
                };
            if !cursor_available {
                if visible {
                    self.outside_click = Default::default();
                    self.runtime.outside_cancel.set(true);
                }
                if let Some(c) = &mut self.capture {
                    c.record(now, None, "cursor_unavailable", false);
                }
                self.detector.reset();
                return;
            }
            if visible {
                self.record(now, p, "launcher_visible", false);
                self.detector.reset();
                let blocked = self.interaction_blocked()
                    || !self.dialog.is_null()
                    || self.capture.is_some()
                    || self.runtime.outside_cancel.replace(false);
                let (next, close) = crate::outside_click::step(
                    self.outside_click,
                    crate::outside_click::Input {
                        buttons: os::mouse_buttons(),
                        inside: os::orbit_at_point(p),
                        tray: os::own_tray_at_point(self.hwnd, p),
                        pinned: self.pinned,
                        blocked,
                        drop_received: self.runtime.outside_drop.replace(false),
                    },
                );
                self.outside_click = next;
                if close {
                    self.hide();
                    return;
                }
                let mut r: RECT = std::mem::zeroed();
                GetWindowRect(self.hwnd, &mut r);
                if p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom {
                    self.last_inside = now;
                } else if self.config.settings.auto_hide
                    && !self.pinned
                    && !self.runtime.editing_blocked()
                    && os::owner_available(self.hwnd, self.tooltip)
                    && self.dialog.is_null()
                    && now - self.last_inside > 0.75
                {
                    self.hide();
                }
                if IsWindowVisible(self.hwnd) != 0 {
                    self.interval(16);
                }
                if self.dialog.is_null()
                    && !self.runtime.editing_blocked()
                    && !self.tooltip_active
                    && now - self.hover_at > 0.6
                {
                    self.show_tip(p);
                }
                return;
            }
            if (!self.runtime_gesture || !self.config.settings.gesture_enabled)
                && self.capture.is_none()
                || !self.dialog.is_null()
            {
                self.record(now, p, "disabled", false);
                self.detector.reset();
                self.interval(160);
                return;
            }
            if self.last_pos == Some((p.x, p.y)) {
                self.record(now, p, "stationary", false);
                if now - self.last_move > 0.5 && self.capture.is_none() {
                    self.interval(80);
                }
                return;
            }
            self.last_pos = Some((p.x, p.y));
            self.last_move = now;
            self.interval(16);
            if [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON, VK_XBUTTON1, VK_XBUTTON2]
                .iter()
                .any(|k| GetAsyncKeyState(*k as i32) < 0)
            {
                self.record(now, p, "mouse_button_pressed", false);
                self.detector.reset();
                return;
            }
            if self.config.settings.skip_fullscreen && now - self.last_fullscreen > 0.5 {
                self.in_fullscreen = os::fullscreen();
                self.last_fullscreen = now;
            }
            if self.config.settings.skip_fullscreen && self.in_fullscreen {
                self.record(now, p, "fullscreen", false);
                self.detector.reset();
                return;
            }
            let recognized = self
                .detector
                .feed(p.x as f64, p.y as f64, now, &self.config.settings);
            self.record(now, p, "tracking", recognized);
            if recognized && self.capture.is_none() {
                self.reveal();
            }
        }
    }

    fn follow_monitor_dpi(&mut self, visible: bool) {
        if !self.runtime.dpi_check.get() {
            return;
        }
        if !visible {
            self.runtime.dpi_check.set(false);
            return;
        }
        if !self.dialog.is_null() || self.runtime.dialog.get().is_some() {
            return;
        }
        self.runtime.dpi_check.set(false);
        let Some((target, device)) = os::window_monitor_dpi(self.hwnd) else {
            return;
        };
        let current = os::window_dpi(self.hwnd);
        if current == target {
            self.runtime.dpi_attempted.set(None);
            return;
        }
        let attempt = crate::runtime::DpiAttempt { device, target };
        if self.runtime.dpi_attempted.get() == Some(attempt) {
            return;
        }
        self.runtime.dpi_attempted.set(Some(attempt));
        unsafe {
            let mut original = RECT::default();
            if GetWindowRect(self.hwnd, &mut original) == 0 {
                return;
            }
            let work = os::work_area(self.hwnd);
            let delta = if original.right < work.right {
                (1, 0)
            } else if original.left > work.left {
                (-1, 0)
            } else if original.bottom < work.bottom {
                (0, 1)
            } else if original.top > work.top {
                (0, -1)
            } else {
                return;
            };
            let flags = SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
            #[cfg(feature = "review-test")]
            crate::stage3_probe::event(
                "dpi_nudge",
                serde_json::json!({"target":target,"delta":[delta.0,delta.1]}),
            );
            if SetWindowPos(
                self.hwnd,
                null_mut(),
                original.left + delta.0,
                original.top + delta.1,
                0,
                0,
                flags,
            ) == 0
            {
                return;
            }
            #[cfg(feature = "review-test")]
            crate::stage3_probe::nudge_notice(self.hwnd, target);
            // A synchronous WM_DPICHANGED only stored a value in Runtime while
            // App was borrowed. Apply it before restoring the original anchor.
            self.drain_notifications();
            let mut resized = RECT::default();
            if GetWindowRect(self.hwnd, &mut resized) == 0 {
                return;
            }
            let work = os::work_area(self.hwnd);
            let width = resized.right - resized.left;
            let height = resized.bottom - resized.top;
            let x = original.left.min(work.right - width).max(work.left);
            let y = original.top.min(work.bottom - height).max(work.top);
            SetWindowPos(self.hwnd, null_mut(), x, y, 0, 0, flags);
        }
    }
    fn record(&mut self, now: f64, p: POINT, state: &str, recognized: bool) {
        if let Some(c) = &mut self.capture {
            c.record(now, Some((p.x, p.y)), state, recognized);
        }
    }
    fn interval(&mut self, ms: u32) {
        if self.poll_ms != ms {
            self.poll_ms = ms;
            unsafe {
                SetTimer(self.hwnd, 1, ms, None);
            }
        }
    }
    pub fn file_drop(&mut self, drop: HDROP) {
        unsafe {
            if self.interaction_blocked() {
                DragFinish(drop);
                return;
            }
            let mut point: POINT = std::mem::zeroed();
            DragQueryPoint(drop, &mut point);
            let hit = self.hit(
                (point.x as f64 / self.scale) as i32,
                (point.y as f64 / self.scale) as i32,
                true,
            );
            let (common, cell) = match hit {
                Hit::Cell(c, p) => (c, Some(p)),
                Hit::Entry(c, _) => (c, None),
                _ => (
                    point.y as f64 / self.scale >= self.layout.folders.y as f64,
                    None,
                ),
            };
            let mut candidate = self.config.clone();
            for i in 0..DragQueryFileW(drop, u32::MAX, null_mut(), 0) {
                let length = DragQueryFileW(drop, i, null_mut(), 0);
                let mut text = vec![0u16; length as usize + 1];
                DragQueryFileW(drop, i, text.as_mut_ptr(), length + 1);
                let path = String::from_utf16_lossy(&text[..length as usize]);
                let remote = path.starts_with(r"\\");
                let kind = if (!remote && std::path::Path::new(&path).is_dir()) || remote && common
                {
                    "folder"
                } else {
                    "launch"
                };
                let name = std::path::Path::new(&path)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy();
                let mut e = Entry::new(&name, &path, kind);
                if i == 0 {
                    e.cell = cell;
                }
                candidate.group_mut(self.page, common).push(e);
            }
            DragFinish(drop);
            self.commit(candidate);
        }
    }
    fn restore_tray_if_due(&mut self) {
        if self.preview.is_some() || self.hwnd.is_null() {
            return;
        }
        if self.runtime.restore_tray.replace(false) && self.tray_retry.is_none() {
            self.tray_retry = Some((Instant::now(), 0));
        }
        let Some((due, count)) = self.tray_retry else {
            return;
        };
        if Instant::now() < due {
            return;
        }
        if self.tray(NIM_ADD) || self.tray(NIM_MODIFY) {
            self.tray_added = true;
            self.tray_retry = None;
            self.tray_warning = false;
        } else if count + 1 >= 5 {
            self.tray_retry = None;
            self.tray_warning = true;
        } else {
            self.tray_retry = Some((
                Instant::now() + std::time::Duration::from_millis(500),
                count + 1,
            ));
        }
    }
    fn tray(&mut self, op: u32) -> bool {
        unsafe {
            let mut data: NOTIFYICONDATAW = std::mem::zeroed();
            data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            data.hWnd = self.hwnd;
            data.uID = 1;
            data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
            data.uCallbackMessage = TRAY;
            data.hIcon = self.tray_icon.0;
            let text = wide("Orbit Launcher · Rust");
            data.szTip[..text.len()].copy_from_slice(&text);
            #[cfg(feature = "review-test")]
            {
                crate::stage3_probe::notify(op, &data) != 0
            }
            #[cfg(not(feature = "review-test"))]
            {
                Shell_NotifyIconW(op, &data) != 0
            }
        }
    }
}

pub unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        #[cfg(feature = "review-test")]
        crate::dpi_follow_probe::message(hwnd, msg, w, l);
        if msg == WM_NCCREATE {
            let create = &*(l as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            return 1;
        }
        if msg == WM_DESTROY {
            RemovePropW(hwnd, wide(crate::instance::READY).as_ptr());
            PostQuitMessage(0);
            return 0;
        }
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Shared;
        if ptr.is_null() {
            return DefWindowProcW(hwnd, msg, w, l);
        }
        if msg == WM_DROPFILES && IsWindowVisible(hwnd) != 0 {
            // Capture receipt even while a nested loop has borrowed App.
            let runtime = &(*ptr).runtime;
            runtime.outside_drop.set(true);
        }
        #[cfg(feature = "review-test")]
        if msg == WM_APP + 17 {
            // Test-build only: create a valid in-process RECT. No cross-process
            // pointer message and no physical pointer/monitor manipulation.
            let rect = RECT {
                left: 100,
                top: 100,
                right: 800,
                bottom: 600,
            };
            return window_proc(hwnd, WM_DPICHANGED, w, &rect as *const RECT as isize);
        }
        #[cfg(feature = "review-test")]
        if msg == WM_APP + 18 {
            let Ok(app) = (*ptr).app.try_borrow_mut() else {
                return 0;
            };
            let _guard = app.runtime.enter(Scope::Moving);
            os::message(
                hwnd,
                "Review-only synthetic move loop. The pointer is not moved.",
                false,
            );
            return 0;
        }
        #[cfg(feature = "review-test")]
        if msg == WM_APP + 19 {
            crate::stage3_probe::send_dpi(hwnd, w as u32);
            return 0;
        }
        #[cfg(feature = "review-test")]
        if msg == WM_APP + 20 {
            let Ok(app) = (*ptr).app.try_borrow_mut() else {
                return 0;
            };
            let _guard = app.runtime.enter(Scope::Moving);
            let fixture = crate::stage3_probe::fixture();
            if let Some(values) = fixture["move_dpis"].as_array() {
                for value in values {
                    if let Some(dpi) = value.as_u64() {
                        crate::stage3_probe::send_dpi(hwnd, dpi as u32);
                    }
                }
            }
            if let Some(rect) = crate::stage3_probe::rect("move_release") {
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    rect.left,
                    rect.top,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            drop(app);
            return 0;
        }
        #[cfg(feature = "review-test")]
        if msg == WM_APP + 90 {
            crate::outside_click_probe::set(w as u8, l);
            if let Ok(mut app) = (*ptr).app.try_borrow_mut() {
                app.poll();
            }
            return 0;
        }
        let runtime = &(*ptr).runtime;
        if runtime.taskbar_message.get() != 0 && msg == runtime.taskbar_message.get() {
            runtime.restore_tray.set(true);
            return 0;
        }
        match msg {
            WM_SETTINGCHANGE | WM_DISPLAYCHANGE => {
                runtime.dpi_check.set(true);
                return DefWindowProcW(hwnd, msg, w, l);
            }
            WM_DPICHANGED => {
                if l != 0 {
                    runtime.dpi.set(Some(Dpi {
                        value: (w & 0xffff) as u32,
                        rect: *(l as *const RECT),
                        reveal: runtime.reveal_plan.get(),
                        moving: runtime.moving(),
                    }));
                }
                return 0;
            }
            RECYCLE_DONE => {
                runtime.record_recycle_done(l as u64, w as i32);
                return 0;
            }
            WM_CLOSE => {
                runtime.close_main.set(true);
                return 0;
            }
            WM_ENTERSIZEMOVE => {
                return 0;
            }
            WM_NCLBUTTONDOWN if w == HTCAPTION as usize => {
                let _guard = runtime.enter(Scope::Moving);
                return DefWindowProcW(hwnd, msg, w, l);
            }
            WM_SYSCOMMAND if w & 0xfff0 == SC_MOVE as usize => {
                let _guard = runtime.enter(Scope::Moving);
                return DefWindowProcW(hwnd, msg, w, l);
            }
            _ => {}
        }
        let Ok(mut app) = (*ptr).app.try_borrow_mut() else {
            if msg == REVEAL {
                // A second launch must still reveal the active modal popup.
                // Foreground ordering needs no access to borrowed App state.
                os::raise_window(hwnd);
                return 0;
            }
            // These are commands, not notifications to replay. DefWindowProc may
            // perform destructive default actions (notably WM_CLOSE).
            if matches!(
                msg,
                WM_COMMAND
                    | WM_HOTKEY
                    | REVEAL
                    | TRAY
                    | WM_KEYDOWN
                    | WM_LBUTTONDOWN
                    | WM_LBUTTONUP
                    | WM_RBUTTONUP
                    | WM_MOUSEWHEEL
            ) {
                return 0;
            }
            if msg == WM_DROPFILES {
                DragFinish(w as HDROP);
                return 0;
            }
            return DefWindowProcW(hwnd, msg, w, l);
        };
        if msg != WM_PAINT && msg != WM_ERASEBKGND {
            app.drain_notifications();
        }
        match msg {
            #[cfg(feature = "review-test")]
            m if m == WM_APP + 80 => {
                if w == 3 || w == 4 || w == 6 {
                    app.hover = Hit::Entry(w == 6, 0);
                    if w == 4 {
                        app.hide();
                    }
                    app.reveal();
                    crate::icon_only_probe::event(
                        "hover_transition",
                        serde_json::json!({"operation":w,"hover":format!("{:?}",app.hover)}),
                    );
                }
                crate::icon_only_probe::event(
                    "state",
                    serde_json::json!({"icons":app.icons.counts(),"scale":app.scale,"icon_only":app.config.settings.app_icon_only,"hover":format!("{:?}",app.hover),"editing_blocked":app.runtime.editing_blocked(),"owner_available":os::owner_available(app.hwnd,app.tooltip),"dialog":app.dialog as usize}),
                );
                if w == 1
                    && let Some(path) = crate::icon_only_probe::fixture()["capture"].as_str()
                {
                    let _ =
                        os::capture(hwnd, std::path::Path::new(path), |canvas| app.draw(canvas));
                }
                0
            }
            WM_PAINT => {
                paint::buffered_paint(hwnd, |c| app.draw(c));
                0
            }
            WM_ERASEBKGND => 1,
            WM_TIMER => {
                app.poll();
                0
            }
            WM_HOTKEY | REVEAL => {
                app.reveal();
                0
            }
            WM_KEYDOWN => {
                if w == VK_ESCAPE as usize {
                    app.hide();
                }
                0
            }
            WM_MOUSEMOVE => {
                let x = ((l as i16) as f64 / app.scale) as i32;
                let y = (((l >> 16) as i16) as f64 / app.scale) as i32;
                let hit = app.hit(x, y, false);
                if app.hover != hit {
                    app.hide_tooltip();
                    app.hover_at = app.start.elapsed().as_secs_f64();
                    app.hover = hit;
                    app.invalidate();
                }
                if matches!(app.pressed, Hit::Entry(..))
                    && ((x - app.down.0).abs() > 5 || (y - app.down.1).abs() > 5)
                {
                    app.dragging = true;
                    SetCursor(LoadCursorW(null_mut(), IDC_SIZEALL));
                }
                let mut track = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut track);
                #[cfg(feature = "review-test")]
                if crate::icon_only_probe::fixture()["capture_on_move"].as_bool() == Some(true)
                    && let Some(path) = crate::icon_only_probe::fixture()["capture"].as_str()
                {
                    // Capture the real hit/hover update synchronously, before a synthetic
                    // TrackMouseEvent can notice the stationary real pointer is outside.
                    let _ =
                        os::capture(hwnd, std::path::Path::new(path), |canvas| app.draw(canvas));
                }
                0
            }
            WM_MOUSELEAVE => {
                app.hide_tooltip();
                app.hover = Hit::Nothing;
                app.invalidate();
                0
            }
            WM_LBUTTONDOWN => {
                if app.interaction_blocked() {
                    return 0;
                }
                app.hide_tooltip();
                let x = ((l as i16) as f64 / app.scale) as i32;
                let y = (((l >> 16) as i16) as f64 / app.scale) as i32;
                app.pressed = app.hit(x, y, false);
                app.down = (x, y);
                app.dragging = false;
                if matches!(app.pressed, Hit::Cell(..) | Hit::Nothing) {
                    ReleaseCapture();
                    let mut cursor: POINT = std::mem::zeroed();
                    GetCursorPos(&mut cursor);
                    let point = ((cursor.y as u32 & 0xffff) << 16) | (cursor.x as u32 & 0xffff);
                    SendMessageW(hwnd, WM_NCLBUTTONDOWN, HTCAPTION as usize, point as isize);
                    app.last_inside = app.start.elapsed().as_secs_f64();
                } else {
                    SetCapture(hwnd);
                }
                0
            }
            WM_LBUTTONUP => {
                if app.interaction_blocked() {
                    return 0;
                }
                ReleaseCapture();
                let x = ((l as i16) as f64 / app.scale) as i32;
                let y = (((l >> 16) as i16) as f64 / app.scale) as i32;
                let hit = app.hit(x, y, app.dragging);
                let pressed = app.pressed;
                app.pressed = Hit::Nothing;
                if app.dragging {
                    if let Hit::Entry(common, index) = pressed {
                        let cell = match hit {
                            Hit::Cell(c, cell) if c == common => Some(cell),
                            Hit::Entry(c, i) if c == common => {
                                app.config.group(app.page, c)[i].cell
                            }
                            _ => None,
                        };
                        if let Some(cell) = cell {
                            let mut c = app.config.clone();
                            let page = app.page;
                            let group = c.group_mut(page, common);
                            let old = group[index].cell;
                            if let Some(other) = group.iter().position(|e| e.cell == Some(cell)) {
                                group[other].cell = old;
                            }
                            group[index].cell = Some(cell);
                            app.commit(c);
                        }
                    }
                } else if hit == pressed {
                    match hit {
                        Hit::Entry(common, index) => {
                            let e = app.config.group(app.page, common)[index].clone();
                            app.launch(e);
                        }
                        Hit::Tool(id) => app.action(id),
                        Hit::Page(p) => {
                            app.page = p;
                            app.scroll_app = 0;
                            app.relayout();
                        }
                        _ => {}
                    }
                }
                app.dragging = false;
                0
            }
            WM_CAPTURECHANGED => {
                app.dragging = false;
                0
            }
            WM_RBUTTONUP => {
                let x = ((l as i16) as f64 / app.scale) as i32;
                let y = (((l >> 16) as i16) as f64 / app.scale) as i32;
                let hit = app.hit(x, y, true);
                app.menu(
                    false,
                    if matches!(hit, Hit::Entry(..) | Hit::Cell(..)) {
                        Some(hit)
                    } else {
                        None
                    },
                );
                0
            }
            WM_MOUSEWHEEL => {
                #[cfg(feature = "review-test")]
                if crate::icon_only_probe::fixture()["latch_wheel_hover"].as_bool() == Some(true) {
                    app.hover = Hit::Entry(false, 0);
                }
                let mut point = POINT {
                    x: (l as i16) as i32,
                    y: ((l >> 16) as i16) as i32,
                };
                ScreenToClient(hwnd, &mut point);
                let common = point.y as f64 / app.scale >= app.layout.folders.y as f64;
                let (rows, _) = app.config.dimensions(common);
                let size = crate::geometry::step(common);
                let area = if common {
                    app.layout.folders
                } else {
                    app.layout.apps
                };
                let maximum = (rows as i32 * size + 4 - area.h).max(0);
                let delta = ((w >> 16) as i16) as i32;
                let steps = if common {
                    app.wheel_folder.feed(delta)
                } else {
                    app.wheel_app.feed(delta)
                };
                let scroll = if common {
                    &mut app.scroll_folder
                } else {
                    &mut app.scroll_app
                };
                let previous = *scroll;
                *scroll = (*scroll - steps * size).clamp(0, maximum);
                if previous != *scroll {
                    app.clear_icon_only_hover();
                }
                #[cfg(feature = "review-test")]
                crate::stage3_probe::event(
                    "main_wheel",
                    serde_json::json!({"app":app.scroll_app,"folder":app.scroll_folder}),
                );
                app.invalidate();
                #[cfg(feature = "review-test")]
                crate::icon_only_probe::event(
                    "hover_wheel",
                    serde_json::json!({"hover":format!("{:?}",app.hover),"scroll":app.scroll_app}),
                );
                0
            }
            WM_DROPFILES => {
                app.file_drop(w as HDROP);
                0
            }
            WM_COMMAND => {
                app.action(w & 0xffff);
                0
            }
            TRAY => {
                match l as u32 {
                    WM_LBUTTONUP => app.reveal(),
                    WM_RBUTTONUP | WM_CONTEXTMENU => app.menu(true, None),
                    _ => {}
                }
                0
            }
            _ => DefWindowProcW(hwnd, msg, w, l),
        }
    }
}
pub fn run(
    path: PathBuf,
    hidden: bool,
    no_gesture: bool,
    preview: Option<PathBuf>,
) -> Result<(), String> {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let preview_mode = preview.is_some();
        let id = crate::identity::Identity::new(&path)?;
        let _instance = if preview_mode {
            None
        } else {
            match crate::instance::acquire(&id, &path)? {
                crate::instance::Start::Owner(owner) => Some(owner),
                crate::instance::Start::Revealed => return Ok(()),
            }
        };
        let path = id.path.clone();
        let com = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) >= 0;
        let _gdiplus = paint::GdiPlus::new()?;
        #[cfg(feature = "review-test")]
        crate::stage2_probe::checkpoint("before_load");
        let (mut config, mut snapshot) = model::load_snapshot(&path)?;
        #[cfg(feature = "review-test")]
        crate::stage2_probe::checkpoint("loaded");
        if snapshot == model::Snapshot::Missing && !preview_mode {
            (config, snapshot) = model::save_checked(&path, &config, &snapshot)?;
        }
        let class = wide(&format!(
            "{}{}",
            id.class,
            if preview_mode {
                format!("-{}", std::process::id())
            } else {
                String::new()
            }
        ));
        #[cfg(feature = "review-test")]
        crate::stage2_probe::checkpoint("before_window");
        let instance = GetModuleHandleW(null());
        let wc = WNDCLASSW {
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            ..std::mem::zeroed()
        };
        if RegisterClassW(&wc) == 0 {
            return Err("Window class registration failed".into());
        }
        dialog::register(instance)?;
        let state = App::new(config, path, preview);
        if !preview_mode {
            state
                .runtime
                .taskbar_message
                .set(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()));
        }
        let app = Box::new(Shared {
            runtime: state.runtime.clone(),
            app: RefCell::new(state),
        });
        let shared = &*app as *const Shared;
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_ACCEPTFILES,
            class.as_ptr(),
            wide("Orbit Launcher · Rust").as_ptr(),
            WS_POPUP | WS_CLIPCHILDREN,
            100,
            100,
            550,
            350,
            null_mut(),
            null_mut(),
            instance,
            shared.cast(),
        );
        if hwnd.is_null() {
            return Err("Window creation failed".into());
        }
        {
            let mut a = app.app.borrow_mut();
            a.hwnd = hwnd;
            a.shared = shared;
            a.snapshot = snapshot;
            a.runtime_gesture = !no_gesture;
            a.scale = GetDpiForWindow(hwnd) as f64 / 96.;
            #[cfg(feature = "review-test")]
            {
                a.scale = crate::stage3_probe::initial_dpi(GetDpiForWindow(hwnd)) as f64 / 96.;
            }
            a.relayout();
            let controls = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_WIN95_CLASSES,
            };
            InitCommonControlsEx(&controls);
            a.init_tooltip();
            DragAcceptFiles(hwnd, 1);
            SetTimer(hwnd, 1, 80, None);
            if !preview_mode {
                a.tray_added = a.tray(NIM_ADD);
                if !a.tray_added {
                    a.runtime.restore_tray.set(true);
                }
                if RegisterHotKey(
                    hwnd,
                    1,
                    MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
                    VK_SPACE as u32,
                ) == 0
                {
                    RegisterHotKey(
                        hwnd,
                        2,
                        MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
                        VK_SPACE as u32,
                    );
                }
            }
            if !hidden || preview_mode {
                a.reveal();
                if preview_mode {
                    a.pinned = true;
                }
            }
        }
        if !preview_mode
            && SetPropW(
                hwnd,
                wide(crate::instance::READY).as_ptr(),
                1usize as HANDLE,
            ) == 0
        {
            let mut a = app.app.borrow_mut();
            if a.tray_added {
                a.tray(NIM_DELETE);
            }
            UnregisterHotKey(hwnd, 1);
            UnregisterHotKey(hwnd, 2);
            KillTimer(hwnd, 1);
            DestroyWindow(hwnd);
            // DestroyWindow queued WM_QUIT; remove our own shutdown message so
            // main's error MessageBox is not cancelled before becoming visible.
            let mut quit: MSG = std::mem::zeroed();
            PeekMessageW(&mut quit, null_mut(), WM_QUIT, WM_QUIT, PM_REMOVE);
            return Err("Orbit window readiness could not be published. Startup cancelled. / 起動完了を通知できません。起動を中止します。".into());
        }
        let mut msg: MSG = std::mem::zeroed();
        loop {
            let result = GetMessageW(&mut msg, null_mut(), 0, 0);
            if result <= 0 {
                break;
            }
            let dialog = app.app.borrow().dialog;
            if dialog.is_null() || IsDialogMessageW(dialog, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        let mut a = app.app.borrow_mut();
        if a.tray_added {
            a.tray(NIM_DELETE);
        }
        UnregisterHotKey(hwnd, 1);
        UnregisterHotKey(hwnd, 2);
        KillTimer(hwnd, 1);
        if com {
            CoUninitialize();
        }
        Ok(())
    }
}
