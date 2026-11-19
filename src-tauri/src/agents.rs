//! Multi-agent hub bergaya T3 Code: Claude Code, OpenCode, Antigravity, Codex, dan Gemini CLI.
//! Mendukung deteksi biner, cek auth, install & login langsung dari notch, daftar sesi terpadu,
//! timeline, rename, hapus, dan eksekusi headless streaming untuk semua provider.
use crate::{cc, sys, win};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, UNIX_EPOCH};
use tauri::Emitter;

#[derive(Clone, Serialize, Deserialize, Default)]
struct ProviderStatus {
    id: String,
    name: String,
    installed: bool,
    bin: String,
    version: String,
    logged_in: bool,
    account: String,
    has_api_key: bool,
    api_key_env: String,
    install_hint: String,
    supports_fork: bool,
    supports_device_login: bool,
    models: Vec<String>,
    checked_at: i64,
    checking: bool,
}

#[derive(Clone, Default)]
struct CachedSession {
    id: String,
    provider: String,
    title: String,
    prompt: String,
    cwd: String,
    project: String,
    branch: String,
    model: String,
    start: i64,
    last: i64,
    ver: u64,
    tok_in: u64,
    tok_out: u64,
    tok_cache: u64,
    path: Option<PathBuf>,
    items: Vec<Value>,
}

#[derive(Clone, Default)]
struct ManagedRun {
    run_id: String,
    provider: String,
    pid: u32,
    session: String,
    cwd: String,
    model: String,
    prompt: String,
    started: i64,
    done: bool,
    exit: Option<i32>,
    error: String,
    activity: String,
    items: Vec<Value>,
    ver: u64,
    tok_in: u64,
    tok_out: u64,
    tok_cache: u64,
}

#[derive(Default)]
struct AgState {
    providers: Vec<ProviderStatus>,
    providers_ts: i64,
    refreshing_providers: bool,
    opencode_sessions: Vec<CachedSession>,
    opencode_db_mtime: i64,
    opencode_refreshing: bool,
    opencode_models: Vec<String>,
    opencode_cwds: HashSet<String>,
    export_cache: HashMap<String, (i64, Vec<Value>, u64, u64, u64, String)>,
    titles: HashMap<String, String>,
    titles_loaded: bool,
    keys: HashMap<String, String>,
    keys_loaded: bool,
    runs: Vec<ManagedRun>,
    seq: u64,
}

static AG: OnceLock<Arc<Mutex<AgState>>> = OnceLock::new();
fn state() -> &'static Arc<Mutex<AgState>> {
    AG.get_or_init(|| Arc::new(Mutex::new(AgState::default())))
}

fn emit_event(kind: &str, extra: Value) {
    let Some(app) = cc::app_handle() else { return };
    let mut v = json!({ "kind": kind });
    if let (Some(o), Value::Object(e)) = (v.as_object_mut(), extra) {
        o.extend(e);
    }
    let _ = app.emit("cc-event", v);
}

fn emit_setup(provider: &str, op: &str, line: &str, done: bool, ok: bool, error: Option<&str>) {
    let Some(app) = cc::app_handle() else { return };
    let _ = app.emit(
        "agent-setup",
        json!({
            "provider": provider,
            "op": op,
            "line": line,
            "done": done,
            "ok": ok,
            "error": error,
        }),
    );
}

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default())
}

fn appdata_dir() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or_default())
}

fn localappdata_dir() -> PathBuf {
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default())
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if ('@'..='~').contains(&nc) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn file_mtime_ms(p: &Path) -> i64 {
    fs::metadata(p)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ======================================================================
// Penyimpanan Kunci API & Judul Sesi Lokal
// ======================================================================
fn keys_path() -> PathBuf {
    cc::notch_dir().join("provider_keys.json")
}
fn titles_path() -> PathBuf {
    cc::notch_dir().join("session_titles.json")
}

fn ensure_keys(st: &mut AgState) {
    if st.keys_loaded {
        return;
    }
    st.keys_loaded = true;
    if let Ok(raw) = fs::read_to_string(keys_path()) {
        if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&raw) {
            st.keys = map;
        }
    }
}

fn save_keys_map(st: &AgState) -> Result<(), String> {
    let s = serde_json::to_string_pretty(&st.keys).map_err(|e| e.to_string())?;
    fs::write(keys_path(), s + "\n").map_err(|e| e.to_string())
}

fn ensure_titles(st: &mut AgState) {
    if st.titles_loaded {
        return;
    }
    st.titles_loaded = true;
    if let Ok(raw) = fs::read_to_string(titles_path()) {
        if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&raw) {
            st.titles = map;
        }
    }
}

fn save_titles_map(st: &AgState) -> Result<(), String> {
    let s = serde_json::to_string_pretty(&st.titles).map_err(|e| e.to_string())?;
    fs::write(titles_path(), s + "\n").map_err(|e| e.to_string())
}

fn title_key(provider: &str, id: &str) -> String {
    format!("{provider}:{id}")
}

fn provider_key_env(provider: &str) -> &'static str {
    match provider {
        "claude" => "ANTHROPIC_API_KEY",
        "opencode" => "OPENROUTER_API_KEY",
        "antigravity" | "gemini" => "GEMINI_API_KEY",
        "codex" => "OPENAI_API_KEY",
        _ => "API_KEY",
    }
}

pub fn apply_provider_env(cmd: &mut Command, provider: &str) {
    let key = {
        let Ok(mut st) = state().lock() else { return };
        ensure_keys(&mut st);
        st.keys.get(provider).cloned().unwrap_or_default()
    };
    if key.is_empty() {
        return;
    }
    let env_name = provider_key_env(provider);
    cmd.env(env_name, &key);
    if provider == "codex" {
        cmd.env("CODEX_API_KEY", &key);
    }
    if provider == "antigravity" {
        cmd.env("GOOGLE_API_KEY", &key);
    }
}

// ======================================================================
// Pencarian Biner & Eksekusi CLI Lintas Format (.exe / .cmd / .ps1)
// ======================================================================
fn find_in_path_and_dirs(names: &[&str], extra_dirs: &[PathBuf]) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = extra_dirs.to_vec();
    if let Ok(path) = std::env::var("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    let npm = appdata_dir().join("npm");
    if !dirs.contains(&npm) {
        dirs.push(npm);
    }
    for d in dirs {
        for n in names {
            let p = d.join(n);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

pub fn provider_bin(provider: &str) -> Option<PathBuf> {
    let home = home_dir();
    let local = localappdata_dir();
    match provider {
        "claude" => cc::claude_bin(),
        "opencode" => find_in_path_and_dirs(
            &["opencode.exe", "opencode.cmd", "opencode.ps1"],
            &[
                home.join(".opencode").join("bin"),
                home.join(".local").join("bin"),
                PathBuf::from(r"C:\Program Files\nodejs"),
            ],
        ),
        "antigravity" => find_in_path_and_dirs(
            &["agy.exe", "agy.cmd", "agy.ps1", "antigravity.cmd"],
            &[
                home.join(".antigravity").join("bin"),
                home.join(".local").join("bin"),
                local.join("Programs").join("antigravity-cli"),
                local.join("Programs").join("Antigravity").join("resources").join("app").join("bin"),
                local.join("Programs").join("Antigravity").join("bin"),
            ],
        ),
        "codex" => find_in_path_and_dirs(
            &["codex.exe", "codex.cmd", "codex.ps1"],
            &[home.join(".local").join("bin"), PathBuf::from(r"C:\Program Files\nodejs")],
        ),
        "gemini" => find_in_path_and_dirs(
            &["gemini.exe", "gemini.cmd", "gemini.ps1"],
            &[home.join(".local").join("bin"), PathBuf::from(r"C:\Program Files\nodejs")],
        ),
        _ => None,
    }
}

fn cli_command(bin: &Path, args: &[&str]) -> Command {
    let ext = bin.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let mut cmd = if ext == "cmd" || ext == "bat" {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(bin);
        c.args(args);
        c
    } else if ext == "ps1" {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
        c.arg(bin);
        c.args(args);
        c
    } else {
        let mut c = Command::new(bin);
        c.args(args);
        c
    };
    sys::hide_console(&mut cmd);
    cmd
}

/// Bunuh proses beserta seluruh pohon anaknya. CLI `.cmd`/`.ps1` membungkus proses
/// node/powershell, jadi mematikan pembungkus saja bisa menyisakan pipe tetap terbuka.
fn kill_process_tree(pid: u32) {
    let mut c = Command::new("taskkill");
    c.args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    sys::hide_console(&mut c);
    let _ = c.status();
}

/// Jalankan CLI dan tangkap output dengan batas waktu agar cek provider tidak pernah menggantung.
/// Bila perintah tidak selesai tepat waktu, proses dibunuh dan error timeout dikembalikan.
fn run_cli_capture_opts(
    bin: &Path,
    args: &[&str],
    provider: &str,
    timeout: Duration,
    dir: Option<&Path>,
) -> Result<String, String> {
    let mut cmd = cli_command(bin, args);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    apply_provider_env(&mut cmd, provider);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let read_pipe = |pipe: Option<Box<dyn Read + Send>>| {
        pipe.map(|s| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let mut s = s;
                let _ = s.read_to_end(&mut buf);
                buf
            })
        })
    };
    let out_h = read_pipe(child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let err_h = read_pipe(child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if started.elapsed() >= timeout => {
                kill_process_tree(child.id());
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let stdout = out_h.and_then(|h| h.join().ok()).unwrap_or_default();
    let stderr = err_h.and_then(|h| h.join().ok()).unwrap_or_default();
    let stdout = String::from_utf8_lossy(&stdout).trim_start_matches('\u{feff}').to_string();
    let stderr = String::from_utf8_lossy(&stderr).trim_start_matches('\u{feff}').to_string();
    match status {
        Some(s) if s.success() => Ok(stdout),
        Some(_) => Err(if stderr.is_empty() { stdout } else { stderr }),
        None => Err(format!("perintah tidak merespons setelah {} detik", timeout.as_secs())),
    }
}

fn run_cli_capture_t(bin: &Path, args: &[&str], provider: &str, timeout: Duration) -> Result<String, String> {
    run_cli_capture_opts(bin, args, provider, timeout, None)
}

fn run_cli_capture(bin: &Path, args: &[&str], provider: &str) -> Result<String, String> {
    run_cli_capture_t(bin, args, provider, Duration::from_secs(20))
}

fn default_models(provider: &str) -> Vec<String> {
    match provider {
        "claude" => vec!["opus", "sonnet", "haiku"].into_iter().map(String::from).collect(),
        "opencode" => vec![
            "github-copilot/claude-sonnet-5.5",
            "github-copilot/claude-opus-5.5",
            "github-copilot/gemini-4-hydrogen",
            "github-copilot/gpt-5.3-codex",
            "opencode/beam-free",
            "openrouter/openrouter/auto",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        "antigravity" => vec![
            "gemini-4-hydrogen",
            "gemini-4-argon",
            "gemini-3.8-flash",
            "claude-opus-5-5",
            "claude-sonnet-5-5",
            "gpt-oss-120b-medium",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        "codex" => vec!["gpt-5.3-codex", "gpt-5.4", "o3", "o4-mini"].into_iter().map(String::from).collect(),
        "gemini" => vec!["gemini-2.5-pro", "gemini-2.5-flash", "gemini-3-flash-preview"].into_iter().map(String::from).collect(),
        _ => vec![],
    }
}

fn check_provider_auth(id: &str, bin: Option<&Path>, stored_key: &str) -> (String, bool, String) {
    let has_key = !stored_key.is_empty() || !std::env::var(provider_key_env(id)).unwrap_or_default().is_empty();
    let Some(b) = bin else {
        return (String::new(), has_key, if has_key { "API Key tersimpan".into() } else { String::new() });
    };
    let ver = run_cli_capture_t(b, &["--version"], id, Duration::from_secs(8))
        .ok()
        .and_then(|s| s.lines().next().map(|l| strip_ansi(l).trim().to_string()))
        .unwrap_or_default();

    match id {
        "claude" => {
            if let Ok(raw) = run_cli_capture_t(b, &["auth", "status"], id, Duration::from_secs(12)) {
                if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                    if v["loggedIn"].as_bool().unwrap_or(false) {
                        let email = v["email"].as_str().unwrap_or("");
                        let tier = v["subscriptionType"].as_str().or(v["authMethod"].as_str()).unwrap_or("akun");
                        let acc = if email.is_empty() { tier.to_string() } else { format!("{email} ({tier})") };
                        return (ver, true, acc);
                    }
                }
            }
            if has_key {
                return (ver, true, "API Key (Anthropic)".into());
            }
            (ver, false, String::new())
        }
        "opencode" => {
            if let Ok(raw) = run_cli_capture_t(b, &["auth", "list"], id, Duration::from_secs(12)) {
                let mut names = Vec::new();
                for line in strip_ansi(&raw).lines() {
                    let t = line.trim();
                    if t.is_empty() || t.starts_with("No ") {
                        continue;
                    }
                    let first = t.split("  ").next().unwrap_or(t).trim();
                    if !first.is_empty() {
                        names.push(first.to_string());
                    }
                }
                if !names.is_empty() {
                    let preview = if names.len() <= 2 {
                        names.join(", ")
                    } else {
                        format!("{}, {} (+{})", names[0], names[1], names.len() - 2)
                    };
                    return (ver, true, preview);
                }
            }
            if has_key {
                return (ver, true, "API Key (OpenRouter)".into());
            }
            (ver, false, String::new())
        }
        "antigravity" => {
            if has_key {
                return (ver, true, "GEMINI_API_KEY".into());
            }
            let home = home_dir();
            let oauth = home.join(".gemini").join("oauth_creds.json").exists()
                || home.join(".gemini").join("antigravity").join("user_settings.pb").exists()
                || home.join(".gemini").join("antigravity-cli").join("settings.json").exists();
            if oauth {
                return (ver, true, "Akun Google".into());
            }
            (ver, false, String::new())
        }
        "codex" => {
            if let Ok(raw) = run_cli_capture_t(b, &["login", "status"], id, Duration::from_secs(12)) {
                let clean = strip_ansi(&raw).trim().to_string();
                if !clean.is_empty() && !clean.to_lowercase().contains("not logged") {
                    return (ver, true, cc::trunc(&clean, 48));
                }
            }
            if home_dir().join(".codex").join("auth.json").exists() {
                return (ver, true, "Terhubung (~/.codex/auth.json)".into());
            }
            if has_key {
                return (ver, true, "API Key (OpenAI)".into());
            }
            (ver, false, String::new())
        }
        "gemini" => {
            if has_key {
                return (ver, true, "GEMINI_API_KEY".into());
            }
            if home_dir().join(".gemini").join("oauth_creds.json").exists() {
                return (ver, true, "Akun Google (OAuth)".into());
            }
            (ver, false, String::new())
        }
        _ => (ver, has_key, String::new()),
    }
}

fn fetch_opencode_models(bin: &Path) -> Vec<String> {
    let Ok(raw) = run_cli_capture_t(bin, &["models"], "opencode", Duration::from_secs(25)) else {
        return default_models("opencode");
    };
    let list: Vec<String> = strip_ansi(&raw)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && l.contains('/'))
        .collect();
    if list.is_empty() {
        default_models("opencode")
    } else {
        list
    }
}

/// (id, name, install hint, supports fork, supports device login)
///
/// Satu-satunya daftar provider. Provider baru juga butuh cabang di `provider_bin`,
/// `provider_key_env`, `default_models`, dan `check_provider_auth`.
const PROVIDER_DEFS: &[(&str, &str, &str, bool, bool)] = &[
    ("claude", "Claude Code", "npm i -g @anthropic-ai/claude-code", true, false),
    ("opencode", "OpenCode", "npm i -g opencode-ai", true, false),
    ("antigravity", "Antigravity", "irm https://antigravity.google/cli/install.ps1 | iex", false, false),
    ("codex", "Codex", "npm i -g @openai/codex", false, true),
    ("gemini", "Gemini CLI", "npm i -g @google/gemini-cli", false, false),
];

fn build_provider_list() -> Vec<ProviderStatus> {
    let defs = PROVIDER_DEFS;
    let (keys, cached_oc_models) = {
        let mut st = state().lock().unwrap_or_else(|e| e.into_inner());
        ensure_keys(&mut st);
        (st.keys.clone(), st.opencode_models.clone())
    };
    let now = cc::now_ms();
    // Cek tiap provider paralel: satu CLI yang lambat/timeout tidak menahan provider lain.
    std::thread::scope(|scope| {
        let handles: Vec<_> = defs
            .iter()
            .map(|&(id, name, install_hint, supports_fork, supports_device)| {
                let stored_key = keys.get(id).cloned().unwrap_or_default();
                let cached_models = cached_oc_models.clone();
                scope.spawn(move || {
                    let bin = provider_bin(id);
                    let (version, logged_in, account) = check_provider_auth(id, bin.as_deref(), &stored_key);
                    let models = if id == "opencode" {
                        if !cached_models.is_empty() {
                            cached_models
                        } else if let Some(b) = bin.as_deref() {
                            let m = fetch_opencode_models(b);
                            if let Ok(mut st) = state().lock() {
                                st.opencode_models = m.clone();
                            }
                            m
                        } else {
                            default_models(id)
                        }
                    } else {
                        default_models(id)
                    };
                    ProviderStatus {
                        id: id.into(),
                        name: name.into(),
                        installed: bin.is_some(),
                        bin: bin.map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                        version,
                        logged_in,
                        account,
                        has_api_key: !stored_key.is_empty(),
                        api_key_env: provider_key_env(id).into(),
                        install_hint: install_hint.into(),
                        supports_fork,
                        supports_device_login: supports_device,
                        models,
                        checked_at: now,
                        checking: false,
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .enumerate()
            .map(|(i, h)| match h.join() {
                Ok(p) => p,
                Err(_) => {
                    // Thread panik: tetap tampilkan kartu provider, jangan hilangkan dari daftar.
                    let (id, name, install_hint, supports_fork, supports_device) = defs[i];
                    let bin = provider_bin(id);
                    let stored_key = keys.get(id).cloned().unwrap_or_default();
                    ProviderStatus {
                        id: id.into(),
                        name: name.into(),
                        installed: bin.is_some(),
                        bin: bin.map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                        version: String::new(),
                        logged_in: false,
                        account: String::new(),
                        has_api_key: !stored_key.is_empty(),
                        api_key_env: provider_key_env(id).into(),
                        install_hint: install_hint.into(),
                        supports_fork,
                        supports_device_login: supports_device,
                        models: default_models(id),
                        checked_at: now,
                        checking: false,
                    }
                }
            })
            .collect()
    })
}

fn quick_providers_placeholder() -> Vec<ProviderStatus> {
    let keys = {
        let mut st = state().lock().unwrap_or_else(|e| e.into_inner());
        ensure_keys(&mut st);
        st.keys.clone()
    };
    PROVIDER_DEFS
        .iter()
        .copied()
        .map(|(id, name, hint, fork, dev)| {
            let bin = provider_bin(id);
            let has_key = keys.get(id).map(|k| !k.is_empty()).unwrap_or(false);
            ProviderStatus {
                id: id.into(),
                name: name.into(),
                installed: bin.is_some(),
                bin: bin.map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                version: String::new(),
                logged_in: has_key,
                account: if has_key { "API Key tersimpan".into() } else { String::new() },
                has_api_key: has_key,
                api_key_env: provider_key_env(id).into(),
                install_hint: hint.into(),
                supports_fork: fork,
                supports_device_login: dev,
                models: default_models(id),
                checked_at: 0,
                checking: false,
            }
        })
        .collect()
}

/// Daftar provider untuk snapshot. Jangan pernah memanggil fungsi ini sambil memegang
/// `state()` — `quick_providers_placeholder()` juga butuh lock yang sama dan Mutex std
/// tidak reentrant (dulu ini menyebabkan seluruh command agent menggantung).
fn providers_for_snapshot() -> Vec<ProviderStatus> {
    let (cached, checking) = {
        let Ok(st) = state().lock() else {
            return quick_providers_placeholder();
        };
        (st.providers.clone(), st.refreshing_providers)
    };
    let mut list = if cached.is_empty() {
        quick_providers_placeholder()
    } else {
        cached
    };
    for p in &mut list {
        p.checking = checking;
    }
    list
}

fn ag_lock() -> std::sync::MutexGuard<'static, AgState> {
    state().lock().unwrap_or_else(|e| e.into_inner())
}

fn trigger_provider_refresh(force: bool) {
    {
        let mut st = ag_lock();
        if st.refreshing_providers {
            return;
        }
        if !force && !st.providers.is_empty() && cc::now_ms() - st.providers_ts < 45_000 {
            return;
        }
        st.refreshing_providers = true;
    }
    std::thread::spawn(move || {
        // Jangan pernah biarkan flag "memeriksa" menyala selamanya bila pengecekan panik.
        let result = std::panic::catch_unwind(build_provider_list);
        let mut st = ag_lock();
        if let Ok(list) = result {
            st.providers = list;
            st.providers_ts = cc::now_ms();
        }
        st.refreshing_providers = false;
        drop(st);
        emit_event("providers", json!({}));
    });
}

/// Cek provider selalu asinkron: UI menerima data cache/placeholder seketika, lalu
/// diperbarui lewat event `cc-event` bertipe `providers` setelah pengecekan selesai.
pub fn providers(refresh: bool) -> Result<Value, String> {
    if refresh {
        trigger_provider_refresh(true);
    } else {
        trigger_provider_refresh(false);
    }
    let (mut list, checking) = {
        let st = ag_lock();
        (st.providers.clone(), st.refreshing_providers)
    };
    if list.is_empty() {
        list = quick_providers_placeholder();
    }
    for p in &mut list {
        p.checking = checking;
    }
    Ok(json!(list))
}

pub fn refresh_models(provider: String) -> Result<Value, String> {
    if provider != "opencode" {
        return providers(true);
    }
    let Some(b) = provider_bin("opencode") else {
        return Err("OpenCode belum terpasang".into());
    };
    let raw = run_cli_capture_t(&b, &["models", "--refresh"], "opencode", Duration::from_secs(45))
        .or_else(|_| run_cli_capture_t(&b, &["models"], "opencode", Duration::from_secs(25)))?;
    let list: Vec<String> = strip_ansi(&raw)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && l.contains('/'))
        .collect();
    if !list.is_empty() {
        if let Ok(mut st) = state().lock() {
            st.opencode_models = list.clone();
            if let Some(p) = st.providers.iter_mut().find(|p| p.id == "opencode") {
                p.models = list.clone();
            }
        }
    }
    Ok(json!(list))
}

// ======================================================================
// Install & Login di Dalam Notch
// ======================================================================
fn stream_setup_child(provider: &str, op: &str, mut cmd: Command) {
    let prov = provider.to_string();
    let operation = op.to_string();
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    sys::hide_console(&mut cmd);
    std::thread::spawn(move || {
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                emit_setup(&prov, &operation, &format!("Gagal menjalankan proses: {e}"), true, false, Some(&e.to_string()));
                return;
            }
        };
        let p_out = prov.clone();
        let o_out = operation.clone();
        let t_out = child.stdout.take().map(|s| {
            std::thread::spawn(move || {
                for l in BufReader::new(s).lines().map_while(Result::ok) {
                    let clean = strip_ansi(&l);
                    if !clean.trim().is_empty() {
                        emit_setup(&p_out, &o_out, &clean, false, false, None);
                    }
                }
            })
        });
        let p_err = prov.clone();
        let o_err = operation.clone();
        let t_err = child.stderr.take().map(|s| {
            std::thread::spawn(move || {
                for l in BufReader::new(s).lines().map_while(Result::ok) {
                    let clean = strip_ansi(&l);
                    if !clean.trim().is_empty() {
                        emit_setup(&p_err, &o_err, &clean, false, false, None);
                    }
                }
            })
        });
        if let Some(t) = t_out {
            let _ = t.join();
        }
        if let Some(t) = t_err {
            let _ = t.join();
        }
        let code = child.wait().ok().and_then(|s| s.code());
        let ok = code == Some(0);
        let list = build_provider_list();
        if let Ok(mut st) = state().lock() {
            st.providers = list;
            st.providers_ts = cc::now_ms();
        }
        emit_event("providers", json!({}));
        let err = if ok { None } else { Some("Proses keluar dengan error") };
        let msg = if ok {
            if operation == "install" { "Instalasi selesai." } else { "Proses login selesai." }
        } else {
            "Proses gagal — cek log di atas."
        };
        emit_setup(&prov, &operation, msg, true, ok, err);
    });
}

pub fn install_provider(provider: String) -> Result<(), String> {
    let cmd = match provider.as_str() {
        "claude" => {
            let mut c = Command::new("cmd");
            c.args(["/C", "npm", "install", "-g", "@anthropic-ai/claude-code"]);
            c
        }
        "opencode" => {
            let mut c = Command::new("cmd");
            c.args(["/C", "npm", "install", "-g", "opencode-ai"]);
            c
        }
        "codex" => {
            let mut c = Command::new("cmd");
            c.args(["/C", "npm", "install", "-g", "@openai/codex"]);
            c
        }
        "gemini" => {
            let mut c = Command::new("cmd");
            c.args(["/C", "npm", "install", "-g", "@google/gemini-cli"]);
            c
        }
        "antigravity" => {
            let mut c = Command::new("powershell");
            c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "irm https://antigravity.google/cli/install.ps1 | iex"]);
            c
        }
        _ => return Err("Provider tidak dikenal".into()),
    };
    emit_setup(&provider, "install", &format!("Memasang {provider}…"), false, false, None);
    stream_setup_child(&provider, "install", cmd);
    Ok(())
}

fn spawn_interactive_terminal(bin: &Path, args: &[&str], provider: &str) -> Result<(), String> {
    let bin_str = bin.to_string_lossy().to_string();
    let ext = bin.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let mut wt_args: Vec<String> = Vec::new();
    if ext == "ps1" {
        wt_args.extend(["powershell".into(), "-NoProfile".into(), "-ExecutionPolicy".into(), "Bypass".into(), "-File".into(), bin_str.clone()]);
    } else if ext == "cmd" || ext == "bat" {
        wt_args.extend(["cmd".into(), "/C".into(), bin_str.clone()]);
    } else {
        wt_args.push(bin_str.clone());
    }
    for a in args {
        wt_args.push((*a).to_string());
    }
    let wt = Command::new("wt.exe").args(&wt_args).spawn();
    if wt.is_err() {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("cmd");
        c.arg("/C").arg("start").arg(format!("Login {provider}"));
        if ext == "ps1" {
            c.args(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", &bin_str]);
        } else {
            c.arg(&bin_str);
        }
        c.args(args).creation_flags(0x10).spawn().map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn login_oauth(provider: String, mode: Option<String>) -> Result<(), String> {
    let bin = provider_bin(&provider).ok_or_else(|| format!("{provider} belum terpasang. Klik Install dulu."))?;
    let m = mode.unwrap_or_default();
    match provider.as_str() {
        "claude" => {
            emit_setup(&provider, "login", "Membuka login akun Claude (browser)…", false, false, None);
            let cmd = cli_command(&bin, &["auth", "login"]);
            stream_setup_child(&provider, "login", cmd);
            Ok(())
        }
        "codex" => {
            if m == "device" {
                emit_setup(&provider, "login", "Memulai Device Auth Codex…", false, false, None);
                let cmd = cli_command(&bin, &["login", "--device-auth"]);
                stream_setup_child(&provider, "login", cmd);
            } else {
                emit_setup(&provider, "login", "Membuka login browser Codex…", false, false, None);
                let cmd = cli_command(&bin, &["login"]);
                stream_setup_child(&provider, "login", cmd);
            }
            Ok(())
        }
        "opencode" => {
            spawn_interactive_terminal(&bin, &["auth", "login"], "OpenCode")?;
            emit_setup(&provider, "login", "Terminal login OpenCode dibuka — pilih provider lalu tekan Refresh setelah selesai.", true, true, None);
            Ok(())
        }
        "antigravity" => {
            spawn_interactive_terminal(&bin, &[], "Antigravity")?;
            emit_setup(&provider, "login", "Antigravity dibuka untuk login akun Google — tekan Refresh setelah selesai.", true, true, None);
            Ok(())
        }
        "gemini" => {
            spawn_interactive_terminal(&bin, &[], "Gemini CLI")?;
            emit_setup(&provider, "login", "Terminal Gemini CLI dibuka untuk login Google — tekan Refresh setelah selesai.", true, true, None);
            Ok(())
        }
        _ => Err("Provider tidak dikenal".into()),
    }
}

pub fn save_key(provider: String, key: String) -> Result<Value, String> {
    let clean = key.trim().to_string();
    if clean.is_empty() {
        return Err("API key tidak boleh kosong".into());
    }
    if provider == "codex" {
        if let Some(bin) = provider_bin("codex") {
            let mut cmd = cli_command(&bin, &["login", "--with-api-key"]);
            cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
            if let Ok(mut ch) = cmd.spawn() {
                if let Some(mut si) = ch.stdin.take() {
                    let _ = si.write_all(clean.as_bytes());
                }
                let _ = ch.wait();
            }
        }
    }
    {
        let mut st = state().lock().map_err(|e| e.to_string())?;
        ensure_keys(&mut st);
        st.keys.insert(provider.clone(), clean);
        save_keys_map(&st)?;
    }
    let list = build_provider_list();
    if let Ok(mut st) = state().lock() {
        st.providers = list.clone();
        st.providers_ts = cc::now_ms();
    }
    emit_event("providers", json!({}));
    Ok(json!(list))
}

pub fn logout_provider(provider: String) -> Result<Value, String> {
    if let Some(bin) = provider_bin(&provider) {
        match provider.as_str() {
            "claude" => {
                let _ = run_cli_capture(&bin, &["auth", "logout"], &provider);
            }
            "codex" => {
                let _ = run_cli_capture(&bin, &["logout"], &provider);
            }
            _ => {}
        }
    }
    {
        let mut st = state().lock().map_err(|e| e.to_string())?;
        ensure_keys(&mut st);
        st.keys.remove(&provider);
        save_keys_map(&st)?;
    }
    let list = build_provider_list();
    if let Ok(mut st) = state().lock() {
        st.providers = list.clone();
        st.providers_ts = cc::now_ms();
    }
    emit_event("providers", json!({}));
    Ok(json!(list))
}

// ======================================================================
// Scanner Sesi Provider Non-Claude (OpenCode, Codex, Gemini, Antigravity)
// ======================================================================
fn refresh_opencode_sessions_async(extra_cwds: Vec<String>) {
    let Some(bin) = provider_bin("opencode") else { return };
    let db = home_dir().join(".local").join("share").join("opencode").join("opencode.db");
    let db_mtime = file_mtime_ms(&db);
    {
        let mut st = state().lock().unwrap_or_else(|e| e.into_inner());
        for c in &extra_cwds {
            if !c.is_empty() && Path::new(c).is_dir() {
                st.opencode_cwds.insert(c.clone());
            }
        }
        if st.opencode_refreshing {
            return;
        }
        if st.opencode_db_mtime != 0 && st.opencode_db_mtime == db_mtime && !st.opencode_sessions.is_empty() {
            return;
        }
        st.opencode_refreshing = true;
    }
    std::thread::spawn(move || {
        let mut cwds: Vec<String> = {
            let st = state().lock().unwrap_or_else(|e| e.into_inner());
            st.opencode_cwds.iter().cloned().collect()
        };
        if let Ok(cur) = std::env::current_dir() {
            let s = cur.to_string_lossy().to_string();
            if !cwds.contains(&s) {
                cwds.push(s);
            }
        }
        let home = home_dir().to_string_lossy().to_string();
        if !cwds.contains(&home) {
            cwds.push(home);
        }
        let mut seen = HashSet::new();
        let mut found = Vec::new();
        for dir in cwds.into_iter().take(6) {
            if !Path::new(&dir).is_dir() {
                continue;
            }
            let Ok(raw) = run_cli_capture_opts(
                &bin,
                &["session", "list", "--format", "json", "-n", "40"],
                "opencode",
                Duration::from_secs(20),
                Some(Path::new(&dir)),
            ) else {
                continue;
            };
            let Ok(arr) = serde_json::from_str::<Vec<Value>>(&raw) else { continue };
            for item in arr {
                let id = item["id"].as_str().unwrap_or("").to_string();
                if id.is_empty() || !seen.insert(id.clone()) {
                    continue;
                }
                let cwd = item["directory"].as_str().unwrap_or(&dir).to_string();
                let title = item["title"].as_str().unwrap_or("").to_string();
                let created = item["created"].as_i64().unwrap_or(0);
                let updated = item["updated"].as_i64().unwrap_or(created);
                found.push(CachedSession {
                    id,
                    provider: "opencode".into(),
                    title: if title.is_empty() { "Sesi OpenCode".into() } else { title },
                    prompt: String::new(),
                    project: cc::base(&cwd),
                    cwd,
                    start: created,
                    last: updated,
                    ver: updated.max(1) as u64,
                    ..Default::default()
                });
            }
        }
        if let Ok(mut st) = state().lock() {
            st.opencode_sessions = found;
            st.opencode_db_mtime = db_mtime.max(1);
            st.opencode_refreshing = false;
        }
    });
}

fn parse_opencode_export(id: &str) -> Result<(Vec<Value>, u64, u64, u64, String), String> {
    let bin = provider_bin("opencode").ok_or("OpenCode belum terpasang")?;
    let raw = run_cli_capture_t(&bin, &["session", "export", id], "opencode", Duration::from_secs(25))?;
    let v: Value = serde_json::from_str(&raw).map_err(|e| format!("Gagal membaca export OpenCode: {e}"))?;
    let mut items = Vec::new();
    let mut seq = 0u64;
    let info = &v["info"];
    let model = info["model"]["id"].as_str().unwrap_or("").to_string();
    let tok_in = info["tokens"]["input"].as_u64().unwrap_or(0);
    let tok_out = info["tokens"]["output"].as_u64().unwrap_or(0);
    let tok_cache = info["tokens"]["cache"]["read"].as_u64().unwrap_or(0);
    if let Some(msgs) = v["messages"].as_array() {
        for m in msgs {
            let ty = m["type"].as_str().unwrap_or("");
            let ts = m["time"]["created"].as_i64().unwrap_or(0);
            if ty == "user" {
                let text = m["text"].as_str().unwrap_or("").trim();
                if !text.is_empty() {
                    seq += 1;
                    items.push(json!({"seq": seq, "ts": ts, "k": "user", "text": cc::trunc(text, 6000)}));
                }
            } else if ty == "assistant" {
                if let Some(parts) = m["content"].as_array() {
                    for p in parts {
                        match p["type"].as_str().unwrap_or("") {
                            "text" => {
                                let t = p["text"].as_str().unwrap_or("").trim();
                                if !t.is_empty() {
                                    seq += 1;
                                    items.push(json!({"seq": seq, "ts": ts, "k": "text", "text": cc::trunc(t, 12000)}));
                                }
                            }
                            "tool" => {
                                let name = p["name"].as_str().unwrap_or("tool");
                                let inp = p["state"]["input"].clone();
                                let status = p["state"]["status"].as_str().unwrap_or("completed");
                                let out_txt = p["state"]["content"]
                                    .as_array()
                                    .map(|a| a.iter().filter_map(|x| x["text"].as_str()).collect::<Vec<_>>().join("\n"))
                                    .unwrap_or_default();
                                let t_start = p["time"]["created"].as_i64().unwrap_or(ts);
                                let t_end = p["time"]["completed"].as_i64().unwrap_or(t_start);
                                seq += 1;
                                items.push(json!({
                                    "seq": seq,
                                    "ts": t_start,
                                    "end": t_end,
                                    "k": "tool",
                                    "id": p["id"],
                                    "name": name,
                                    "sum": cc::tool_summary(name, &inp),
                                    "input": cc::shrink(&inp, 4000),
                                    "st": if status == "error" || status == "failed" { "err" } else if status == "running" || status == "pending" { "run" } else { "ok" },
                                    "out": cc::trunc(&out_txt, 2500),
                                }));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    Ok((items, tok_in, tok_out, tok_cache, model))
}

fn walk_files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>, depth: u8) {
    if depth > 4 {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_files(&p, ext, out, depth + 1);
        } else if p.extension().and_then(|x| x.to_str()) == Some(ext) {
            out.push(p);
        }
    }
}

fn scan_codex_sessions(since_cutoff: i64) -> Vec<CachedSession> {
    let root = home_dir().join(".codex").join("sessions");
    if !root.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    walk_files(&root, "jsonl", &mut files, 0);
    let mut out = Vec::new();
    for p in files {
        let mtime = file_mtime_ms(&p);
        if mtime < since_cutoff {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&p) else { continue };
        let mut s = CachedSession {
            provider: "codex".into(),
            path: Some(p.clone()),
            last: mtime,
            ver: mtime.max(1) as u64,
            ..Default::default()
        };
        let mut seq = 0u64;
        for line in raw.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            let ty = v["type"].as_str().unwrap_or("");
            if ty == "session_meta" {
                let pl = &v["payload"];
                if let Some(id) = pl["id"].as_str() {
                    s.id = id.to_string();
                }
                if let Some(cwd) = pl["cwd"].as_str() {
                    s.cwd = cwd.to_string();
                    s.project = cc::base(cwd);
                }
            } else if ty == "event_msg" {
                let pl = &v["payload"];
                match pl["type"].as_str().unwrap_or("") {
                    "user_message" => {
                        if let Some(msg) = pl["message"].as_str() {
                            seq += 1;
                            s.items.push(json!({"seq": seq, "ts": mtime, "k": "user", "text": cc::trunc(msg, 6000)}));
                            if s.title.is_empty() {
                                s.title = cc::trunc(msg, 80);
                            }
                            s.prompt = cc::trunc(msg, 240);
                        }
                    }
                    "agent_message" => {
                        if let Some(msg) = pl["message"].as_str() {
                            seq += 1;
                            s.items.push(json!({"seq": seq, "ts": mtime, "k": "text", "text": cc::trunc(msg, 12000)}));
                        }
                    }
                    _ => {}
                }
            } else if ty == "turn_context" {
                if let Some(m) = v["payload"]["model"].as_str() {
                    s.model = m.to_string();
                }
            }
        }
        if s.id.is_empty() {
            s.id = p.file_stem().and_then(|x| x.to_str()).unwrap_or("codex").to_string();
        }
        if s.title.is_empty() {
            s.title = "Sesi Codex".into();
        }
        out.push(s);
    }
    out
}

fn scan_gemini_sessions(since_cutoff: i64) -> Vec<CachedSession> {
    let root = home_dir().join(".gemini").join("tmp");
    if !root.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    walk_files(&root, "json", &mut files, 0);
    let mut out = Vec::new();
    for p in files {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.starts_with("session-") {
            continue;
        }
        let mtime = file_mtime_ms(&p);
        if mtime < since_cutoff {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&p) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&raw) else { continue };
        let id = v["sessionId"].as_str().unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }
        let mut s = CachedSession {
            id,
            provider: "gemini".into(),
            path: Some(p),
            last: mtime,
            ver: mtime.max(1) as u64,
            ..Default::default()
        };
        let mut seq = 0u64;
        if let Some(msgs) = v["messages"].as_array() {
            for m in msgs {
                let ty = m["type"].as_str().unwrap_or("");
                let content = m["content"].as_str().unwrap_or("").trim();
                if ty == "user" && !content.is_empty() {
                    seq += 1;
                    s.items.push(json!({"seq": seq, "ts": mtime, "k": "user", "text": cc::trunc(content, 6000)}));
                    if s.title.is_empty() {
                        s.title = cc::trunc(content, 80);
                    }
                    s.prompt = cc::trunc(content, 240);
                } else if (ty == "gemini" || ty == "model") && !content.is_empty() {
                    seq += 1;
                    s.items.push(json!({"seq": seq, "ts": mtime, "k": "text", "text": cc::trunc(content, 12000)}));
                }
            }
        }
        if s.title.is_empty() {
            s.title = "Sesi Gemini".into();
        }
        out.push(s);
    }
    out
}

fn scan_antigravity_sessions(since_cutoff: i64) -> Vec<CachedSession> {
    let root = home_dir().join(".gemini").join("antigravity").join("brain");
    if !root.is_dir() {
        return Vec::new();
    }
    let Ok(dirs) = fs::read_dir(&root) else { return Vec::new() };
    let mut out = Vec::new();
    for d in dirs.flatten() {
        let p = d.path();
        if !p.is_dir() {
            continue;
        }
        let id = d.file_name().to_string_lossy().to_string();
        if id.starts_with('.') {
            continue;
        }
        let mtime = file_mtime_ms(&p);
        if mtime < since_cutoff {
            continue;
        }
        let mut title = String::new();
        let mut items = Vec::new();
        let mut seq = 0u64;
        for fname in ["task.md", "implementation_plan.md", "walkthrough.md"] {
            let fp = p.join(fname);
            if let Ok(txt) = fs::read_to_string(&fp) {
                if title.is_empty() {
                    if let Some(first) = txt.lines().find(|l| !l.trim().is_empty()) {
                        title = cc::trunc(first.trim().trim_start_matches('#').trim(), 80);
                    }
                }
                seq += 1;
                items.push(json!({"seq": seq, "ts": mtime, "k": "text", "text": format!("**{fname}**\n\n{}", cc::trunc(&txt, 8000))}));
            }
        }
        if title.is_empty() {
            title = format!("Antigravity · {}", &id[..id.len().min(8)]);
        }
        out.push(CachedSession {
            id,
            provider: "antigravity".into(),
            title,
            project: "Antigravity".into(),
            last: mtime,
            ver: mtime.max(1) as u64,
            path: Some(p),
            items,
            ..Default::default()
        });
    }
    out
}

// ======================================================================
// Snapshot Terpadu & Timeline
// ======================================================================
pub fn snapshot(since_ms: i64) -> Result<Value, String> {
    let mut base_snap = cc::snapshot(since_ms)?;
    let now = cc::now_ms();
    let cutoff = now - 3 * 24 * 3600 * 1000;

    let mut cwds = Vec::new();
    if let Some(arr) = base_snap["sessions"].as_array() {
        for s in arr {
            if let Some(c) = s["cwd"].as_str() {
                if !c.is_empty() {
                    cwds.push(c.to_string());
                }
            }
        }
    }
    refresh_opencode_sessions_async(cwds);

    let (oc_sessions, runs, titles) = {
        let mut st = state().lock().map_err(|e| e.to_string())?;
        ensure_titles(&mut st);
        (st.opencode_sessions.clone(), st.runs.clone(), st.titles.clone())
    };
    // PENTING: panggil setelah lock dilepas — quick_providers_placeholder ikut mengunci state().
    let provs = providers_for_snapshot();
    if provs.iter().all(|p| p.checked_at == 0) {
        trigger_provider_refresh(false);
    }

    let mut extra_sessions: HashMap<(String, String), Value> = HashMap::new();
    let mut insert_cached = |s: CachedSession| {
        let custom_title = titles.get(&title_key(&s.provider, &s.id)).cloned();
        let title = custom_title.unwrap_or(s.title);
        let key = (s.provider.clone(), s.id.clone());
        let val = json!({
            "id": s.id,
            "provider": s.provider,
            "path": s.path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
            "title": title,
            "prompt": s.prompt,
            "cwd": s.cwd,
            "project": if s.project.is_empty() { cc::base(&s.cwd) } else { s.project },
            "branch": s.branch,
            "model": s.model,
            "version": "",
            "entry": s.provider,
            "start": s.start,
            "turn": s.last,
            "last": s.last,
            "ver": s.ver,
            "tok": {"in": s.tok_in, "out": s.tok_out, "cache": s.tok_cache},
            "state": "done",
            "activity": "",
            "note": "",
            "mode": "",
            "pid": 0,
            "proc": Value::Null,
            "managed": false,
            "managed_live": false,
            "pending": 0,
            "hooked": false,
            "subagent": false,
        });
        extra_sessions.insert(key, val);
    };

    for s in oc_sessions {
        if s.last >= cutoff {
            insert_cached(s);
        }
    }
    for s in scan_codex_sessions(cutoff) {
        insert_cached(s);
    }
    for s in scan_gemini_sessions(cutoff) {
        insert_cached(s);
    }
    for s in scan_antigravity_sessions(cutoff) {
        insert_cached(s);
    }

    // Gabungkan run aktif/terbaru dari provider non-Claude agar tampil real-time di list & pill.
    for r in runs.iter() {
        let sid = if r.session.is_empty() { r.run_id.clone() } else { r.session.clone() };
        let key = (r.provider.clone(), sid.clone());
        let title = titles
            .get(&title_key(&r.provider, &sid))
            .cloned()
            .unwrap_or_else(|| cc::trunc(&r.prompt, 80));
        let st_str = if !r.done { "running" } else { "done" };
        let entry = json!({
            "id": sid,
            "provider": r.provider,
            "path": "",
            "title": title,
            "prompt": r.prompt,
            "cwd": r.cwd,
            "project": cc::base(&r.cwd),
            "branch": "",
            "model": r.model,
            "version": "",
            "entry": "Notch",
            "start": r.started,
            "turn": r.started,
            "last": if r.done { r.started } else { r.started.max(now) },
            "ver": r.ver.max(1),
            "tok": {"in": r.tok_in, "out": r.tok_out, "cache": r.tok_cache},
            "state": st_str,
            "activity": if !r.done { r.activity.clone() } else { String::new() },
            "note": "",
            "mode": "",
            "pid": if !r.done { r.pid } else { 0 },
            "proc": Value::Null,
            "managed": true,
            "managed_live": !r.done,
            "pending": 0,
            "hooked": false,
            "subagent": false,
        });
        extra_sessions.insert(key, entry);
    }

    let extra_today_sessions = extra_sessions
        .values()
        .filter(|s| s["last"].as_i64().unwrap_or(0) >= since_ms)
        .count() as u64;
    let extra_today_tokens: u64 = extra_sessions
        .values()
        .filter(|s| s["last"].as_i64().unwrap_or(0) >= since_ms)
        .map(|s| s["tok"]["in"].as_u64().unwrap_or(0) + s["tok"]["out"].as_u64().unwrap_or(0))
        .sum();
    if let Some(td) = base_snap.get_mut("today").and_then(|t| t.as_object_mut()) {
        let cur_s = td.get("sessions").and_then(|v| v.as_u64()).unwrap_or(0);
        let cur_t = td.get("tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        td.insert("sessions".into(), json!(cur_s + extra_today_sessions));
        td.insert("tokens".into(), json!(cur_t + extra_today_tokens));
    }

    let mut all_sessions = base_snap["sessions"].as_array().cloned().unwrap_or_default();
    all_sessions.extend(extra_sessions.into_values());
    all_sessions.sort_by(|a, b| b["last"].as_i64().unwrap_or(0).cmp(&a["last"].as_i64().unwrap_or(0)));
    all_sessions.truncate(80);
    base_snap["sessions"] = Value::Array(all_sessions);

    let mut all_runs = base_snap["runs"].as_array().cloned().unwrap_or_default();
    for r in runs.iter().rev().take(10) {
        all_runs.push(json!({
            "run": r.run_id, "provider": r.provider, "pid": r.pid, "session": r.session, "cwd": r.cwd,
            "prompt": cc::trunc(&r.prompt, 160), "started": r.started, "done": r.done, "exit": r.exit, "error": r.error,
        }));
    }
    base_snap["runs"] = Value::Array(all_runs);
    base_snap["providers"] = json!(provs);
    Ok(base_snap)
}

pub fn session(provider: String, id: String, ver: u64) -> Result<Value, String> {
    if provider == "claude" {
        return cc::session(id, ver);
    }
    // Cek run aktif/baru di memori dulu
    {
        let st = state().lock().map_err(|e| e.to_string())?;
        if let Some(r) = st.runs.iter().rev().find(|r| r.provider == provider && (r.session == id || r.run_id == id)) {
            if !r.done || r.items.len() > 0 {
                if r.ver == ver && ver != 0 && provider != "opencode" {
                    return Ok(Value::Null);
                }
                if !r.done {
                    return Ok(json!({"id": id, "ver": r.ver, "items": r.items}));
                }
            }
        }
    }
    match provider.as_str() {
        "opencode" => {
            let target_ver = {
                let st = state().lock().map_err(|e| e.to_string())?;
                st.opencode_sessions.iter().find(|s| s.id == id).map(|s| s.ver).unwrap_or(1)
            };
            if target_ver == ver && ver != 0 {
                return Ok(Value::Null);
            }
            if let Some(cached) = {
                let st = state().lock().map_err(|e| e.to_string())?;
                st.export_cache.get(&id).filter(|c| c.0 == target_ver as i64).cloned()
            } {
                return Ok(json!({"id": id, "ver": target_ver, "items": cached.1}));
            }
            let parsed = parse_opencode_export(&id)?;
            if let Ok(mut st) = state().lock() {
                st.export_cache.insert(id.clone(), (target_ver as i64, parsed.0.clone(), parsed.1, parsed.2, parsed.3, parsed.4));
            }
            Ok(json!({"id": id, "ver": target_ver, "items": parsed.0}))
        }
        "codex" => {
            let list = scan_codex_sessions(0);
            let Some(s) = list.into_iter().find(|s| s.id == id) else {
                return Err("Sesi Codex tidak ditemukan".into());
            };
            if s.ver == ver && ver != 0 {
                return Ok(Value::Null);
            }
            Ok(json!({"id": id, "ver": s.ver, "items": s.items}))
        }
        "gemini" => {
            let list = scan_gemini_sessions(0);
            let Some(s) = list.into_iter().find(|s| s.id == id) else {
                return Err("Sesi Gemini tidak ditemukan".into());
            };
            if s.ver == ver && ver != 0 {
                return Ok(Value::Null);
            }
            Ok(json!({"id": id, "ver": s.ver, "items": s.items}))
        }
        "antigravity" => {
            let st = state().lock().map_err(|e| e.to_string())?;
            if let Some(r) = st.runs.iter().rev().find(|r| r.provider == "antigravity" && (r.session == id || r.run_id == id)) {
                if r.ver == ver && ver != 0 {
                    return Ok(Value::Null);
                }
                return Ok(json!({"id": id, "ver": r.ver, "items": r.items}));
            }
            drop(st);
            let list = scan_antigravity_sessions(0);
            let Some(s) = list.into_iter().find(|s| s.id == id) else {
                return Err("Sesi Antigravity tidak ditemukan".into());
            };
            if s.ver == ver && ver != 0 {
                return Ok(Value::Null);
            }
            Ok(json!({"id": id, "ver": s.ver, "items": s.items}))
        }
        _ => Err("Provider tidak dikenal".into()),
    }
}

// ======================================================================
// Rename, Hapus, Stop, Buka
// ======================================================================
pub fn rename(provider: String, id: String, title: String) -> Result<(), String> {
    if provider == "claude" {
        return cc::rename(id, title);
    }
    let clean: String = title.replace(['\r', '\n'], " ").split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.is_empty() {
        return Err("Nama sesi tidak boleh kosong".into());
    }
    {
        let mut st = state().lock().map_err(|e| e.to_string())?;
        ensure_titles(&mut st);
        st.titles.insert(title_key(&provider, &id), clean.clone());
        save_titles_map(&st)?;
    }
    emit_event("renamed", json!({"provider": provider, "session": id, "title": clean}));
    Ok(())
}

pub fn delete_session(provider: String, id: String) -> Result<(), String> {
    if provider == "claude" {
        return cc::delete_session(id);
    }
    {
        let st = state().lock().map_err(|e| e.to_string())?;
        if st.runs.iter().any(|r| r.provider == provider && (r.session == id || r.run_id == id) && !r.done) {
            return Err("Sesi masih berjalan. Stop dulu sebelum menghapus.".into());
        }
    }
    match provider.as_str() {
        "opencode" => {
            if let Some(bin) = provider_bin("opencode") {
                run_cli_capture(&bin, &["session", "delete", &id], "opencode")?;
            }
            if let Ok(mut st) = state().lock() {
                st.opencode_sessions.retain(|s| s.id != id);
                st.opencode_db_mtime = 0;
                st.export_cache.remove(&id);
            }
        }
        "codex" => {
            if let Some(s) = scan_codex_sessions(0).into_iter().find(|s| s.id == id) {
                if let Some(p) = s.path {
                    fs::remove_file(p).map_err(|e| e.to_string())?;
                }
            }
        }
        "gemini" => {
            if let Some(s) = scan_gemini_sessions(0).into_iter().find(|s| s.id == id) {
                if let Some(p) = s.path {
                    fs::remove_file(p).map_err(|e| e.to_string())?;
                }
            }
        }
        "antigravity" => {
            // Hapus catatan run lokal Nimbus untuk sesi ini; folder brain hanya dihapus jika bukan sesi aktif
            if let Ok(mut st) = state().lock() {
                st.runs.retain(|r| !(r.provider == "antigravity" && (r.session == id || r.run_id == id)));
            }
        }
        _ => return Err("Provider tidak dikenal".into()),
    }
    if let Ok(mut st) = state().lock() {
        ensure_titles(&mut st);
        st.titles.remove(&title_key(&provider, &id));
        let _ = save_titles_map(&st);
        st.runs.retain(|r| !(r.provider == provider && (r.session == id || r.run_id == id) && r.done));
    }
    emit_event("deleted", json!({"provider": provider, "session": id}));
    Ok(())
}

pub fn kill(provider: String, pid: u32) -> Result<(), String> {
    if provider == "claude" {
        return cc::kill(pid);
    }
    if pid == 0 {
        return Err("PID proses tidak ditemukan".into());
    }
    win::kill_tree(pid)?;
    if let Ok(mut st) = state().lock() {
        for r in st.runs.iter_mut() {
            if r.pid == pid {
                r.done = true;
                r.activity = "Dihentikan dari notch".into();
                r.ver += 1;
            }
        }
    }
    emit_event("killed", json!({"provider": provider, "pid": pid}));
    Ok(())
}

pub fn open(provider: String, kind: String, cwd: String, session: Option<String>) -> Result<(), String> {
    if provider == "claude" || kind == "folder" || kind == "vscode" {
        return cc::open(kind, cwd, session);
    }
    if !Path::new(&cwd).is_dir() {
        return Err(format!("Folder tidak ditemukan: {cwd}"));
    }
    let bin = provider_bin(&provider).ok_or_else(|| format!("{provider} belum terpasang"))?;
    let mut args: Vec<String> = Vec::new();
    let sid = session.filter(|s| !s.is_empty());
    match provider.as_str() {
        "opencode" => {
            if let Some(s) = sid {
                args.extend(["-s".into(), s]);
            }
        }
        "antigravity" => {
            if let Some(s) = sid {
                args.extend(["--conversation".into(), s]);
            }
        }
        "codex" => {
            if let Some(s) = sid {
                args.extend(["resume".into(), s]);
            }
        }
        "gemini" => {
            if let Some(s) = sid {
                args.extend(["--resume".into(), s]);
            }
        }
        _ => return Err("Provider tidak mendukung terminal resume".into()),
    }
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let bin_str = bin.to_string_lossy().to_string();
    let ext = bin.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let mut wt = Command::new("wt.exe");
    wt.arg("-d").arg(&cwd);
    if ext == "ps1" {
        wt.args(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", &bin_str]);
    } else if ext == "cmd" || ext == "bat" {
        wt.args(["cmd", "/C", &bin_str]);
    } else {
        wt.arg(&bin_str);
    }
    wt.args(&arg_refs);
    if wt.spawn().is_err() {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("cmd");
        c.arg("/C").arg("start").arg(provider).arg(&bin_str).args(&arg_refs).current_dir(&cwd).creation_flags(0x10);
        c.spawn().map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ======================================================================
// Eksekusi Headless Multi-Provider + Live Streaming Event
// ======================================================================
#[allow(clippy::too_many_arguments)]
pub fn run(
    provider: String,
    cwd: String,
    prompt: String,
    resume: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    mode: Option<String>,
    name: Option<String>,
    fork: Option<bool>,
    attachments: Option<Vec<String>>,
) -> Result<String, String> {
    if provider == "claude" {
        return cc::run(cwd, prompt, resume, model, effort, mode, name, fork, attachments);
    }
    let bin = provider_bin(&provider).ok_or_else(|| format!("{provider} belum terpasang. Buka menu Providers untuk install."))?;
    let cwd = cwd.trim().to_string();
    if cwd.is_empty() || !Path::new(&cwd).is_dir() {
        return Err(format!("Folder tidak ditemukan: {cwd}"));
    }
    let files: Vec<String> = attachments
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && Path::new(s).exists())
        .collect();
    let body = prompt.trim().to_string();
    if body.is_empty() && files.is_empty() {
        return Err("Prompt atau lampiran kosong".into());
    }
    let full_prompt = if files.is_empty() || provider == "opencode" {
        if body.is_empty() { "Tolong periksa lampiran berikut.".into() } else { body.clone() }
    } else {
        let list = files.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n");
        let text = if body.is_empty() { "Tolong periksa lampiran berikut." } else { body.as_str() };
        format!("[Lampiran]\n{list}\n\n{text}")
    };

    let mut args: Vec<String> = Vec::new();
    let mut use_stdin_prompt = false;
    let model_clean = model.as_deref().filter(|s| !s.is_empty() && *s != "default");
    let effort_clean = effort.as_deref().filter(|s| !s.is_empty() && *s != "default");
    let mode_clean = mode.as_deref().filter(|s| !s.is_empty() && *s != "default");
    let res_clean = resume.as_deref().filter(|s| !s.is_empty());

    match provider.as_str() {
        "opencode" => {
            args.extend(["run".into(), "--format".into(), "json".into()]);
            if let Some(r) = res_clean {
                args.extend(["-s".into(), r.into()]);
                if fork.unwrap_or(false) {
                    args.push("--fork".into());
                }
            } else if let Some(n) = name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                args.extend(["--title".into(), n.into()]);
            }
            if let Some(m) = model_clean {
                let m_full = if let Some(ef) = effort_clean {
                    if m.contains('#') { m.to_string() } else { format!("{m}#{ef}") }
                } else {
                    m.to_string()
                };
                args.extend(["-m".into(), m_full]);
            }
            if matches!(mode_clean, Some("auto" | "acceptEdits" | "dontAsk" | "bypassPermissions")) {
                args.push("--auto".into());
            }
            for f in &files {
                args.extend(["-f".into(), f.clone()]);
            }
            args.push(full_prompt.clone());
        }
        "antigravity" => {
            args.extend(["-p".into(), full_prompt.clone(), "--output-format".into(), "stream-json".into()]);
            if let Some(r) = res_clean {
                args.extend(["--conversation".into(), r.into()]);
            }
            if let Some(m) = model_clean {
                args.extend(["--model".into(), m.into()]);
            }
            if let Some(ef) = effort_clean {
                args.extend(["--effort".into(), ef.into()]);
            }
            match mode_clean {
                Some("acceptEdits" | "accept-edits") => args.extend(["--mode".into(), "accept-edits".into()]),
                Some("auto") => args.extend(["--mode".into(), "auto".into()]),
                Some("plan") => args.extend(["--mode".into(), "plan".into()]),
                Some("dontAsk" | "bypass" | "bypassPermissions") => args.push("--dangerously-skip-permissions".into()),
                _ => {}
            }
            for f in &files {
                if let Some(p) = Path::new(f).parent() {
                    args.extend(["--add-dir".into(), p.to_string_lossy().to_string()]);
                }
            }
        }
        "codex" => {
            args.push("exec".into());
            if let Some(r) = res_clean {
                args.extend(["resume".into(), r.into()]);
            }
            args.push("--json".into());
            args.push("--skip-git-repo-check".into());
            if res_clean.is_none() {
                args.extend(["-C".into(), cwd.clone()]);
            }
            if let Some(m) = model_clean {
                args.extend(["-m".into(), m.into()]);
            }
            match mode_clean {
                Some("auto" | "acceptEdits" | "full-auto") => args.push("--full-auto".into()),
                Some("dontAsk" | "bypassPermissions") => args.push("--dangerously-bypass-approvals-and-sandbox".into()),
                _ => {}
            }
            args.push("-".into());
            use_stdin_prompt = true;
        }
        "gemini" => {
            args.extend(["-p".into(), full_prompt.clone(), "--output-format".into(), "stream-json".into(), "--skip-trust".into()]);
            if let Some(r) = res_clean {
                args.extend(["--resume".into(), r.into()]);
            }
            if let Some(m) = model_clean {
                args.extend(["-m".into(), m.into()]);
            }
            match mode_clean {
                Some("acceptEdits" | "auto_edit") => args.extend(["--approval-mode".into(), "auto_edit".into()]),
                Some("auto" | "dontAsk" | "yolo" | "bypassPermissions") => args.extend(["--approval-mode".into(), "yolo".into()]),
                Some("plan") => args.extend(["--approval-mode".into(), "plan".into()]),
                _ => {}
            }
        }
        _ => return Err("Provider tidak dikenal".into()),
    }

    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let mut cmd = cli_command(&bin, &arg_refs);
    cmd.current_dir(&cwd)
        .env("NOTCH_MANAGED", "1")
        .stdin(if use_stdin_prompt { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_provider_env(&mut cmd, &provider);

    let mut child = cmd.spawn().map_err(|e| format!("Gagal menjalankan {provider}: {e}"))?;
    if use_stdin_prompt {
        if let Some(mut si) = child.stdin.take() {
            let _ = si.write_all(full_prompt.as_bytes());
        }
    }

    let now = cc::now_ms();
    let run_id = format!("run-{provider}-{now}");
    let init_sid = if fork.unwrap_or(false) { String::new() } else { res_clean.unwrap_or("").to_string() };
    let initial_items = vec![json!({"seq": 1, "ts": now, "k": "user", "text": cc::trunc(&full_prompt, 6000)})];
    {
        let mut st = state().lock().map_err(|e| e.to_string())?;
        st.seq += 1;
        st.runs.push(ManagedRun {
            run_id: run_id.clone(),
            provider: provider.clone(),
            pid: child.id(),
            session: init_sid.clone(),
            cwd: cwd.clone(),
            model: model_clean.unwrap_or("").to_string(),
            prompt: full_prompt.clone(),
            started: now,
            activity: "Berpikir…".into(),
            items: initial_items,
            ver: 1,
            ..Default::default()
        });
        if st.runs.len() > 40 {
            st.runs.remove(0);
        }
    }

    emit_event("run", json!({"provider": provider, "run": run_id, "state": "start", "session": init_sid}));

    let rid = run_id.clone();
    let prov = provider.clone();
    std::thread::spawn(move || {
        let err_lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let et = child.stderr.take().map(|s| {
            let el = err_lines.clone();
            std::thread::spawn(move || {
                for l in BufReader::new(s).lines().map_while(Result::ok) {
                    let c = strip_ansi(&l);
                    if !c.trim().is_empty() {
                        if let Ok(mut g) = el.lock() {
                            g.push(c);
                            if g.len() > 20 {
                                g.remove(0);
                            }
                        }
                    }
                }
            })
        });
        if let Some(out) = child.stdout.take() {
            let mut emitted_init = false;
            for l in BufReader::new(out).lines().map_while(Result::ok) {
                let clean = l.trim_start_matches('\u{feff}').trim();
                if clean.is_empty() {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<Value>(clean) else { continue };
                let mut st = match state().lock() {
                    Ok(g) => g,
                    Err(_) => continue,
                };
                let Some(r) = st.runs.iter_mut().find(|r| r.run_id == rid) else { continue };
                // Deteksi session ID dari format tiap provider
                let detected_sid = v["sessionID"]
                    .as_str()
                    .or_else(|| v["session_id"].as_str())
                    .or_else(|| v["thread_id"].as_str())
                    .or_else(|| v["conversation_id"].as_str())
                    .map(String::from);
                if let Some(sid) = detected_sid {
                    if !sid.is_empty() && r.session != sid {
                        r.session = sid.clone();
                        if !emitted_init {
                            emitted_init = true;
                            drop(st);
                            emit_event("run", json!({"provider": prov, "run": rid, "state": "init", "session": sid}));
                            st = state().lock().unwrap_or_else(|e| e.into_inner());
                        }
                    }
                }
                let Some(r) = st.runs.iter_mut().find(|r| r.run_id == rid) else { continue };
                let ts = cc::now_ms();
                let mut next_seq = (r.items.len() as u64) + 1;
                let ty = v["type"].as_str().unwrap_or("");
                match prov.as_str() {
                    "opencode" => {
                        let p = &v["part"];
                        if ty == "text" {
                            let t = p["text"].as_str().or_else(|| v["text"].as_str()).unwrap_or("").trim();
                            if !t.is_empty() {
                                r.items.push(json!({"seq": next_seq, "ts": ts, "k": "text", "text": cc::trunc(t, 12000)}));
                                r.activity = cc::trunc(t.lines().next().unwrap_or(""), 100);
                                r.ver += 1;
                            }
                        } else if ty == "tool_use" {
                            let name = p["tool"].as_str().unwrap_or("tool");
                            let inp = p["state"]["input"].clone();
                            let out_t = p["state"]["output"].as_str().unwrap_or("");
                            let status = p["state"]["status"].as_str().unwrap_or("completed");
                            let sum = cc::tool_summary(name, &inp);
                            r.activity = format!("{name} · {sum}");
                            r.items.push(json!({
                                "seq": next_seq, "ts": ts, "k": "tool", "name": name, "sum": sum,
                                "input": cc::shrink(&inp, 4000),
                                "st": if status == "error" { "err" } else if status == "running" { "run" } else { "ok" },
                                "out": cc::trunc(out_t, 2500),
                            }));
                            r.ver += 1;
                        }
                    }
                    "codex" => {
                        if ty == "item.completed" {
                            let it = &v["item"];
                            match it["type"].as_str().unwrap_or("") {
                                "agent_message" => {
                                    let t = it["text"].as_str().unwrap_or("").trim();
                                    if !t.is_empty() {
                                        r.items.push(json!({"seq": next_seq, "ts": ts, "k": "text", "text": cc::trunc(t, 12000)}));
                                        r.activity = cc::trunc(t.lines().next().unwrap_or(""), 100);
                                        r.ver += 1;
                                    }
                                }
                                "command_execution" => {
                                    let cmd_s = it["command"].as_str().unwrap_or("");
                                    let exit = it["exit_code"].as_i64().unwrap_or(0);
                                    r.items.push(json!({
                                        "seq": next_seq, "ts": ts, "k": "tool", "name": "Bash",
                                        "sum": cc::trunc(cmd_s, 120), "input": json!({"command": cmd_s}),
                                        "st": if exit == 0 { "ok" } else { "err" },
                                        "out": cc::trunc(it["aggregated_output"].as_str().unwrap_or(""), 2500),
                                    }));
                                    r.ver += 1;
                                }
                                _ => {}
                            }
                        } else if ty == "turn.completed" {
                            r.tok_in += v["usage"]["input_tokens"].as_u64().unwrap_or(0);
                            r.tok_out += v["usage"]["output_tokens"].as_u64().unwrap_or(0);
                            r.tok_cache += v["usage"]["cached_input_tokens"].as_u64().unwrap_or(0);
                        }
                    }
                    "gemini" | "antigravity" => {
                        if ty == "message" && v["role"].as_str() != Some("user") {
                            let t = v["content"].as_str().or_else(|| v["text"].as_str()).unwrap_or("").trim();
                            if !t.is_empty() {
                                if let Some(last) = r.items.last_mut().filter(|it| it["k"] == "text" && v["delta"].as_bool().unwrap_or(false)) {
                                    let cur = last["text"].as_str().unwrap_or("");
                                    last["text"] = json!(cc::trunc(&format!("{cur}{t}"), 12000));
                                } else {
                                    r.items.push(json!({"seq": next_seq, "ts": ts, "k": "text", "text": cc::trunc(t, 12000)}));
                                }
                                r.activity = cc::trunc(t.lines().next().unwrap_or(""), 100);
                                r.ver += 1;
                            }
                        } else if ty == "tool_use" {
                            let name = v["tool_name"].as_str().or_else(|| v["name"].as_str()).unwrap_or("tool");
                            let inp = v["parameters"].clone();
                            let sum = cc::tool_summary(name, &inp);
                            r.activity = format!("{name} · {sum}");
                            r.items.push(json!({
                                "seq": next_seq, "ts": ts, "k": "tool", "id": v["tool_id"],
                                "name": name, "sum": sum, "input": cc::shrink(&inp, 4000), "st": "run",
                            }));
                            r.ver += 1;
                        } else if ty == "tool_result" {
                            let tid = v["tool_id"].as_str().unwrap_or("");
                            let err = v["status"].as_str() == Some("error");
                            if let Some(it) = r.items.iter_mut().rev().find(|it| it["id"] == tid) {
                                it["st"] = json!(if err { "err" } else { "ok" });
                                it["out"] = json!(cc::trunc(v["output"].as_str().unwrap_or(""), 2500));
                                it["end"] = json!(ts);
                                r.ver += 1;
                            }
                        } else if ty == "result" {
                            let st_tok = &v["stats"];
                            r.tok_in += st_tok["input_tokens"].as_u64().unwrap_or(0);
                            r.tok_out += st_tok["output_tokens"].as_u64().unwrap_or(0);
                        }
                    }
                    _ => {}
                }
                let _ = &mut next_seq;
            }
        }
        if let Some(t) = et {
            let _ = t.join();
        }
        let code = child.wait().ok().and_then(|s| s.code());
        let err = err_lines.lock().map(|g| g.join("\n")).unwrap_or_default();
        let mut sid = String::new();
        if let Ok(mut st) = state().lock() {
            if let Some(r) = st.runs.iter_mut().find(|r| r.run_id == rid) {
                r.done = true;
                r.exit = code;
                if code != Some(0) {
                    r.error = cc::trunc(err.trim(), 600);
                }
                r.ver += 1;
                sid = r.session.clone();
            }
            if prov == "opencode" {
                st.opencode_db_mtime = 0;
            }
        }
        emit_event("run", json!({"provider": prov, "run": rid, "state": "done", "session": sid, "exit": code}));
        emit_event("stop", json!({"provider": prov, "session": sid, "error": code != Some(0)}));
    });

    Ok(run_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_placeholder_and_refresh_work() {
        let fast = providers(false).expect("providers(false) failed");
        let arr = fast.as_array().expect("providers(false) must return array");
        assert_eq!(arr.len(), 5);
        for p in arr {
            println!("FAST: {} installed={} logged_in={} bin={}", p["id"], p["installed"], p["logged_in"], p["bin"]);
        }
        let full = providers(true).expect("providers(true) failed");
        let arr2 = full.as_array().expect("providers(true) must return array");
        assert_eq!(arr2.len(), 5);
        // providers(true) sekarang asinkron — tunggu pengecekan background selesai.
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            let (done, list) = {
                let st = state().lock().unwrap_or_else(|e| e.into_inner());
                (!st.providers.is_empty() && !st.refreshing_providers, st.providers.clone())
            };
            if done {
                for p in list {
                    println!("FULL: {} installed={} logged_in={} ver={} acc={} models={}", p.id, p.installed, p.logged_in, p.version, p.account, p.models.len());
                }
                break;
            }
            assert!(Instant::now() < deadline, "provider refresh tidak selesai dalam 90 detik");
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    #[test]
    fn providers_for_snapshot_does_not_deadlock() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let list = providers_for_snapshot();
            let _ = tx.send(list.len());
        });
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(10)).expect("providers_for_snapshot deadlock"),
            5
        );
    }

    #[test]
    #[cfg(windows)]
    fn cli_capture_times_out_and_kills_tree() {
        let started = Instant::now();
        let err = run_cli_capture_opts(
            Path::new("C:\\Windows\\System32\\cmd.exe"),
            &["/C", "ping", "-n", "30", "127.0.0.1"],
            "claude",
            Duration::from_millis(800),
            None,
        )
        .expect_err("perintah panjang harus kena timeout");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "timeout tidak efektif, butuh {:?}",
            started.elapsed()
        );
        assert!(err.contains("tidak merespons"), "error tak terduga: {err}");
    }

    #[test]
    fn local_session_scanners_do_not_panic() {
        let _ = scan_codex_sessions(0);
        let _ = scan_gemini_sessions(0);
        let _ = scan_antigravity_sessions(0);
        assert!(!default_models("claude").is_empty());
        assert!(!default_models("opencode").is_empty());
        assert!(!default_models("antigravity").is_empty());
    }
}

