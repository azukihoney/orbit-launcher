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
