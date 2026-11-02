//! The same tests run in the frozen stage2 copy before any production changes.
use crate::{identity, model::Snapshot, paint::font_face};
#[test]
fn font_boundaries_are_pure_and_keep_complete_utf16() {
    for name in [
        "",
        "UDEV Gothic",
        &"W".repeat(31),
        &"W".repeat(32),
        &format!("{}😀", "A".repeat(30)),
    ] {
        let face = font_face(name);
        let n = face.iter().position(|&v| v == 0).unwrap();
        assert!(n <= 31);
        assert!(n == 0 || !(0xd800..=0xdbff).contains(&face[n - 1]));
    }
    assert_eq!(font_face("a\0b")[..3], [97, 0, 0]);
    assert_eq!(
        &font_face(&format!("{}😀", "A".repeat(29)))[29..],
        [0xd83d, 0xde00, 0]
    );
}
#[test]
fn fingerprints_require_digest_and_existence_not_just_length() {
    let a = Snapshot::Present {
        length: 3,
        sha256: [1; 32],
    };
    assert_eq!(a, a.clone());
    assert_ne!(
        a,
        Snapshot::Present {
            length: 3,
            sha256: [2; 32]
        }
    );
    assert_ne!(
        a,
        Snapshot::Present {
            length: 4,
            sha256: [1; 32]
        }
    );
    assert_ne!(
        Snapshot::Missing,
        Snapshot::Present {
            length: 0,
            sha256: [0; 32]
        }
    );
}
#[test]
fn lexical_path_normalization_is_pure() {
    use std::path::Path;
    assert_eq!(
        identity::unprefixed(Path::new(r"\\?\C:\sample\config.json")),
        r"C:\sample\config.json"
    );
    assert_eq!(
        identity::unprefixed(Path::new(r"\\?\UNC\server\share\config.json")),
        r"\\server\share\config.json"
    );
    assert_eq!(
        identity::unprefixed(Path::new("C:/sample/設定.json")),
        r"C:\sample\設定.json"
    );
}

fn layout_json(l: &crate::app::Layout, dpi: u32) -> serde_json::Value {
    fn r(v: crate::paint::R) -> serde_json::Value {
        serde_json::json!([v.x, v.y, v.w, v.h])
    }
    serde_json::json!({"width":l.width,"height":l.height,"apps":r(l.apps),"folders":r(l.folders),
        "divider":l.divider,"controls":r(l.controls),"rail":l.rail,"scale":dpi as f64/96.})
}
fn fixtures() -> serde_json::Value {
    serde_json::from_str(include_str!("stage3_layout_vectors.json")).unwrap()
}
fn input(
    v: &serde_json::Value,
) -> (
    crate::model::Settings,
    usize,
    windows_sys::Win32::Foundation::RECT,
    u32,
) {
    let f = &v["fixture"];
    let a = f["window_work"].as_array().unwrap();
    let mut s = crate::model::Settings {
        app_columns: 6,
        folder_columns: 6,
        ..Default::default()
    };
    if f["rows"].as_i64().unwrap_or(0) > 0 {
        s.app_rows = 16;
        s.folder_rows = 16;
    }
    (
        s,
        f["pages"].as_u64().unwrap_or(1) as usize,
        windows_sys::Win32::Foundation::RECT {
            left: a[0].as_i64().unwrap() as i32,
            top: a[1].as_i64().unwrap() as i32,
            right: a[2].as_i64().unwrap() as i32,
            bottom: a[3].as_i64().unwrap() as i32,
        },
        f["initial_dpi"].as_u64().unwrap() as u32,
    )
}
#[test]
fn layouts_match_the_actual_stage2_relayout() {
    for v in fixtures().as_array().unwrap() {
        let (s, pages, work, dpi) = input(v);
        let actual = crate::stage3_geometry_reference::calculate(&s, pages, work, dpi);
        assert_eq!(
            layout_json(&actual, dpi),
            v["expected_layout"],
            "{}",
            v["name"]
        );
    }
}
#[test]
fn placement_rectangles_use_rendered_size() {
    for v in fixtures()
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| !v["expected_rect"].is_null())
    {
        let (s, pages, work, dpi) = input(v);
        let cursor = &v["fixture"]["cursor"];
        let layout = crate::stage3_geometry_reference::calculate(&s, pages, work, dpi);
        let actual = crate::stage3_geometry_reference::place(
            &layout,
            (
                cursor[0].as_i64().unwrap() as i32,
                cursor[1].as_i64().unwrap() as i32,
            ),
            work,
            dpi,
        );
        assert_eq!(
            serde_json::json!([actual.x, actual.y, actual.w, actual.h]),
            v["expected_rect"],
            "{}",
            v["name"]
        );
    }
}
