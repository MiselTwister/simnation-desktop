use std::str::FromStr;
use std::time::Duration;
use std::sync::{Arc, Mutex}; 
use tauri::{AppHandle, Emitter, Manager}; // Cleaned up unused imports
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use sysinfo::System;

// 🛰️ WINDOWS API FOR RAW MEMORY READING
use winapi::um::memoryapi::{OpenFileMappingW, MapViewOfFile, FILE_MAP_READ, UnmapViewOfFile};
use winapi::um::handleapi::CloseHandle;

// 🎯 ACCURATE ZONE OFFSETS (Verified against scs-telemetry-common.hpp)
// Zone 4 (Floats) starts at 700. truck_f is the first struct in Zone 4.
const SPEED_OFFSET: usize = 700;        // truck_f.speed (Offset 700)
const FUEL_OFFSET: usize = 752;         // truck_f.fuel (Offset 700 + 52)
const TEMP_OFFSET: usize = 776;         // truck_f.waterTemperature (Offset 700 + 76)
const DAMAGE_OFFSET: usize = 788;       // truck_f.wearEngine (Offset 700 + 88)
const LIMIT_OFFSET: usize = 820;        // truck_f.speedLimit (Offset 700 + 120)

// Zone 3 (Integers) starts at 500. common_i (4 bytes) comes before truck_i.
const GEAR_OFFSET: usize = 504;         // truck_i.gear (Offset 500 + 4)

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
        // 🚨 PRO FIX: UTF-16 encoding for the Windows Shared Memory name
        let name_str = "Local\\SCSTelemetry";
        let mut name: Vec<u16> = name_str.encode_utf16().collect();
        name.push(0); 

        loop {
            let is_mock = {
                let s = state.lock().unwrap();
                s.is_mock_active
            };

            if is_mock {
                // ... (Mock logic remains the same as it works)
                let _ = handle.emit("telemetry-update", serde_json::json!({
                    "speed": 65, "limit": 80, "gear": 12, "fuel": 85, "temp": 90, "damage": 0
                }));
            } else {
                unsafe {
                    let h_map_file = OpenFileMappingW(FILE_MAP_READ, 0, name.as_ptr());
                    if !h_map_file.is_null() {
                        let p_buf = MapViewOfFile(h_map_file, FILE_MAP_READ, 0, 0, 0);
                        if !p_buf.is_null() {
                            // 🏁 Check 'sdkActive' at Byte 0
                            let sdk_active = *(p_buf as *const bool);
                            
                            if sdk_active {
                                // 📐 Precision Offset Reading
                                let speed_ms = *(p_buf.add(SPEED_OFFSET) as *const f32);
                                let limit_ms = *(p_buf.add(LIMIT_OFFSET) as *const f32);
                                let gear = *(p_buf.add(GEAR_OFFSET) as *const i32);
                                let fuel = *(p_buf.add(FUEL_OFFSET) as *const f32);
                                let temp = *(p_buf.add(TEMP_OFFSET) as *const f32);
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

            // 🔍 Game Detection Loop
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