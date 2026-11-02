use crate::{app::Layout, model::Settings, paint::R};
use windows_sys::Win32::Foundation::RECT;
pub const APP_STEP: i32 = 78;
pub const FOLDER_STEP: i32 = 60;
pub const CARD_INSET: i32 = 2;
pub const SIDE_MARGIN: i32 = 12;
pub const TOP_MARGIN: i32 = 10;
pub const BOTTOM_MARGIN: i32 = 10;
pub const DIVIDER_GAP: i32 = 3;
pub const FOLDER_GAP: i32 = 4;
const GRID_SLACK: i32 = 14;
/// Smallest DPI a fixed UI scale may shrink to while fitting the work area.
pub const MIN_FIXED_DPI: u32 = 48;
pub const HEIGHT_BUDGET_DEDUCTION: i32 = TOP_MARGIN + DIVIDER_GAP + FOLDER_GAP + BOTTOM_MARGIN - 4;
pub fn step(common: bool) -> i32 {
    if common { FOLDER_STEP } else { APP_STEP }
}
pub fn wanted(rows: usize, common: bool) -> i32 {
    rows as i32 * step(common) + 4
}
pub fn card(cell: R) -> R {
    cell.inset(CARD_INSET)
}
fn window_width(s: &Settings) -> i32 {
    (s.app_columns as i32 * APP_STEP + GRID_SLACK)
        .max(s.folder_columns as i32 * FOLDER_STEP + GRID_SLACK + 56)
        + 2 * SIDE_MARGIN
}
/// DPI the UI is drawn at. `auto` returns the display's DPI untouched; a fixed
/// scale ignores it, shrinking only as far as needed to fit the work area.
pub fn ui_dpi(s: &Settings, work: RECT, display_dpi: u32) -> u32 {
    let Some(percent) = s.ui_scale_percent() else {
        return display_dpi;
    };
    let wanted = percent * 96 / 100;
    let available = (work.right - work.left).max(0) as i64;
    let fit = available * 96 / window_width(s) as i64;
    wanted
        .min(fit.min(u32::MAX as i64) as u32)
        .max(MIN_FIXED_DPI)
}
pub fn calculate(s: &Settings, pages: usize, work: RECT, dpi: u32) -> Layout {
    let scale = dpi as f64 / 96.;
    let max_height = ((work.bottom - work.top) as f64 / scale) as i32 - 32;
    let tabs = if pages > 1 { 34 } else { 0 };
    let wanted_app = wanted(s.app_rows, false);
    let wanted_folder = wanted(s.folder_rows, true);
    let budget = (max_height - HEIGHT_BUDGET_DEDUCTION - tabs).max(220);
    let (app_h, folder_h) = if wanted_app + wanted_folder.max(132) > budget {
        let app_h = (budget * 6 / 10).max(90);
        (app_h, (budget - app_h).max(132))
    } else {
        (wanted_app, wanted_folder.max(132))
    };
    let width = window_width(s);
    let app_y = TOP_MARGIN + tabs;
    let divider = app_y + app_h + DIVIDER_GAP;
    let folder_y = divider + FOLDER_GAP;
    Layout {
        width,
        height: folder_y + folder_h + BOTTOM_MARGIN,
        apps: R::new(SIDE_MARGIN, app_y, width - 2 * SIDE_MARGIN, app_h),
        folders: R::new(
            SIDE_MARGIN,
            folder_y,
            width - 2 * SIDE_MARGIN - 56,
            folder_h,
        ),
        divider,
        // Anchored to the window, not the app grid: folders can decide the width.
        controls: R::new(
            width - SIDE_MARGIN - GRID_SLACK - 2 * APP_STEP,
            app_y,
            2 * APP_STEP,
            APP_STEP,
        ),
        rail: width - SIDE_MARGIN - 44,
    }
}
pub fn place(layout: &Layout, cursor: (i32, i32), work: RECT, dpi: u32) -> R {
    let cursor = windows_sys::Win32::Foundation::POINT {
        x: cursor.0,
        y: cursor.1,
    };
    let scale = dpi as f64 / 96.;
    // Match the actual rendered dimensions (R::scale rounds), including
    // half-pixel cases such as 506 DIP at 125% = 633 physical pixels.
    let size = R::new(0, 0, layout.width, layout.height).scale(scale);
    let w = size.w;
    let h = size.h;
    let x = (cursor.x as i64 - w as i64 / 2)
        .min(work.right as i64 - w as i64)
        .max(work.left as i64) as i32;
    let y = (cursor.y as i64 - (50. * scale) as i64)
        .min(work.bottom as i64 - h as i64)
        .max(work.top as i64) as i32;

    R::new(x, y, w, h)
}
