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

// 🎯 ZONE OFFSETS (Aligned with scs-telemetry-common.hpp)
const SPEED_OFFSET: usize = 700;        
const FUEL_OFFSET: usize = 752;         
const TEMP_OFFSET: usize = 776;         
const DAMAGE_OFFSET: usize = 788;       
const LIMIT_OFFSET: usize = 820;        
const GEAR_OFFSET: usize = 504;         

struct AppState {
    is_mock_active: bool,
}

// 📝 PRO LOGGER FUNCTION - Writes to Windows Documents folder
fn log_to_file(message: &str) {
    if let Ok(mut log_path) = std::env::var("USERPROFILE").map(std::path::PathBuf::from) {
        log_path.push("Documents");
        log_path.push("SimNation_Debug.log");

        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path) 
        {
            let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(file, "[{}] {}", timestamp, message);
        }
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

        log_to_file("SERVICE START: Shared Memory Listener Initialized.");

        let mut last_connected_state = false;
        let mut sdk_error_logged = false;

        loop {
            let is_mock = { state.lock().unwrap().is_mock_active };

            if is_mock {
                let _ = handle.emit("telemetry-update", serde_json::json!({
                    "speed": 65, "limit": 80, "gear": 12, "fuel": 85, "temp": 90, "damage": 0
                }));
                std::thread::sleep(Duration::from_millis(16));
                continue;
            }

            unsafe {
                // 1. OPEN HANDLE ONCE
                let h_map_file = OpenFileMappingW(FILE_MAP_READ, 0, name.as_ptr());
                
                if h_map_file.is_null() {
                    if last_connected_state {
                        let err = GetLastError();
                        log_to_file(&format!("LOST CONNECTION: Mapping vanished (WinErr {}). Waiting for game...", err));
                        last_connected_state = false;
                    }
                    std::thread::sleep(Duration::from_millis(1000));
                    continue; 
                }

                // 2. MAP VIEW ONCE
                let p_buf = MapViewOfFile(h_map_file, FILE_MAP_READ, 0, 0, 0);
                if p_buf.is_null() {
                    CloseHandle(h_map_file);
                    std::thread::sleep(Duration::from_millis(1000));
                    continue;
                }

                if !last_connected_state {
                    log_to_file("SUCCESS: Connected to Shared Memory and SDK is ACTIVE.");
                    last_connected_state = true;
                    sdk_error_logged = false;
                }

                let mut tick_counter = 0;

                // 3. INNER LOOP: Read continuously without closing handles
                loop {
                    // Break if mock is turned on mid-game
                    if { state.lock().unwrap().is_mock_active } {
                        break; 
                    }

                    let sdk_active = *(p_buf as *const bool);
                    
                    if sdk_active {
                        // 🛑 PRO FIX: Force 1-Byte Pointer Math to get the right offsets
                        let base_ptr = p_buf as *const u8;
                        
                        let speed_ms = *(base_ptr.add(SPEED_OFFSET) as *const f32);
                        let limit_ms = *(base_ptr.add(LIMIT_OFFSET) as *const f32);
                        let gear = *(base_ptr.add(GEAR_OFFSET) as *const i32);
                        let fuel = *(base_ptr.add(FUEL_OFFSET) as *const f32);
                        let temp = *(base_ptr.add(TEMP_OFFSET) as *const f32);
                        let damage = *(base_ptr.add(DAMAGE_OFFSET) as *const f32);

                        // 🛑 PRO FIX: NaN Protection (Prevents JSON serialization crashes)
                        let safe_speed = if speed_ms.is_nan() { 0.0 } else { (speed_ms * 3.6).abs() };
                        let safe_limit = if limit_ms.is_nan() { 0.0 } else { (limit_ms * 3.6).abs() };
                        let safe_damage = if damage.is_nan() { 0.0 } else { (damage * 100.0).round() };
                        let safe_fuel = if fuel.is_nan() { 0.0 } else { fuel };
                        let safe_temp = if temp.is_nan() { 0.0 } else { temp };

                        // 🛑 PRO FIX: Diagnostic Pulse (Logs data every ~2 seconds)
                        tick_counter += 1;
                        if tick_counter >= 120 {
                            log_to_file(&format!("DATA PULSE -> Speed: {:.1} | Gear: {} | Fuel: {:.1}", safe_speed, gear, safe_fuel));
                            tick_counter = 0;
                        }

                        let _ = handle.emit("telemetry-update", serde_json::json!({
                            "speed": safe_speed,
                            "limit": safe_limit,
                            "gear": gear,
                            "fuel": safe_fuel, 
                            "temp": safe_temp,
                            "damage": safe_damage
                        }));
                    } else {
                        if !sdk_error_logged {
                            log_to_file("WAITING: Memory block found, but SDK is not sending data yet.");
                            sdk_error_logged = true;
                        }
                        // If the game engine actually detaches the plugin, break the loop
                        // to clean up the handles properly and wait for restart.
                        last_connected_state = false;
                        break;
                    }
                    
                    std::thread::sleep(Duration::from_millis(16)); 
                }

                // 4. CLEANUP (Only runs if the game shuts down or mock is toggled)
                UnmapViewOfFile(p_buf);
                CloseHandle(h_map_file);
            }
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