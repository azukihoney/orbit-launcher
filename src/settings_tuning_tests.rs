use super::*;

#[test] // Kills a stale 2.5 default, existing-value rewriting, and new lower/upper bounds.
fn tuning_new_defaults_bounds_and_existing_values() {
    assert_eq!(Config::default().settings.timeout, 1.);
    for min in [5., 100., 200., 400.] {
        for timeout in [0.3, 1., 2.5, 6.] {
            let mut c = Config::default();
            c.settings.min_radius = min;
            c.settings.timeout = timeout;
            c.settings.font_family = "x".repeat(100);
            c.normalize().unwrap();
            let mut copy: Config =
                serde_json::from_value(serde_json::to_value(&c).unwrap()).unwrap();
            copy.normalize().unwrap();
            assert_eq!(copy.settings.timeout, timeout);
            assert_eq!(copy.settings.font_family, c.settings.font_family);
        }
    }
}

#[test] // Kills the generic message, wrong item/range, and unstable first-error ordering.
fn tuning_each_field_reports_its_label_and_range() {
    let cases = [
        (
            "app_rows",
            serde_json::json!(0),
            "App rows",
            "アプリの行数",
            "1",
            "16",
        ),
        (
            "app_columns",
            serde_json::json!(1),
            "App columns",
            "アプリの列数",
            "2",
            "32",
        ),
        (
            "folder_rows",
            serde_json::json!(0),
            "Folder rows",
            "フォルダの行数",
            "1",
            "16",
        ),
        (
            "folder_columns",
            serde_json::json!(0),
            "Folder columns",
            "フォルダの列数",
            "1",
            "32",
        ),
        (
            "app_font_px",
            serde_json::json!(7),
            "App font size (px)",
            "アプリ文字サイズ（px）",
            "8",
            "16",
        ),
        (
            "folder_font_px",
            serde_json::json!(17),
            "Folder font size (px)",
            "フォルダ文字サイズ（px）",
            "8",
            "16",
        ),
        (
            "language",
            serde_json::json!("xx"),
            "Language",
            "表示言語",
            "ja",
            "en",
        ),
        (
            "theme",
            serde_json::json!("xx"),
            "Theme",
            "テーマ",
            "light",
            "dark",
        ),
        (
            "direction",
            serde_json::json!("xx"),
            "Direction",
            "回転方向",
            "ccw",
            "both",
        ),
        (
            "font_family",
            serde_json::json!(" "),
            "Font",
            "フォント",
            "1",
            "128",
        ),
        (
            "turns",
            serde_json::json!(1.49),
            "Number of turns",
            "円を描く回数",
            "1.5",
            "4",
        ),
        (
            "timeout",
            serde_json::json!(0.299),
            "Time limit (seconds)",
            "制限時間（秒）",
            "0.3",
            "6",
        ),
        (
            "min_radius",
            serde_json::json!(400.001),
            "Minimum radius (screen px)",
            "最小半径（画面px）",
            "5",
            "400",
        ),
        (
            "max_radius",
            serde_json::json!(1500.001),
            "Maximum radius (screen px)",
            "最大半径（画面px）",
            "30",
            "1500",
        ),
    ];
    for (key, value, en, ja, low, high) in &cases {
        let mut v = serde_json::to_value(Config::default()).unwrap();
        v["settings"][key] = value.clone();
        let error = serde_json::from_value::<Config>(v)
            .unwrap()
            .normalize()
            .unwrap_err();
        for fragment in [*en, *ja, *low, *high, " / "] {
            assert!(error.contains(fragment), "{key}: {error}");
        }
    }
    let mut v = serde_json::to_value(Config::default()).unwrap();
    for (key, value, ..) in &cases {
        v["settings"][key] = value.clone();
    }
    let error = serde_json::from_value::<Config>(v)
        .unwrap()
        .normalize()
        .unwrap_err();
    assert!(error.starts_with("App rows"));
}

#[test] // Kills non-finite acceptance, off-by-one range checks, and missing min<max.
fn tuning_nonfinite_and_radius_relation() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for which in 0..4 {
            let mut c = Config::default();
            let (field, name) = match which {
                0 => (&mut c.settings.turns, "Number of turns"),
                1 => (&mut c.settings.timeout, "Time limit (seconds)"),
                2 => (&mut c.settings.min_radius, "Minimum radius (screen px)"),
                _ => (&mut c.settings.max_radius, "Maximum radius (screen px)"),
            };
            *field = value;
            assert!(c.normalize().unwrap_err().contains(name));
        }
    }
    for (min, max) in [(100., 100.), (200., 100.)] {
        let mut c = Config::default();
        c.settings.min_radius = min;
        c.settings.max_radius = max;
        let error = c.normalize().unwrap_err();
        assert!(error.contains("must be less than") && error.contains("未満"));
    }
    for min in [4.999, 400.001] {
        let mut c = Config::default();
        c.settings.min_radius = min;
        assert!(c.normalize().is_err());
    }
    for timeout in [0.299999, 6.000001] {
        let mut c = Config::default();
        c.settings.timeout = timeout;
        assert!(c.normalize().is_err());
    }
}
