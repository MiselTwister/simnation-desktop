use std::str::FromStr;
use std::time::Duration;
use std::sync::{Arc, Mutex}; 
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use sysinfo::System;

// 🛰️ WINDOWS API FOR RAW MEMORY READING
use winapi::um::memoryapi::{OpenFileMappingW, MapViewOfFile, FILE_MAP_READ, UnmapViewOfFile};
use winapi::um::handleapi::CloseHandle;
use std::os::windows::ffi::OsStrExt;

// 🎯 ZONE OFFSETS (Aligned with your C++ scsTelemetryMap_t struct)
const SPEED_OFFSET: usize = 704;        // truck.speed
const LIMIT_OFFSET: usize = 712;        // truck.navigation.speed.limit
const GEAR_OFFSET: usize = 508;         // truck.displayed.gear
const WATER_TEMP_OFFSET: usize = 744;   // truck.water.temperature
const FUEL_OFFSET: usize = 756;         // truck.fuel.amount
const DAMAGE_OFFSET: usize = 792;       // truck.wear.engine

// --- 🧠 APP STATE ---
struct AppState {
    is_mock_active: bool,
}

// --- 🎮 FRONTEND COMPATIBILITY COMMANDS ---
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
        let _ = manager.on_shortcut(new, move |handle: &AppHandle, _, event| { 
            if event.state == ShortcutState::Pressed { let _ = handle.emit("media-play", ()); }
        });
    }
    if let Ok(new) = Shortcut::from_str(&new_stop) {
        let _ = manager.on_shortcut(new, move |handle: &AppHandle, _, event| { 
            if event.state == ShortcutState::Pressed { let _ = handle.emit("media-stop", ()); }
        });
    }
    if let Ok(new) = Shortcut::from_str(&new_overlay) {
        let _ = manager.on_shortcut(new, move |handle: &AppHandle, _, event| { 
            if event.state == ShortcutState::Pressed {
                if let Some(w) = handle.get_webview_window("radio_overlay") {
                    let is_vis = w.is_visible().unwrap_or(false);
                    if is_vis { let _ = w.hide(); } else { 
                        let _ = w.show(); 
                        let _ = w.unminimize();
                        let _ = w.set_always_on_top(true);
                        let _ = w.set_focus(); 
                    }
                }
            }
        });
    }
    Ok(())
}

// --- 🚛 RAW TELEMETRY LOOP ---
fn start_telemetry_loop(handle: AppHandle, state: Arc<Mutex<AppState>>) {
    std::thread::spawn(move || {
        // 🚨 FIX: Standardized escape for Local\\SCSTelemetry
        let name: Vec<u16> = std::ffi::OsStr::new("Local\\SCSTelemetry")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut mock_counter: f32 = 0.0;

        loop {
            let is_mock = {
                let s = state.lock().unwrap();
                s.is_mock_active
            };

            if is_mock {
                mock_counter += 0.1;
                let mock_speed = (mock_counter.sin() * 40.0) + 60.0; 
                let _ = handle.emit("telemetry-update", serde_json::json!({
                    "speed": mock_speed,
                    "limit": 80,
                    "gear": 12,
                    "fuel": 85, 
                    "temp": 90,
                    "damage": 2
                }));
            } else {
                unsafe {
                    let h_map_file = OpenFileMappingW(FILE_MAP_READ, 0, name.as_ptr());
                    if !h_map_file.is_null() {
                        let p_buf = MapViewOfFile(h_map_file, FILE_MAP_READ, 0, 0, 0);
                        if !p_buf.is_null() {
                            // 🚨 PRO CHECK: Verify if the SDK is actually active
                            let sdk_active = *(p_buf as *const bool);
                            
                            if sdk_active {
                                let speed_ms = *(p_buf.add(SPEED_OFFSET) as *const f32);
                                let limit_ms = *(p_buf.add(LIMIT_OFFSET) as *const f32);
                                let gear = *(p_buf.add(GEAR_OFFSET) as *const i32);
                                let fuel = *(p_buf.add(FUEL_OFFSET) as *const f32);
                                let temp = *(p_buf.add(WATER_TEMP_OFFSET) as *const f32);
                                let damage = *(p_buf.add(DAMAGE_OFFSET) as *const f32);

                                let _ = handle.emit("telemetry-update", serde_json::json!({
                                    "speed": (speed_ms * 3.6).abs(),
                                    "limit": (limit_ms * 3.6).abs(),
                                    "gear": gear,
                                    "fuel": fuel, 
                                    "temp": temp,
                                    "damage": (damage * 100.0).round()
                                }));
                            }
                            UnmapViewOfFile(p_buf);
                        }
                        CloseHandle(h_map_file);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(16)); 
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = Arc::new(Mutex::new(AppState { is_mock_active: false }));

    tauri::Builder::default()
        .manage(app_state.clone()) 
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--silent"])))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(move |app| {
            let handle = app.handle().clone();

            start_telemetry_loop(handle.clone(), app_state.clone());

            std::thread::spawn(move || {
                let mut sys = System::new_all();
                let games = ["eurotrucks2", "amtrucks"];
                let mut was_running = false;
                
                loop {
                    sys.refresh_processes(sysinfo::ProcessesToUpdate::All);
                    let is_running = sys.processes().values().any(|p| {
                        let n = p.name().to_string_lossy().to_lowercase();
                        games.iter().any(|&g| n.contains(g))
                    });

                    if is_running && !was_running {
                        if let Some(w) = handle.get_webview_window("main") { let _ = w.show(); }
                        if let Some(w) = handle.get_webview_window("radio_overlay") { 
                            let _ = w.show(); 
                            let _ = w.unminimize();
                            let _ = w.set_always_on_top(true);
                        }
                        was_running = true;
                    } else if !is_running && was_running {
                        if let Some(w) = handle.get_webview_window("main") { let _ = w.hide(); }
                        if let Some(w) = handle.get_webview_window("radio_overlay") { let _ = w.hide(); }
                        was_running = false;
                    }
                    std::thread::sleep(Duration::from_secs(5));
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            update_hotkeys,
            toggle_telemetry_mock,
            install_telemetry_plugin
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}