#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod cc;
mod google;
mod sys;
mod win;

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

/// Ukuran jendela tetap (logis). Notch digambar di dalamnya; area di luar notch tembus klik.
const WIN_W: f64 = 760.0;
const WIN_H: f64 = 560.0;

static HIT: Mutex<Vec<[f64; 4]>> = Mutex::new(Vec::new());
static OPEN: AtomicBool = AtomicBool::new(false);

fn place(win: &WebviewWindow) -> Result<(i32, i32, f64), String> {
    let m = win.primary_monitor().map_err(|e| e.to_string())?.ok_or("monitor tidak ditemukan")?;
    let sf = m.scale_factor();
    let (mx, my) = (m.position().x as f64 / sf, m.position().y as f64 / sf);
    let mw = m.size().width as f64 / sf;
    win.set_size(tauri::LogicalSize::new(WIN_W, WIN_H)).map_err(|e| e.to_string())?;
    win.set_position(tauri::LogicalPosition::new(mx + (mw - WIN_W) / 2.0, my)).map_err(|e| e.to_string())?;
    Ok((m.position().x + m.size().width as i32 / 2, m.size().width as i32, sf))
}

/// UI melaporkan area interaktif (px CSS relatif jendela) dan apakah notch terbuka.
#[tauri::command]
fn set_hit(rects: Vec<[f64; 4]>, open: bool) {
    if let Ok(mut h) = HIT.lock() { *h = rects; }
    OPEN.store(open, Ordering::Relaxed);
}
#[tauri::command]
fn focus_window(win: WebviewWindow) { let _ = win.set_focus(); }

#[tauri::command] fn set_volume(level: f32) -> Result<(), String> { sys::set_volume(level / 100.0) }
#[tauri::command] fn set_brightness(level: u32) -> Result<(), String> { sys::set_brightness(level.min(100)) }

async fn blk<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}
#[tauri::command] async fn media() -> Result<Option<sys::Media>, String> { blk(sys::media).await }
#[tauri::command] async fn media_ctl(op: String) -> Result<(), String> { blk(move || sys::media_ctl(&op)).await }
#[tauri::command] async fn wifi_list() -> Result<Vec<sys::Net>, String> { blk(sys::wifi_list).await }
#[tauri::command] async fn wifi_connect(ssid: String, password: Option<String>) -> Result<(), String> { blk(move || sys::wifi_connect(&ssid, password)).await }
#[tauri::command] async fn bt_list() -> Result<Vec<sys::BtDevice>, String> { blk(sys::bt_list).await }
#[tauri::command] async fn bt_connect(name: String) -> Result<(), String> { blk(move || sys::bt_connect(&name)).await }
#[tauri::command] async fn open_settings(uri: String) -> Result<(), String> {
    if !uri.starts_with("ms-settings:") { return Err("URI tidak diizinkan".into()); }
    blk(move || {
        let mut cmd = Command::new("cmd");
        cmd.args(["/c", "start", "", &uri]);
        sys::hide_console(&mut cmd);
        cmd.spawn().map_err(|e| e.to_string())?;
        Ok(())
    }).await
}
#[tauri::command] async fn radios() -> Result<sys::RadioInfo, String> { blk(sys::radios).await }
#[tauri::command] async fn set_radio(kind: String, on: bool) -> Result<(), String> { blk(move || sys::set_radio(&kind, on)).await }
#[tauri::command] async fn levels() -> Result<(f32, Option<u32>), String> { blk(sys::levels).await }
#[tauri::command] async fn system_stats() -> Result<sys::SystemStats, String> { blk(sys::system_stats).await }
#[tauri::command] async fn gcal_status() -> google::GCalStatus { google::status() }
#[tauri::command] async fn gcal_login() -> Result<(), String> { blk(google::login).await }
#[tauri::command] async fn gcal_save_creds(id: String, secret: String) -> Result<(), String> { blk(move || google::save_creds(id, secret)).await }
#[tauri::command] async fn gcal_open_folder() -> Result<(), String> { blk(google::open_config_folder).await }
#[tauri::command] async fn gcal_events(min: String, max: String) -> Result<Vec<google::Ev>, String> { blk(move || google::events(&min, &max)).await }

// ---- Claude Code
#[tauri::command] async fn cc_snapshot(since: i64) -> Result<serde_json::Value, String> { blk(move || cc::snapshot(since)).await }
#[tauri::command] async fn cc_session(id: String, ver: u64) -> Result<serde_json::Value, String> { blk(move || cc::session(id, ver)).await }
#[tauri::command] async fn cc_decide(id: u64, action: String, note: Option<String>) -> Result<(), String> { blk(move || cc::decide(id, action, note)).await }
#[tauri::command] async fn cc_run(cwd: String, prompt: String, resume: Option<String>, model: Option<String>, mode: Option<String>) -> Result<String, String> {
    blk(move || cc::run(cwd, prompt, resume, model, mode)).await
}
#[tauri::command] async fn cc_kill(pid: u32) -> Result<(), String> { blk(move || cc::kill(pid)).await }
#[tauri::command] async fn cc_open(kind: String, cwd: String, session: Option<String>) -> Result<(), String> { blk(move || cc::open(kind, cwd, session)).await }
#[tauri::command] async fn cc_hooks(action: String) -> Result<serde_json::Value, String> { blk(move || cc::hooks(action)).await }
#[tauri::command] async fn cc_history() -> Result<serde_json::Value, String> { blk(cc::history).await }
#[tauri::command] async fn cc_clear_allow() -> Result<(), String> { blk(cc::clear_allow).await }

// ---- Autostart
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[tauri::command]
async fn get_autostart() -> Result<bool, String> {
    blk(|| {
        let mut c = Command::new("reg");
        c.args(["query", RUN_KEY, "/v", "WindowsNotch"]);
        sys::hide_console(&mut c);
        Ok(c.output().map(|o| o.status.success()).unwrap_or(false))
    }).await
}
#[tauri::command]
async fn set_autostart(enabled: bool) -> Result<(), String> {
    blk(move || {
        let mut c = Command::new("reg");
        if enabled {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            c.args(["add", RUN_KEY, "/v", "WindowsNotch", "/t", "REG_SZ", "/d", &format!("\"{}\"", exe.to_string_lossy()), "/f"]);
        } else {
            c.args(["delete", RUN_KEY, "/v", "WindowsNotch", "/f"]);
        }
        sys::hide_console(&mut c);
        let s = c.status().map_err(|e| e.to_string())?;
        if s.success() || (!enabled && s.code() == Some(1)) { Ok(()) } else { Err("Gagal mengubah autostart Windows".into()) }
    }).await
}

fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
        let _ = w.emit("tray-show", ());
    }
}

/// Thread input: hotkey global, hit-test tembus klik, klik di luar, dan deteksi browser di bawah notch.
fn input_loop(win: WebviewWindow, app: AppHandle) {
    let mut hot = false;
    let mut was_down = false;
    let mut inside_last: Option<bool> = None;
    let (mut pos, mut sf) = (win.outer_position().unwrap_or_default(), win.scale_factor().unwrap_or(1.0));
    let mut center = place(&win).map(|x| x.0).unwrap_or(960);
    let (mut t_pos, mut t_browser) = (Instant::now(), Instant::now());
    let mut browser = false;
    let _ = win.set_ignore_cursor_events(true);
    loop {
        let h = win::key_down(0x11) && win::key_down(0x12) && win::key_down(0x20);
        if h && !hot { let _ = app.emit("global-hotkey", ()); }
        hot = h;

        if t_pos.elapsed() > Duration::from_millis(1500) {
            t_pos = Instant::now();
            pos = win.outer_position().unwrap_or(pos);
            sf = win.scale_factor().unwrap_or(sf);
            // Pindahkan ulang hanya bila resolusi/monitor berubah.
            if let Ok(Some(m)) = win.primary_monitor() {
                let msf = m.scale_factor();
                let want_x = m.position().x + ((m.size().width as f64 - WIN_W * msf) / 2.0).round() as i32;
                if (pos.x - want_x).abs() > 2 || pos.y != m.position().y || (sf - msf).abs() > 0.01 {
                    if let Ok(c) = place(&win) { center = c.0; }
                    pos = win.outer_position().unwrap_or(pos);
                    sf = win.scale_factor().unwrap_or(sf);
                }
            }
        }

        let p = win::cursor();
        let (x, y) = ((p.x - pos.x) as f64 / sf, (p.y - pos.y) as f64 / sf);
        let inside = HIT.lock().map(|r| r.iter().any(|r| x >= r[0] - 2.0 && x <= r[0] + r[2] + 2.0 && y >= r[1] - 2.0 && y <= r[1] + r[3] + 2.0)).unwrap_or(false);
        if inside_last != Some(inside) {
            inside_last = Some(inside);
            let _ = win.set_ignore_cursor_events(!inside);
            let _ = win.emit("hover", inside);
        }
        let down = win::key_down(0x01) || win::key_down(0x02);
        if down && !was_down && !inside && OPEN.load(Ordering::Relaxed) {
            let _ = win.emit("click-outside", ());
        }
        was_down = down;

        if t_browser.elapsed() > Duration::from_millis(400) {
            t_browser = Instant::now();
            let b = win::browser_under(center);
            if b != browser { browser = b; let _ = win.emit("browser-under-notch", b); }
        }
        std::thread::sleep(Duration::from_millis(16));
    }
}

mod spectrum;

#[tauri::command]
fn spectrum_start(app: AppHandle) { spectrum::start(app); }
#[tauri::command]
fn spectrum_stop() { spectrum::stop(); }

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            cc::start(app.handle().clone());
            let open = MenuItem::with_id(app, "show", "Buka Notch", true, None::<&str>)?;
            let autostart = MenuItem::with_id(app, "autostart", "Autostart Windows", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Keluar", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &autostart, &quit])?;
            let mut tray = TrayIconBuilder::new()
                .menu(&menu)
                .tooltip("Windows Notch")
                .on_menu_event(|app, e| match e.id().as_ref() {
                    "show" => show(app),
                    "autostart" => { let _ = app.emit("tray-autostart", ()); }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, e| {
                    if let TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, button_state: tauri::tray::MouseButtonState::Up, .. } = e {
                        show(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() { tray = tray.icon(icon.clone()); }
            tray.build(app)?;
            if let Some(win) = app.get_webview_window("main") {
                let _ = place(&win);
                let (w, a) = (win.clone(), app.handle().clone());
                std::thread::spawn(move || input_loop(w, a));
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(false) = event {
                let _ = window.emit("window-blur", ());
            }
        })
        .invoke_handler(tauri::generate_handler![
            set_hit, focus_window, set_volume, set_brightness,
            media, media_ctl, wifi_list, wifi_connect, bt_list, bt_connect, open_settings,
            radios, set_radio, levels, system_stats,
            gcal_status, gcal_login, gcal_save_creds, gcal_open_folder, gcal_events,
            get_autostart, set_autostart,
            cc_snapshot, cc_session, cc_decide, cc_run, cc_kill, cc_open, cc_hooks, cc_history, cc_clear_allow,
            spectrum_start, spectrum_stop
        ])
        .run(tauri::generate_context!())
        .expect("gagal menjalankan aplikasi");
}
