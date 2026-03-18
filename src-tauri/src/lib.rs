use std::str::FromStr;
use std::time::Duration;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
// 🕵️ Game detector import
use sysinfo::System;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! SimNation Desktop is ready.", name)
}

// --- OVERLAY LOGIC ---
fn toggle_overlay_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("overlay") {
        let is_visible = window.is_visible().unwrap_or(false);
        
        if is_visible {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
            let _ = window.set_always_on_top(true);
        }
    }
}

#[tauri::command]
async fn hide_overlay(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("overlay") {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

// --- HOTKEY UPDATER ---
#[tauri::command]
fn update_hotkeys(
    app: AppHandle,
    old_play: String, new_play: String,
    old_stop: String, new_stop: String,
    old_overlay: String, new_overlay: String,
) -> Result<(), String> {
    let manager = app.global_shortcut();

    let unreg = |key: &String| {
        if !key.is_empty() {
            if let Ok(s) = Shortcut::from_str(key) { let _ = manager.unregister(s); }
        }
    };

    unreg(&old_play); unreg(&old_stop); unreg(&old_overlay);

    if let Ok(new) = Shortcut::from_str(&new_play) {
        let _ = manager.on_shortcut(new, move |app_handle, _, event| { 
            if event.state == ShortcutState::Pressed { let _ = app_handle.emit("media-play", ()); }
        });
    }
    if let Ok(new) = Shortcut::from_str(&new_stop) {
        let _ = manager.on_shortcut(new, move |app_handle, _, event| { 
            if event.state == ShortcutState::Pressed { let _ = app_handle.emit("media-stop", ()); }
        });
    }
    if let Ok(new) = Shortcut::from_str(&new_overlay) {
        let _ = manager.on_shortcut(new, move |app_handle, _, event| { 
            if event.state == ShortcutState::Pressed { toggle_overlay_window(&app_handle); }
        });
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // ✅ FIXED: V2 uses the Builder pattern for the updater
        .plugin(tauri_plugin_updater::Builder::new().build()) 
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--silent"])))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(|app| {
            // --- 🚛 THE GAME DETECTOR LOOP ---
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut sys = System::new_all();
                let games = ["eurotrucks2.exe", "amtrucks.exe", "eurotrucks2", "amtrucks"];
                let mut was_running = false;

                loop {
                    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
                    
                    let is_running = sys.processes().iter().any(|(_, process)| {
                        let name = process.name().to_string_lossy().to_lowercase();
                        games.iter().any(|&game| name.contains(game))
                    });

                    if is_running && !was_running {
                        if let Some(window) = handle.get_webview_window("overlay") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_always_on_top(true);
                        }
                        if let Some(main_window) = handle.get_webview_window("main") {
                            let _ = main_window.hide();
                        }
                        was_running = true;
                    } else if !is_running && was_running {
                        if let Some(window) = handle.get_webview_window("overlay") {
                            let _ = window.hide();
                        }
                        was_running = false;
                    }
                    std::thread::sleep(Duration::from_secs(5));
                }
            });

            // --- TRAY & MENU ---
            let show_i = MenuItem::with_id(app, "show", "Open SimNation", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit Radio", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") { let _ = w.show(); let _ = w.set_focus(); }
                    }
                    "quit" => { std::process::exit(0); }
                    _ => { }
                })
                .on_tray_icon_event(|tray, event| match event {
                    TrayIconEvent::Click { button: MouseButton::Left, .. } => {
                        if let Some(w) = tray.app_handle().get_webview_window("main") { let _ = w.show(); let _ = w.set_focus(); }
                    }
                    _ => {}
                })
                .build(app)?;

            let manager = app.global_shortcut();
            
            if let Ok(k) = Shortcut::from_str("MediaPlayPause") {
                let _ = manager.on_shortcut(k, move |app_handle, _, event| { 
                    if event.state == ShortcutState::Pressed { let _ = app_handle.emit("media-hardware-toggle", ()); }
                });
            }
            if let Ok(k) = Shortcut::from_str("MediaStop") {
                let _ = manager.on_shortcut(k, move |app_handle, _, event| { 
                    if event.state == ShortcutState::Pressed { let _ = app_handle.emit("media-stop", ()); }
                });
            }

            let args: Vec<String> = std::env::args().collect();
            if args.contains(&"--silent".to_string()) {
                if let Some(w) = app.get_webview_window("main") { let _ = w.hide(); }
            }

            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                if window.label() == "main" { let _ = window.hide(); api.prevent_close(); }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            greet, 
            update_hotkeys, 
            hide_overlay
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}