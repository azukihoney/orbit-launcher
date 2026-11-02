#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod diagnostics;
mod dialog;
#[cfg(feature = "review-test")]
mod dpi_follow_probe;
mod folder_color;
mod geometry;
mod gesture;
mod icon_only;
#[cfg(feature = "review-test")]
mod icon_only_probe;
mod icon_store;
mod identity;
mod instance;
#[cfg(test)]
mod layout_margin_tests;
mod model;
mod os;
mod outside_click;
#[cfg(feature = "review-test")]
mod outside_click_probe;
mod paint;
mod path_decisions;
mod runtime;
#[cfg(feature = "review-test")]
mod stage2_probe;
#[cfg(test)]
mod stage3_geometry_reference;
#[cfg(feature = "review-test")]
mod stage3_probe;
#[cfg(test)]
mod stage3_unit_controls;
mod wheel;
fn main() {
    let mut path = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("data/config.json");
    let mut hidden = false;
    let mut no_gesture = false;
    let mut preview = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                if let Some(value) = args.next() {
                    path = value.into();
                }
            }
            "--hidden" => hidden = true,
            "--no-gesture" => no_gesture = true,
            "--preview" => {
                if let Some(value) = args.next() {
                    preview = Some(std::path::PathBuf::from(value));
                }
            }
            _ => {
                os::message(std::ptr::null_mut(), "Unknown command-line option", true);
                return;
            }
        }
    }
    if !path.is_absolute() {
        path = std::env::current_dir().unwrap().join(path);
    }
    if let Err(error) = app::run(path, hidden, no_gesture, preview) {
        os::message(std::ptr::null_mut(), &error, true);
        std::process::exit(1);
    }
}
