// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Fix: NVIDIA + Wayland explicit-sync crash (Error 71)
    // Injected before any GTK/GDK/WebKitGTK call so it is picked up at process start.
    std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1");

    dotenvy::dotenv().ok();
    app_lib::run();
}
