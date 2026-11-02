//! Keep the raw JSON value in Entry.extra for forward and backward compatibility.
use crate::model::Entry;

pub struct Color {
    pub name: &'static str,
    pub ja: &'static str,
    pub en: &'static str,
    pub light: (u32, u32),
    pub dark: (u32, u32),
}

pub const COLORS: [Color; 6] = [
    Color {
        name: "red",
        ja: "赤",
        en: "Red",
        light: (0xfffdecec, 0xfffbe3e3),
        dark: (0xff4a2e33, 0xff42282d),
    },
    Color {
        name: "orange",
        ja: "橙",
        en: "Orange",
        light: (0xfffdf1e4, 0xfffbe9d6),
        dark: (0xff4a3a2a, 0xff423324),
    },
    Color {
        name: "yellow",
        ja: "黄",
        en: "Yellow",
        light: (0xfffdf8dc, 0xfffaf2c8),
        dark: (0xff47432a, 0xff3f3b24),
    },
    Color {
        name: "green",
        ja: "緑",
        en: "Green",
        light: (0xffe9f6ec, 0xffdff1e3),
        dark: (0xff2d4236, 0xff283b30),
    },
    Color {
        name: "blue",
        ja: "青",
        en: "Blue",
        light: (0xffe8f1fc, 0xffdde9fa),
        dark: (0xff2b3a50, 0xff263447),
    },
    Color {
        name: "purple",
        ja: "紫",
        en: "Purple",
        light: (0xfff1eafb, 0xffe9dff8),
        dark: (0xff3c3150, 0xff352b47),
    },
];

pub fn selected(entry: &Entry) -> Option<usize> {
    let name = entry.extra.get("color")?.as_str()?;
    COLORS.iter().position(|color| color.name == name)
}
