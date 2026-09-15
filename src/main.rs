#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod config;
mod local;
mod model;
mod sources;
mod ui;
mod update;
mod wallpaper;

fn main() {
    ui::run();
}
