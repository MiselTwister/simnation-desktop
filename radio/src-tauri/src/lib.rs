use std::str::FromStr;
use std::time::Duration;
use std::sync::{Arc, Mutex};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use sysinfo::System;

// --- 🧠 APP STATE (Synced with Hub logic) ---
struct AppState {
    is_mock_active: bool,
}

// --- 🖥️ WINDOW LOGIC ---
fn toggle_overlay_window(app: &AppHandle) {
    // 🚨 UPDATED: Using "radio_overlay" to match your telemetry config
    if let Some(window) = app.get_webview_window("radio_overlay") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_always_on_top(true);
            let _ = window.set_focus(); 
        }
    }
}

#[tauri::command]
async fn hide_overlay(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("radio_overlay") {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

// --- 🎮 TELEMETRY COMMANDS ---
#[tauri::command]
fn toggle_telemetry_mock(state: tauri::State<'_, Arc<Mutex<AppState>>>) -> bool {
    let mut s = state.lock().unwrap();
    s.is_mock_active = !s.is_mock_active;
    s.is_mock_active
}

#[tauri::command]
fn install_telemetry_plugin() -> Result<String, String> {
    Ok("SCS Telemetry is active via SimNation Hub.".to_string())
}

// --- ⌨️ HOTKEY UPDATER ---
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

// --- 🚀 MOCK DATA LOOP ---
fn start_mock_loop(handle: AppHandle, state: Arc<Mutex<AppState>>) {
    std::thread::spawn(move || {
        let mut counter: f32 = 0.0;
        loop {
            let is_mock = state.lock().unwrap().is_mock_active;
            if is_mock {
                counter += 0.1;
                let mock_speed = (counter.sin() * 30.0) + 55.0;
                let _ = handle.emit("telemetry-update", serde_json::json!({
                    "speed": mock_speed,
                    "limit": 80,
                    "gear": 10,
                    "fuel": 75,
                    "temp": 85,
                    "damage": 1
                }));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = Arc::new(Mutex::new(AppState { is_mock_active: false }));

    tauri::Builder::default()
        .manage(app_state.clone()) 
        .plugin(tauri_plugin_updater::Builder::new().build()) 
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--silent"])))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(move |app| {
            let handle = app.handle().clone();
            
            // Start the mock listener loop
            start_mock_loop(handle.clone(), app_state.clone());

            // 🔍 GAME DETECTION LOOP
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
                        if let Some(w) = handle.get_webview_window("radio_overlay") {
                            let _ = w.show(); let _ = w.unminimize(); let _ = w.set_always_on_top(true);
                        }
                        if let Some(main) = handle.get_webview_window("main") { let _ = main.hide(); }
                        was_running = true;
                    } else if !is_running && was_running {
                        if let Some(w) = handle.get_webview_window("radio_overlay") { let _ = w.hide(); }
                        if let Some(main) = handle.get_webview_window("main") { let _ = main.show(); let _ = main.unminimize(); let _ = main.set_focus(); }
                        was_running = false;
                    }
                    std::thread::sleep(Duration::from_secs(5));
                }
            });

            // 🖱️ TRAY SETUP
            let show_i = MenuItem::with_id(app, "show", "Open SimNation", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit Radio", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => { if let Some(w) = app.get_webview_window("main") { let _ = w.show(); let _ = w.set_focus(); } }
                    "quit" => { std::process::exit(0); }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| match event {
                    TrayIconEvent::Click { button: MouseButton::Left, .. } => {
                        if let Some(w) = tray.app_handle().get_webview_window("main") { let _ = w.show(); let _ = w.set_focus(); }
                    }
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                if window.label() == "main" { let _ = window.hide(); api.prevent_close(); }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            update_hotkeys, 
            hide_overlay,
            toggle_telemetry_mock,
            install_telemetry_plugin
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}