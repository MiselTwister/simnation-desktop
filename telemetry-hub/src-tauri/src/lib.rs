use std::str::FromStr;
use std::time::Duration;
use std::sync::{Arc, Mutex}; 
use std::fs::OpenOptions;
use std::io::Write; 
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use sysinfo::System;

// 🧰 NEW: System Tray Imports
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;

// 🛰️ WINDOWS API
use winapi::um::memoryapi::{OpenFileMappingW, MapViewOfFile, FILE_MAP_READ, UnmapViewOfFile};
use winapi::um::handleapi::CloseHandle;
use winapi::um::errhandlingapi::GetLastError; 

// 🎯 ZONE OFFSETS (Perfectly Aligned to 64-bit scsTelemetryMap_t)
const GEAR_OFFSET: usize = 504;         
const SPEED_OFFSET: usize = 948;        
const RPM_OFFSET: usize = 952;          
const CRUISE_OFFSET: usize = 988;       
const FUEL_OFFSET: usize = 1000;        
const TEMP_OFFSET: usize = 1024;        
const ODOMETER_OFFSET: usize = 1056;    
const ROUTE_DIST_OFFSET: usize = 1060;  
const ROUTE_TIME_OFFSET: usize = 1064;  
const LIMIT_OFFSET: usize = 1068;       

// 💥 DAMAGE OFFSETS
const WEAR_ENGINE: usize = 1036;
const WEAR_TRANSMISSION: usize = 1040;
const WEAR_CABIN: usize = 1044;
const WEAR_CHASSIS: usize = 1048;
const WEAR_WHEELS: usize = 1052;
const TRAILER_CHASSIS: usize = 6156;
const TRAILER_WHEELS: usize = 6160;
const TRAILER_BODY: usize = 6164;
const CARGO_DAMAGE: usize = 1468;

struct AppState {
    is_mock_active: bool,
}

// 📝 PRO LOGGER FUNCTION
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
                    "speed": 65, "limit": 80, "gear": 12, "fuel": 85, "temp": 90, "damage": 0,
                    "rpm": 1200, "routeDistance": 150.5, "routeTime": 5400, "cruiseControl": 65, "odometer": 12050
                }));
                // 🛑 PRO FIX: Throttled to ~30fps
                std::thread::sleep(Duration::from_millis(33));
                continue;
            }

            unsafe {
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

                loop {
                    if { state.lock().unwrap().is_mock_active } {
                        break; 
                    }

                    let sdk_active = *(p_buf as *const bool);
                    
                    if sdk_active {
                        let base_ptr = p_buf as *const u8;
                        
                        let gear = *(base_ptr.add(GEAR_OFFSET) as *const i32);
                        let speed_ms = *(base_ptr.add(SPEED_OFFSET) as *const f32);
                        let limit_ms = *(base_ptr.add(LIMIT_OFFSET) as *const f32);
                        let fuel = *(base_ptr.add(FUEL_OFFSET) as *const f32);
                        let temp = *(base_ptr.add(TEMP_OFFSET) as *const f32);
                        
                        let rpm = *(base_ptr.add(RPM_OFFSET) as *const f32);
                        let cruise_ms = *(base_ptr.add(CRUISE_OFFSET) as *const f32);
                        let odometer_km = *(base_ptr.add(ODOMETER_OFFSET) as *const f32);
                        let route_dist_m = *(base_ptr.add(ROUTE_DIST_OFFSET) as *const f32);
                        let route_time_s = *(base_ptr.add(ROUTE_TIME_OFFSET) as *const f32);

                        let w_eng = *(base_ptr.add(WEAR_ENGINE) as *const f32);
                        let w_tra = *(base_ptr.add(WEAR_TRANSMISSION) as *const f32);
                        let w_cab = *(base_ptr.add(WEAR_CABIN) as *const f32);
                        let w_cha = *(base_ptr.add(WEAR_CHASSIS) as *const f32);
                        let w_whl = *(base_ptr.add(WEAR_WHEELS) as *const f32);
                        let t_cha = *(base_ptr.add(TRAILER_CHASSIS) as *const f32);
                        let t_whl = *(base_ptr.add(TRAILER_WHEELS) as *const f32);
                        let t_bod = *(base_ptr.add(TRAILER_BODY) as *const f32);
                        let c_dam = *(base_ptr.add(CARGO_DAMAGE) as *const f32);

                        let mut damages = vec![w_eng, w_tra, w_cab, w_cha, w_whl, t_cha, t_whl, t_bod, c_dam];
                        let max_damage = damages.into_iter()
                            .filter(|d| !d.is_nan()) 
                            .fold(0.0_f32, |a, b| a.max(b)); 

                        let safe_speed = if speed_ms.is_nan() { 0.0 } else { (speed_ms * 3.6).abs() };
                        let safe_limit = if limit_ms.is_nan() { 0.0 } else { (limit_ms * 3.6).abs() };
                        let safe_cruise = if cruise_ms.is_nan() { 0.0 } else { (cruise_ms * 3.6).abs() };
                        let safe_damage = (max_damage * 100.0).round();
                        let safe_fuel = if fuel.is_nan() { 0.0 } else { fuel };
                        let safe_temp = if temp.is_nan() { 0.0 } else { temp };
                        let safe_rpm = if rpm.is_nan() { 0.0 } else { rpm };
                        
                        let dist_km = if route_dist_m.is_nan() || route_dist_m < 0.0 { 0.0 } else { route_dist_m / 1000.0 };
                        let time_mins = if route_time_s.is_nan() || route_time_s < 0.0 { 0.0 } else { route_time_s / 60.0 };

                        tick_counter += 1;
                        if tick_counter >= 60 { // Adjusted log pulse to match the new 30fps rate
                            log_to_file(&format!("DATA PULSE -> Speed: {:.1} | Gear: {} | GPS Dist: {:.1}km", safe_speed, gear, dist_km));
                            tick_counter = 0;
                        }

                        let _ = handle.emit("telemetry-update", serde_json::json!({
                            "speed": safe_speed,
                            "limit": safe_limit,
                            "gear": gear,
                            "fuel": safe_fuel, 
                            "temp": safe_temp,
                            "damage": safe_damage,
                            "rpm": safe_rpm,
                            "cruiseControl": safe_cruise,
                            "odometer": odometer_km,
                            "routeDistance": dist_km,
                            "routeTime": time_mins
                        }));
                    } else {
                        if !sdk_error_logged {
                            log_to_file("WAITING: Memory block found, but SDK is not sending data yet.");
                            sdk_error_logged = true;
                        }
                        last_connected_state = false;
                        break;
                    }
                    
                    // 🛑 PRO FIX: Throttled to ~30fps to stop IPC Flooding to the frontend
                    std::thread::sleep(Duration::from_millis(33)); 
                }

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
            
            // 🧰 NEW: Setup System Tray Menu
            let quit_i = MenuItem::with_id(app, "quit", "Quit SimNation", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Show Hub", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

            // 🧰 NEW: Build the System Tray
            if let Some(icon) = app.default_window_icon().cloned() {
                let _tray = TrayIconBuilder::new()
                    .icon(icon)
                    .menu(&menu)
                    .on_menu_event(move |app, event| match event.id.as_ref() {
                        "quit" => std::process::exit(0),
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        _ => {}
                    })
                    .build(app)?;
            }

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