use crate::{
    app::{App, Hit},
    geometry,
    model::{Config, Entry, Settings},
    paint::R,
};
use std::path::PathBuf;
use windows_sys::Win32::Foundation::RECT;

fn layout(pages: usize, height: i32) -> crate::app::Layout {
    let settings = Settings {
        app_rows: 2,
        app_columns: 6,
        folder_rows: 2,
        folder_columns: 6,
        ..Default::default()
    };
    geometry::calculate(
        &settings,
        pages,
        RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: height,
        },
        96,
    )
}

#[test]
fn one_page_layout_v3() {
    let l = layout(1, 1200);
    assert_eq!((l.width, l.height), (506, 319));
    assert_eq!(l.apps, R::new(12, 10, 482, 160));
    assert_eq!(l.divider, 173);
    assert_eq!(l.folders, R::new(12, 177, 426, 132));
    assert_eq!(l.controls, R::new(324, 10, 156, 78));
    assert_eq!(l.rail, 450);
}

#[test]
fn tabbed_layout_v3() {
    let l = layout(2, 1200);
    assert_eq!((l.width, l.height), (506, 353));
    assert_eq!(l.apps, R::new(12, 44, 482, 160));
    assert_eq!(l.divider, 207);
    assert_eq!(l.folders, R::new(12, 211, 426, 132));
    assert_eq!(l.controls, R::new(324, 44, 156, 78));
    assert_eq!(l.rail, 450);
    assert_eq!(
        R::new(12, 10, 106, 28),
        R::new(l.apps.x, l.apps.y - 34, 106, 28)
    );
    assert_eq!(
        R::new(122, 10, 106, 28),
        R::new(l.apps.x + 110, l.apps.y - 34, 106, 28)
    );
}

#[test]
fn short_work_area_v3() {
    let l = layout(1, 300);
    assert_eq!(300 - 32 - 23, 245);
    assert_eq!((l.width, l.height), (506, 306));
    assert_eq!(l.apps, R::new(12, 10, 482, 147));
    assert_eq!(l.divider, 160);
    assert_eq!(l.folders, R::new(12, 164, 426, 132));
}

#[test]
fn card_rectangles_and_gap_v3() {
    let l = layout(1, 1200);
    let app0 = R::new(l.apps.x, l.apps.y, 78, 78).inset(2);
    let app12 = R::new(l.apps.x + 2 * 78, l.apps.y + 78, 78, 78).inset(2);
    let folder0 = R::new(l.folders.x, l.folders.y, 60, 60).inset(2);
    assert_eq!(app0, R::new(14, 12, 74, 74));
    assert_eq!(app12, R::new(170, 90, 74, 74));
    assert_eq!(folder0, R::new(14, 179, 56, 56));
    assert!(app0.has(14, 12));
    assert!(!app0.has(88, 20));
    assert!(!R::new(l.apps.x + 78, l.apps.y, 78, 78).inset(2).has(89, 20));
}

#[test]
fn controls_and_rail_keep_relative_positions() {
    let l = layout(1, 1200);
    assert_eq!(l.controls.w, 2 * 78);
    assert_eq!(l.controls.h, 78);
    assert_eq!(l.rail, l.width - 12 - 44);
    assert_eq!(
        R::new(l.rail + 16, l.folders.y, 28, 28),
        R::new(466, 177, 28, 28)
    );
    assert_eq!(
        R::new(l.rail, l.folders.y + 34, 44, 44),
        R::new(450, 211, 44, 44)
    );
    assert_eq!(
        R::new(l.rail, l.folders.y + 84, 44, 44),
        R::new(450, 261, 44, 44)
    );
}

#[test]
fn place_preserves_window_size() {
    let l = layout(1, 1200);
    let r = geometry::place(
        &l,
        (960, 600),
        RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1200,
        },
        96,
    );
    assert_eq!((r.w, r.h), (l.width, l.height));
}

#[test]
fn hit_and_draw_share_card_bounds() {
    let mut config = Config::default();
    config.pages[0].items.clear();
    config.common.clear();
    let mut entry = Entry::new("Example", "example.exe", "launch");
    entry.cell = Some([0, 0]);
    config.pages[0].items.push(entry);
    let app = App::new(config, PathBuf::from("unused.json"), None);
    let card = R::new(app.layout.apps.x, app.layout.apps.y, 78, 78).inset(2);
    assert_eq!(card, R::new(14, 12, 74, 74));
    assert_eq!(app.hit(card.x, card.y, false), Hit::Entry(false, 0));
    assert_eq!(app.hit(87, 85, false), Hit::Entry(false, 0));
    for x in 88..92 {
        assert!(!matches!(app.hit(x, 20, false), Hit::Entry(..)));
    }
}

#[test]
fn tool_rectangles_keep_relative_offsets() {
    let mut config = Config::default();
    config.pages[0].items.clear();
    config.common.clear();
    let app = App::new(config, PathBuf::from("unused.json"), None);
    let rects: Vec<R> = app.tool_rects().into_iter().map(|(_, r)| r).collect();
    assert_eq!(
        rects,
        vec![
            R::new(351, 37, 28, 28),
            R::new(383, 37, 28, 28),
            R::new(415, 37, 28, 28),
            R::new(447, 37, 28, 28),
            R::new(466, 177, 28, 28),
            R::new(450, 211, 44, 44),
            R::new(450, 261, 44, 44),
        ]
    );
}

#[test]
fn tab_hit_uses_new_top_and_side_margin() {
    let mut config = Config::default();
    config.pages[0].items.clear();
    config.common.clear();
    let mut second = config.pages[0].clone();
    second.id = "second".into();
    config.pages.push(second);
    let app = App::new(config, PathBuf::from("unused.json"), None);
    assert_eq!(app.hit(12, 10, false), Hit::Page(0));
    assert_eq!(app.hit(122, 10, false), Hit::Page(1));
    assert_eq!(app.hit(119, 20, false), Hit::Nothing);
}

#[test]
fn controls_stay_right_aligned_whatever_the_column_counts() {
    // (app_columns, folder_columns): apps decide the width, folders decide it,
    // equal, and the envi configuration that exposed the gap.
    for (apps, folders) in [(6, 6), (4, 16), (12, 15), (12, 12), (16, 2)] {
        let settings = Settings {
            app_rows: 2,
            app_columns: apps,
            folder_rows: 2,
            folder_columns: folders,
            ..Default::default()
        };
        let l = geometry::calculate(
            &settings,
            1,
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1200,
            },
            96,
        );
        assert_eq!(
            l.width - (l.controls.x + l.controls.w),
            26,
            "apps={apps} folders={folders}"
        );
        assert_eq!(l.controls.w, 2 * 78);
    }
}

#[test]
fn app_decided_width_keeps_the_previous_control_position() {
    // Apps decide the width: the block must not move from its 0.8.5 position.
    for apps in [6usize, 8, 12, 16] {
        let settings = Settings {
            app_rows: 2,
            app_columns: apps,
            folder_rows: 2,
            folder_columns: 2,
            ..Default::default()
        };
        let l = geometry::calculate(
            &settings,
            1,
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1200,
            },
            96,
        );
        assert_eq!(l.controls.x, 12 + (apps as i32 - 2) * 78, "apps={apps}");
    }
}

#[test]
fn right_aligned_tools_are_clickable_when_folders_decide_the_width() {
    let mut config = Config::default();
    config.pages[0].items.clear();
    config.common.clear();
    config.settings.app_columns = 12;
    config.settings.folder_columns = 15;
    let app = App::new(config, PathBuf::from("unused.json"), None);
    let rects = app.tool_rects();
    let close = rects.iter().find(|(id, _)| *id == 3).unwrap().1;
    assert_eq!(close.x + close.w, app.layout.width - 31);
    assert_eq!(
        app.hit(close.x + close.w / 2, close.y + close.h / 2, false),
        Hit::Tool(3)
    );
}

fn work(width: i32) -> RECT {
    RECT {
        left: 100,
        top: 0,
        right: 100 + width,
        bottom: 2000,
    }
}

fn scaled(ui_scale: &str, folder_columns: usize) -> Settings {
    Settings {
        ui_scale: ui_scale.into(),
        app_columns: 12,
        folder_columns,
        ..Default::default()
    }
}

#[test]
fn auto_ui_scale_returns_the_display_dpi_even_when_too_wide() {
    for dpi in [96, 120, 144, 192, 288] {
        assert_eq!(geometry::ui_dpi(&scaled("auto", 15), work(300), dpi), dpi);
    }
}

#[test]
fn fixed_ui_scale_ignores_the_display_dpi() {
    for (scale, wanted) in [("75", 72), ("100", 96), ("125", 120), ("150", 144)] {
        for dpi in [96, 120, 144, 192] {
            assert_eq!(
                geometry::ui_dpi(&scaled(scale, 15), work(3840), dpi),
                wanted,
                "{scale} at {dpi}"
            );
        }
    }
}

#[test]
fn fixed_ui_scale_shrinks_only_to_fit_the_work_area() {
    // 15 folder columns make the window 994 DIP wide.
    let s = scaled("200", 15);
    assert_eq!(geometry::ui_dpi(&s, work(1988), 96), 192);
    assert_eq!(geometry::ui_dpi(&s, work(1987), 96), 191);
    let dpi = geometry::ui_dpi(&s, work(1920), 96);
    assert_eq!(dpi, 185);
    let l = geometry::calculate(&s, 1, work(1920), dpi);
    let size = R::new(0, 0, l.width, l.height).scale(dpi as f64 / 96.);
    assert!(size.w <= 1920, "{}", size.w);
}

#[test]
fn fixed_ui_scale_has_a_lower_limit_and_survives_a_bad_work_area() {
    let s = scaled("100", 15);
    assert_eq!(geometry::ui_dpi(&s, work(100), 96), geometry::MIN_FIXED_DPI);
    assert_eq!(geometry::ui_dpi(&s, work(0), 96), geometry::MIN_FIXED_DPI);
    assert_eq!(geometry::ui_dpi(&s, work(-50), 96), geometry::MIN_FIXED_DPI);
}

#[test]
fn window_width_is_unchanged_by_the_extraction() {
    let l = geometry::calculate(&scaled("auto", 15), 1, work(3840), 96);
    assert_eq!(l.width, 994);
    let l = geometry::calculate(&scaled("auto", 2), 1, work(3840), 96);
    assert_eq!(l.width, 12 * 78 + 14 + 24);
}
