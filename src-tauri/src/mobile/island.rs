//! The Dynamic Island is a desktop window; phones have the media notification and lock screen
//! instead. Same API as the desktop `island.rs`, doing nothing.

use tauri::AppHandle;

pub fn set_click_through(_on: bool) {}

pub fn set_pill(_rect: [i32; 4]) {}

pub fn open(_app: &AppHandle) {}

pub fn close(_app: &AppHandle) {}

pub fn set_bounds(_app: &AppHandle, _x: i32, _y: i32, _w: u32, _h: u32) {}

pub fn set_panel(_open: bool) {}

pub fn main_away(_app: &AppHandle) -> bool {
    false
}

pub fn main_changed(_app: &AppHandle) {}
