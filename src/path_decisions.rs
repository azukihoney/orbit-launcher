//! Decisions kept separate from Windows calls so the edge cases can be tested.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowseStart {
    pub folder: String,
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Existing {
    File,
    Folder,
    Missing,
}

pub fn drive_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\'
}

pub fn unc(path: &str) -> bool {
    path.starts_with("\\\\") || path.starts_with("//")
}

/// Decide before any shortcut I/O, including an environment-expanded UNC path.
pub fn may_read_link(kind: &str, expanded_path: &str) -> bool {
    kind == "launch" && expanded_path.to_ascii_lowercase().ends_with(".lnk") && !unc(expanded_path)
}

/// Use the edit control's current text, not the stored entry.
pub fn browse_field_value(value: &str, expand_value: impl FnOnce(&str) -> String) -> String {
    expand_value(value.trim())
}

fn usable_path(path: &str) -> bool {
    !path.contains('\0') && !path.contains('"') && !path.contains('*') && !path.contains('?')
}

pub fn location_eligible(path: &str) -> bool {
    (drive_absolute(path) || unc(path)) && usable_path(path)
}

pub fn browse_start(path: &str, existing: Existing, parent_exists: bool) -> Option<BrowseStart> {
    let path = path.trim();
    if !drive_absolute(path) || !usable_path(path) {
        return None;
    }
    let normalized = path.trim_end_matches('\\');
    let normalized = if normalized.len() == 2 {
        path
    } else {
        normalized
    };
    match existing {
        Existing::Folder => Some(BrowseStart {
            folder: normalized.into(),
            file_name: None,
        }),
        Existing::File | Existing::Missing if parent_exists => {
            let split = normalized.rfind('\\')?;
            let parent = if split == 2 {
                &normalized[..3]
            } else {
                &normalized[..split]
            };
            Some(BrowseStart {
                folder: parent.into(),
                file_name: (existing == Existing::File).then(|| normalized[split + 1..].into()),
            })
        }
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    Disabled,
    Missing,
    Select { folder: String, item: String },
    Open { folder: String },
}

pub fn location(path: &str, kind: &str, existing: Existing) -> Location {
    if !location_eligible(path) {
        return Location::Disabled;
    }
    if existing == Existing::Missing || kind == "folder" && existing != Existing::Folder {
        return Location::Missing;
    }
    if path.len() == 3 && drive_absolute(path) {
        return Location::Open {
            folder: path.into(),
        };
    }
    let normalized = if path.starts_with("//") {
        path.replace('/', "\\")
    } else {
        path.into()
    };
    let trimmed = normalized.trim_end_matches('\\');
    let split = match trimmed.rfind('\\') {
        Some(value) => value,
        None => return Location::Missing,
    };
    // A drive root has no selectable parent. Open it directly.
    let folder = if split == 2 {
        &trimmed[..3]
    } else {
        &trimmed[..split]
    };
    Location::Select {
        folder: folder.into(),
        item: trimmed.into(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconSource {
    Explicit,
    LinkTarget,
    Original,
}

pub fn icon_source(
    kind: &str,
    path: &str,
    icon: Option<(&str, bool)>,
    target: Option<(&str, bool)>,
) -> IconSource {
    if kind != "launch" || !path.to_ascii_lowercase().ends_with(".lnk") || unc(path) {
        return IconSource::Original;
    }
    if icon.is_some_and(|(p, _)| unc(p)) || target.is_some_and(|(p, _)| unc(p)) {
        return IconSource::Original;
    }
    if icon.is_some_and(|(p, exists)| !p.is_empty() && exists) {
        IconSource::Explicit
    } else if target.is_some_and(|(p, exists)| !p.is_empty() && exists) {
        IconSource::LinkTarget
    } else {
        IconSource::Original
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browse_rows_and_edges() {
        let system_root = crate::model::expand("%SystemRoot%");
        assert_eq!(
            browse_start(
                &crate::model::expand(" %SystemRoot%\\System32 "),
                Existing::Folder,
                true
            ),
            Some(BrowseStart {
                folder: format!(r"{}\System32", system_root),
                file_name: None
            })
        );
        assert_eq!(
            browse_start(" C:\\a\\x.lnk ", Existing::File, true),
            Some(BrowseStart {
                folder: "C:\\a".into(),
                file_name: Some("x.lnk".into())
            })
        );
        assert_eq!(
            browse_start("C:\\a\\", Existing::Folder, true),
            Some(BrowseStart {
                folder: "C:\\a".into(),
                file_name: None
            })
        );
        assert_eq!(
            browse_start("C:\\a\\missing", Existing::Missing, true),
            Some(BrowseStart {
                folder: "C:\\a".into(),
                file_name: None
            })
        );
        assert_eq!(
            browse_start("C:\\", Existing::Folder, true).unwrap().folder,
            "C:\\"
        );
        for p in [
            "",
            "relative",
            "shell:AppsFolder",
            "https://example.com",
            "\\\\server\\share",
            "C:\\bad?name",
        ] {
            assert_eq!(browse_start(p, Existing::Missing, false), None);
        }
    }
    #[test]
    fn location_rows() {
        assert_eq!(
            location("C:\\a\\x.lnk", "launch", Existing::File),
            Location::Select {
                folder: "C:\\a".into(),
                item: "C:\\a\\x.lnk".into()
            }
        );
        assert_eq!(
            location("C:\\a", "folder", Existing::Folder),
            Location::Select {
                folder: "C:\\".into(),
                item: "C:\\a".into()
            }
        );
        assert_eq!(
            location("C:\\a", "launch", Existing::Folder),
            Location::Select {
                folder: "C:\\".into(),
                item: "C:\\a".into()
            }
        );
        assert_eq!(
            location("C:\\", "folder", Existing::Folder),
            Location::Open {
                folder: "C:\\".into()
            }
        );
        assert_eq!(
            location("C:\\a", "launch", Existing::Missing),
            Location::Missing
        );
        for p in ["notepad.exe", "shell:AppsFolder", "https://example.com"] {
            assert_eq!(location(p, "launch", Existing::Missing), Location::Disabled);
            assert!(!location_eligible(p));
        }
        assert!(!location_eligible("C:\\bad?name"));
        assert_eq!(
            location("\\\\server\\share\\x", "launch", Existing::File),
            Location::Select {
                folder: "\\\\server\\share".into(),
                item: "\\\\server\\share\\x".into()
            }
        );
        assert_eq!(
            location("\\\\server\\share\\missing", "launch", Existing::Missing),
            Location::Missing
        );
        assert_eq!(
            location("//server/share/x", "launch", Existing::File),
            Location::Select {
                folder: "\\\\server\\share".into(),
                item: "\\\\server\\share\\x".into()
            }
        );
    }
    #[test]
    fn icon_order_and_controls() {
        assert_eq!(
            icon_source(
                "launch",
                "C:\\x.lnk",
                Some(("C:\\i.ico", true)),
                Some(("C:\\x.exe", true))
            ),
            IconSource::Explicit
        );
        assert_eq!(
            icon_source("launch", "C:\\x.LNK", None, Some(("C:\\x.exe", true))),
            IconSource::LinkTarget
        );
        assert_eq!(
            icon_source(
                "launch",
                "C:\\x.lnk",
                Some(("C:\\missing.ico", false)),
                Some(("C:\\x.exe", true))
            ),
            IconSource::LinkTarget
        );
        assert_eq!(
            icon_source("launch", "C:\\x.lnk", None, None),
            IconSource::Original
        );
        for (kind, path) in [
            ("folder", "C:\\x.lnk"),
            ("launch", "C:\\x.exe"),
            ("launch", "\\\\server\\x.lnk"),
        ] {
            assert_eq!(icon_source(kind, path, None, None), IconSource::Original);
        }
        assert_eq!(
            icon_source(
                "launch",
                "C:\\x.lnk",
                Some(("\\\\server\\i.ico", true)),
                None
            ),
            IconSource::Original
        );
    }

    #[test]
    fn r2_link_read_guard_and_browse_edges() {
        for (kind, path, expected) in [
            ("launch", "C:\\a.LNK", true),
            ("launch", "\\\\server\\a.lnk", false),
            ("launch", "//server/a.lnk", false),
            ("folder", "C:\\a.lnk", false),
            ("launch", "C:\\a.exe", false),
        ] {
            assert_eq!(may_read_link(kind, path), expected, "{kind} {path}");
        }
        assert!(!may_read_link(
            "launch",
            &browse_field_value(" %R2_UNC% ", |_| "\\\\server\\a.lnk".into())
        ));
        assert_eq!(
            browse_field_value("  %TEMP%\\a  ", |s| s.replace("%TEMP%", "C:\\tmp")),
            "C:\\tmp\\a"
        );
        for path in ["C:foo", "C:\\bad\"name", "C:\\orphan\\x"] {
            assert_eq!(browse_start(path, Existing::Missing, false), None);
        }
        assert_eq!(browse_start("C:foo", Existing::Folder, true), None);
        assert_eq!(browse_start("C:\\bad\"name", Existing::Folder, true), None);
        assert_eq!(
            location("C:\\a.txt", "folder", Existing::File),
            Location::Missing
        );
        assert_eq!(
            icon_source("folder", "C:\\a.lnk", Some(("C:\\a.ico", true)), None),
            IconSource::Original
        );
    }
}
