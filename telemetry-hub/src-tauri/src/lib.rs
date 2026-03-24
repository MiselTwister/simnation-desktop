use std::str::FromStr;
use std::time::Duration;
use std::sync::{Arc, Mutex}; 
use std::fs::OpenOptions;
use std::io::Write; 
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use sysinfo::System;

// 🛰️ WINDOWS API
use winapi::um::memoryapi::{OpenFileMappingW, MapViewOfFile, FILE_MAP_READ, UnmapViewOfFile};
use winapi::um::handleapi::CloseHandle;
use winapi::um::errhandlingapi::GetLastError; 

// 🎯 ZONE OFFSETS (Verified against scs-telemetry-common.hpp)
const SPEED_OFFSET: usize = 700;        
const FUEL_OFFSET: usize = 752;         
const TEMP_OFFSET: usize = 776;         
const DAMAGE_OFFSET: usize = 788;       
const LIMIT_OFFSET: usize = 820;        
const GEAR_OFFSET: usize = 504;         

struct AppState {
    is_mock_active: bool,
}

// 📝 PRO LOGGER FUNCTION
fn log_to_file(message: &str) {
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("telemetry_debug.log") 
    {
        // 🚨 FIX: Using full path for chrono
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{}] {}", timestamp, message);
    }
}

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
                    if w.is_visible().unwrap_or(false) { let _ = w.hide(); } 
                    else { let _ = w.show(); let _ = w.unminimize(); let _ = w.set_focus(); }
                }
            }
        });
    }
    Ok(())
}

fn start_telemetry_loop(handle: AppHandle, state: Arc<Mutex<AppState>>) {
    std::thread::spawn(move || {
        let name_str = "Local\\SCSTelemetry"; 
        let mut name: Vec<u16> = name_str.encode_utf16().collect();
        name.push(0); 

        log_to_file(&format!("THREAD START: Looking for mapping {}", name_str));

        let mut last_connected_state = false;
        let mut error_count = 0;

        loop {
            let is_mock = { state.lock().unwrap().is_mock_active };

            if is_mock {
                let _ = handle.emit("telemetry-update", serde_json::json!({
                    "speed": 65, "limit": 80, "gear": 12, "fuel": 85, "temp": 90, "damage": 0
                }));
            } else {
                unsafe {
                    let h_map_file = OpenFileMappingW(FILE_MAP_READ, 0, name.as_ptr());
                    
                    if h_map_file.is_null() {
                        if last_connected_state {
                            let err = GetLastError();
                            log_to_file(&format!("LOST CONNECTION: Mapping NULL. WinErr: {}", err));
                            last_connected_state = false;
                        }
                    } else {
                        let p_buf = MapViewOfFile(h_map_file, FILE_MAP_READ, 0, 0, 0);
                        
                        if !p_buf.is_null() {
                            let sdk_active = *(p_buf as *const bool);
                            
                            if sdk_active {
                                if !last_connected_state {
                                    log_to_file("SUCCESS: Connected to Shared Memory and SDK is ACTIVE");
                                    last_connected_state = true;
                                }

                                // 🚨 Variables synced with constant names
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
                            } else {
                                if error_count % 100 == 0 { // Log every ~1.6 seconds to avoid massive files
                                    log_to_file("WAITING: Memory found but SDK Active is false.");
                                }
                                error_count += 1;
                                last_connected_state = false;
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
                            let _ = w.show(); let _ = w.unminimize(); let _ = w.set_always_on_top(true);
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