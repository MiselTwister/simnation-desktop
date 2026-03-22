use std::str::FromStr;
use std::time::Duration;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WindowEvent, State,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use sysinfo::System;

// 🚛 Telemetry & System Imports
use scs_sdk_telemetry::shared_memory::SharedMemory;
use winreg::enums::*;
use winreg::RegKey;
use discord_rich_presence::{Backend, DiscordIpc, DiscordIpcClient};

// --- 🎮 DISCORD STATE ---
struct DiscordState(Arc<Mutex<Option<DiscordIpcClient>>>);

// --- 🛠️ COMMAND: ONE-CLICK TELEMETRY INSTALLER ---
#[tauri::command]
async fn install_telemetry_plugin(app: AppHandle) -> Result<String, String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let steam_key = hkcu.open_subkey("Software\\Valve\\Steam").map_err(|_| "Steam not found in registry")?;
    let steam_path: String = steam_key.get_value("SteamPath").map_err(|_| "Could not find SteamPath")?;
    
    let mut installed_games = Vec::new();
    let games = [
        ("Euro Truck Simulator 2", "eurotrucks2"),
        ("American Truck Simulator", "amtrucks")
    ];

    for (name, folder) in games {
        let mut plugin_path = PathBuf::from(&steam_path);
        plugin_path.push("steamapps/common");
        plugin_path.push(name);
        plugin_path.push("bin/win_x64/plugins");

        // Create directory if it doesn't exist
        if let Err(_) = std::fs::create_dir_all(&plugin_path) { continue; }

        // Path to our bundled DLL (defined in tauri.conf.json resources)
        let resource_path = app.path().resolve("resources/scs-telemetry.dll", tauri::path::BaseDirectory::Resource)
            .map_err(|_| "Bundled DLL not found")?;

        plugin_path.push("scs-telemetry.dll");
        std::fs::copy(resource_path, plugin_path).map_err(|e| format!("Failed to copy DLL: {}", e))?;
        installed_games.push(name);
    }

    if installed_games.is_empty() {
        return Err("No SCS games found to install plugins into.".into());
    }
    Ok(format!("Installed SNR Telemetry to: {}", installed_games.join(", ")))
}

// --- 🔊 COMMAND: DISCORD RICH PRESENCE ---
#[tauri::command]
fn update_discord(state: State<'_, DiscordState>, details: String, state_text: String, art_key: String) -> Result<(), String> {
    let mut client_lock = state.0.lock().unwrap();
    if client_lock.is_none() {
        let mut client = DiscordIpcClient::new("YOUR_DISCORD_CLIENT_ID").map_err(|_| "Client Init Failed")?;
        let _ = client.connect();
        *client_lock = Some(client);
    }

    if let Some(client) = client_lock.as_mut() {
        let payload = discord_rich_presence::activity::Activity::new()
            .details(&details)
            .state(&state_text)
            .assets(discord_rich_presence::activity::Assets::new().large_image(&art_key));
        
        client.set_activity(payload).map_err(|_| "Update Failed")?;
    }
    Ok(())
}

// --- 🚛 TELEMETRY STREAMER ---
fn start_telemetry_loop(handle: AppHandle) {
    std::thread::spawn(move || {
        let mut shared_mem = SharedMemory::connect();
        loop {
            if let Ok(data) = shared_mem.read() {
                let payload = serde_json::json!({
                    "speed": data.truck.common.speed.kmph,
                    "gear": data.truck.common.gear.displayed,
                    "fuel": data.truck.common.fuel.value,
                    "fuel_max": data.truck.common.fuel.capacity,
                    "damage": data.truck.common.damage.total,
                    "blinkers": {
                        "l": data.truck.common.lights.blinker_left_on,
                        "r": data.truck.common.lights.blinker_right_on
                    }
                });
                let _ = handle.emit("telemetry-update", payload);
            }
            std::thread::sleep(Duration::from_millis(16)); // ~60fps
        }
    });
}

// --- OVERLAY LOGIC ---
fn toggle_overlay_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("overlay") {
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
        .manage(DiscordState(Arc::new(Mutex::new(None))))
        .plugin(tauri_plugin_updater::Builder::new().build()) 
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--silent"])))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(|app| {
            let handle = app.handle().clone();

            // 🚀 Start Telemetry Bridge
            start_telemetry_loop(handle.clone());

            // 🚛 Game Detector Loop
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
                        if let Some(w) = handle.get_webview_window("overlay") {
                            let _ = w.show(); let _ = w.unminimize(); let _ = w.set_always_on_top(true);
                        }
                        if let Some(main) = handle.get_webview_window("main") { let _ = main.hide(); }
                        was_running = true;
                    } else if !is_running && was_running {
                        if let Some(w) = handle.get_webview_window("overlay") { let _ = w.hide(); }
                        if let Some(main) = handle.get_webview_window("main") { let _ = main.show(); let _ = main.unminimize(); let _ = main.set_focus(); }
                        was_running = false;
                    }
                    std::thread::sleep(Duration::from_secs(5));
                }
            });

            // Tray & Menu setup omitted for brevity but should remain as per your existing code...
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
            update_hotkeys, hide_overlay, install_telemetry_plugin, update_discord
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}