//! Claude Code hub.
//! - Membaca transcript `~/.claude/projects/*/*.jsonl` secara inkremental (timeline, token, judul).
//! - Server hook HTTP di 127.0.0.1 (token rahasia) untuk status live dan gerbang izin
//!   `PermissionRequest` (Izinkan/Tolak dari notch).
//! - Menjalankan sesi headless (`claude -p --output-format stream-json`) yang sepenuhnya dikontrol notch.
//! - Kontrol proses: CPU/RAM, proses anak, stop (kill pohon proses), buka folder/VS Code/terminal.
use crate::win;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

pub const PORT: u16 = 47821;
const MAX_ITEMS: usize = 600;
const RECENT_MS: i64 = 3 * 24 * 3600 * 1000;
const UNMANAGED_WAIT: u64 = 180;
const MANAGED_WAIT: u64 = 900;

pub fn now_ms() -> i64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0) }

/// "2026-11-10T04:05:37.912Z" -> epoch ms (UTC).
fn iso_ms(s: &str) -> Option<i64> {
    let n = |a: usize, l: usize| -> Option<i64> { s.get(a..a + l)?.parse::<i64>().ok() };
    let (y, mo, d, h, mi, se) = (n(0, 4)?, n(5, 2)?, n(8, 2)?, n(11, 2)?, n(14, 2)?, n(17, 2)?);
    let mut ms = 0;
    if s.as_bytes().get(19) == Some(&b'.') {
        let f: String = s[20..].chars().take_while(|c| c.is_ascii_digit()).take(3).collect();
        ms = format!("{f:0<3}").parse().unwrap_or(0);
    }
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let doy = (153 * ((mo + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86_400 + h * 3_600 + mi * 60 + se) * 1_000 + ms)
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n { return s.to_string(); }
    let mut o: String = s.chars().take(n).collect();
    o.push('…');
    o
}
/// Potong semua string panjang di dalam nilai JSON (agar IPC ringan).
fn shrink(v: &Value, n: usize) -> Value {
    match v {
        Value::String(s) => Value::String(trunc(s, n)),
        Value::Array(a) => Value::Array(a.iter().take(60).map(|x| shrink(x, n)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), shrink(x, n))).collect()),
        _ => v.clone(),
    }
}
fn base(p: &str) -> String {
    p.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().unwrap_or(p).to_string()
}

/// Ringkasan satu baris untuk sebuah pemanggilan tool.
pub fn tool_summary(name: &str, inp: &Value) -> String {
    let s = |k: &str| inp.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let first_line = |x: String| x.lines().next().unwrap_or("").to_string();
    let r = match name {
        "Read" | "Write" | "NotebookEdit" => { let f = s("file_path"); base(&if f.is_empty() { s("notebook_path") } else { f }) }
        "Edit" | "MultiEdit" => {
            let (o, n) = (s("old_string"), s("new_string"));
            let f = base(&s("file_path"));
            if name == "Edit" { format!("{f}  +{} −{}", n.lines().count(), o.lines().count()) } else { f }
        }
        "Bash" | "PowerShell" => first_line(s("command")),
        "Grep" => { let p = s("path"); format!("{}{}", s("pattern"), if p.is_empty() { String::new() } else { format!("  ·  {}", base(&p)) }) }
        "Glob" => s("pattern"),
        "WebFetch" => s("url"),
        "WebSearch" => s("query"),
        "Task" | "Agent" => { let d = s("description"); if d.is_empty() { first_line(s("prompt")) } else { d } }
        "TodoWrite" => format!("{} tugas", inp.get("todos").and_then(|t| t.as_array()).map(|a| a.len()).unwrap_or(0)),
        "Skill" => s("skill"),
        _ => {
            if !s("description").is_empty() { s("description") }
            else if let Some(o) = inp.as_object() { o.values().find_map(|v| v.as_str().map(|x| first_line(x.to_string()))).unwrap_or_default() }
            else { String::new() }
        }
    };
    trunc(&r, 200)
}

// ======================================================================
// Transcript
// ======================================================================
#[derive(Default)]
struct Tx {
    offset: u64,
    mtime: i64,
    ver: u64,
    session_id: String,
    title: String,
    last_prompt: String,
    cwd: String,
    branch: String,
    model: String,
    version: String,
    entry: String,
    first_ts: i64,
    last_ts: i64,
    tok_in: u64,
    tok_out: u64,
    tok_cache: u64,
    usage: Vec<(i64, u64)>,
    seen: HashSet<String>,
    items: VecDeque<Value>,
    seq: u64,
    busy: bool,
    turn_ts: i64,
    activity: String,
    sidechain_ts: i64,
}

impl Tx {
    fn push(&mut self, mut it: Value, ts: i64) {
        self.seq += 1;
        it["seq"] = json!(self.seq);
        it["ts"] = json!(ts);
        self.items.push_back(it);
        while self.items.len() > MAX_ITEMS { self.items.pop_front(); }
    }

    fn user_text(&mut self, text: &str, ts: i64) {
        let t = text.trim();
        if t.is_empty() { return; }
        if t.starts_with("[Request interrupted") {
            self.push(json!({"k":"note","text":"Dihentikan oleh pengguna"}), ts);
            self.busy = false;
            self.activity = "Dihentikan".into();
            return;
        }
        if let Some(rest) = t.strip_prefix("<command-name>") {
            let cmd = rest.split("</command-name>").next().unwrap_or("").trim();
            let args = t.split("<command-args>").nth(1).and_then(|x| x.split("</command-args>").next()).unwrap_or("").trim();
            let line = if args.is_empty() { cmd.to_string() } else { format!("{cmd} {args}") };
            self.push(json!({"k":"user","text":line}), ts);
            return;
        }
        if t.starts_with('<') || t.starts_with("Caveat:") { return; }
        self.push(json!({"k":"user","text":trunc(t, 6000)}), ts);
        self.last_prompt = trunc(t, 300);
        self.turn_ts = ts;
        self.busy = true;
        self.activity = "Berpikir…".into();
    }

    fn line(&mut self, v: &Value) {
        let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        let ts = v.get("timestamp").and_then(|x| x.as_str()).and_then(iso_ms).unwrap_or(0);
        if ts > 0 {
            if self.first_ts == 0 { self.first_ts = ts; }
            self.last_ts = self.last_ts.max(ts);
        }
        let gs = |k: &str| v.get(k).and_then(|x| x.as_str()).filter(|s| !s.is_empty()).map(String::from);
        if let Some(x) = gs("sessionId") { self.session_id = x; }
        if self.cwd.is_empty() { if let Some(x) = gs("cwd") { self.cwd = x; } }
        if let Some(x) = gs("gitBranch") { self.branch = x; }
        if let Some(x) = gs("version") { self.version = x; }
        if let Some(x) = gs("entrypoint") { self.entry = x; }
        match ty {
            "ai-title" => if let Some(x) = gs("aiTitle") { self.title = x; },
            "custom-title" => if let Some(x) = gs("customTitle") { self.title = x; },
            "summary" => if self.title.is_empty() { if let Some(x) = gs("summary") { self.title = x; } },
            "last-prompt" => if let Some(x) = gs("lastPrompt") { self.last_prompt = x; },
            "user" | "assistant" => {}
            _ => return,
        }
        if ty != "user" && ty != "assistant" { return; }
        if v.get("isSidechain").and_then(|x| x.as_bool()).unwrap_or(false) {
            self.sidechain_ts = ts;
            return;
        }
        if v.get("isMeta").and_then(|x| x.as_bool()).unwrap_or(false) { return; }
        let Some(msg) = v.get("message") else { return };
        let content = msg.get("content");

        if ty == "user" {
            if v.get("isCompactSummary").and_then(|x| x.as_bool()).unwrap_or(false) {
                self.push(json!({"k":"note","text":"Percakapan diringkas (compact)"}), ts);
                return;
            }
            match content {
                Some(Value::String(s)) => self.user_text(s, ts),
                Some(Value::Array(arr)) => {
                    for b in arr {
                        match b.get("type").and_then(|x| x.as_str()).unwrap_or("") {
                            "text" => self.user_text(b.get("text").and_then(|x| x.as_str()).unwrap_or(""), ts),
                            "image" => self.push(json!({"k":"user","text":"[gambar]"}), ts),
                            "tool_result" => {
                                let id = b.get("tool_use_id").and_then(|x| x.as_str()).unwrap_or("");
                                let err = b.get("is_error").and_then(|x| x.as_bool()).unwrap_or(false);
                                let out = match b.get("content") {
                                    Some(Value::String(s)) => s.clone(),
                                    Some(Value::Array(a)) => a.iter().filter_map(|c| c.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("\n"),
                                    _ => String::new(),
                                };
                                if let Some(it) = self.items.iter_mut().rev().take(200).find(|it| it["id"] == id) {
                                    it["st"] = json!(if err { "err" } else { "ok" });
                                    it["out"] = json!(trunc(&out, 2500));
                                    it["end"] = json!(ts);
                                }
                                self.busy = true;
                                self.activity = "Berpikir…".into();
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            return;
        }

        // assistant
        if let Some(m) = msg.get("model").and_then(|x| x.as_str()) {
            if !m.starts_with('<') { self.model = m.to_string(); }
        }
        let mid = msg.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
        if let Some(u) = msg.get("usage") {
            if !mid.is_empty() && self.seen.insert(mid.clone()) {
                let g = |k: &str| u.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
                let (i, c, r, o) = (g("input_tokens"), g("cache_creation_input_tokens"), g("cache_read_input_tokens"), g("output_tokens"));
                self.tok_in += i + c;
                self.tok_cache += r;
                self.tok_out += o;
                self.usage.push((ts, i + c + o));
            }
        }
        if let Some(Value::Array(arr)) = content {
            for b in arr {
                match b.get("type").and_then(|x| x.as_str()).unwrap_or("") {
                    "text" => {
                        let t = b.get("text").and_then(|x| x.as_str()).unwrap_or("").trim();
                        if !t.is_empty() {
                            self.push(json!({"k":"text","text":trunc(t, 12000)}), ts);
                            self.activity = trunc(t.lines().next().unwrap_or(""), 120);
                        }
                    }
                    "thinking" => self.activity = "Berpikir…".into(),
                    "tool_use" => {
                        let name = b.get("name").and_then(|x| x.as_str()).unwrap_or("tool");
                        let inp = b.get("input").cloned().unwrap_or(Value::Null);
                        let sum = tool_summary(name, &inp);
                        self.activity = format!("{name} · {sum}");
                        self.push(json!({"k":"tool","id":b.get("id"),"name":name,"sum":sum,"input":shrink(&inp, 4000),"st":"run"}), ts);
                    }
                    _ => {}
                }
            }
        }
        match msg.get("stop_reason").and_then(|x| x.as_str()) {
            Some("tool_use") => self.busy = true,
            Some(_) => { self.busy = false; }
            None => {}
        }
    }

    fn update(&mut self, path: &Path, len: u64, mtime: i64) -> bool {
        if len < self.offset { *self = Tx::default(); }
        if len == self.offset { self.mtime = mtime; return false; }
        let Ok(mut f) = fs::File::open(path) else { return false };
        if f.seek(SeekFrom::Start(self.offset)).is_err() { return false; }
        let mut buf = Vec::with_capacity((len - self.offset) as usize);
        if f.take(len - self.offset).read_to_end(&mut buf).is_err() { return false; }
        let Some(last_nl) = buf.iter().rposition(|&b| b == b'\n') else { return false };
        for l in buf[..last_nl].split(|&b| b == b'\n') {
            if l.is_empty() { continue; }
            if let Ok(v) = serde_json::from_slice::<Value>(l) { self.line(&v); }
        }
        self.offset += last_nl as u64 + 1;
        self.mtime = mtime;
        self.ver += 1;
        true
    }
}

// ======================================================================
// State
// ======================================================================
#[derive(Default, Clone)]
struct Live {
    state: String,
    activity: String,
    last: i64,
    pid: u32,
    mode: String,
    note: String,
    cwd: String,
    transcript: String,
}

struct Pending {
    id: u64,
    session: String,
    tool: String,
    input: Value,
    suggestions: Value,
    cwd: String,
    created: i64,
    managed: bool,
    tx: mpsc::Sender<String>,
}

#[derive(Default, Clone)]
struct Managed {
    run_id: String,
    pid: u32,
    session: String,
    cwd: String,
    prompt: String,
    started: i64,
    done: bool,
    exit: Option<i32>,
    error: String,
    result: Value,
}

#[derive(Default)]
struct State {
    tx: HashMap<PathBuf, Tx>,
    live: HashMap<String, Live>,
    pending: Vec<Pending>,
    next_id: u64,
    managed: Vec<Managed>,
    allow_session: HashMap<String, HashSet<String>>,
    history: VecDeque<Value>,
    cpu_prev: HashMap<u32, (u64, Instant)>,
    server_err: String,
}

pub struct Hub {
    app: AppHandle,
    token: String,
    st: Mutex<State>,
}

static HUB: OnceLock<Arc<Hub>> = OnceLock::new();
fn hub() -> Result<&'static Arc<Hub>, String> { HUB.get().ok_or_else(|| "hub belum siap".to_string()) }

fn notch_dir() -> PathBuf {
    let p = PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("WindowsNotch");
    let _ = fs::create_dir_all(&p);
    p
}
fn claude_dir() -> PathBuf {
    if let Ok(d) = std::env::var("CLAUDE_CONFIG_DIR") { return PathBuf::from(d); }
    PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join(".claude")
}
fn load_token() -> String {
    let f = notch_dir().join("cc_hook.json");
    if let Some(t) = fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()).and_then(|v| v["token"].as_str().map(String::from)) {
        if t.len() >= 24 { return t; }
    }
    use rand::Rng;
    let t: String = (0..40).map(|_| format!("{:x}", rand::thread_rng().gen_range(0..16))).collect();
    let _ = fs::write(&f, json!({"token": t}).to_string());
    t
}

pub fn start(app: AppHandle) {
    let h = Arc::new(Hub { app, token: load_token(), st: Mutex::new(State::default()) });
    let _ = HUB.set(h.clone());
    std::thread::spawn(move || serve(h));
}

impl Hub {
    fn emit(&self, kind: &str, extra: Value) {
        let mut v = json!({"kind": kind});
        if let (Some(o), Value::Object(e)) = (v.as_object_mut(), extra) { o.extend(e); }
        let _ = self.app.emit("cc-event", v);
    }
}

// ======================================================================
// Server hook HTTP
// ======================================================================
fn serve(h: Arc<Hub>) {
    let l = match TcpListener::bind(("127.0.0.1", PORT)) {
        Ok(l) => l,
        Err(e) => {
            if let Ok(mut s) = h.st.lock() { s.server_err = format!("Port {PORT} tidak bisa dipakai: {e}"); }
            return;
        }
    };
    for s in l.incoming().flatten() {
        let h = h.clone();
        std::thread::spawn(move || { let _ = handle(&h, s); });
    }
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> { h.windows(n.len()).position(|w| w == n) }

fn dechunk(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = find(&b[i..], b"\r\n") {
        let sz = usize::from_str_radix(String::from_utf8_lossy(&b[i..i + p]).split(';').next().unwrap_or("0").trim(), 16).unwrap_or(0);
        if sz == 0 { break; }
        let start = i + p + 2;
        if start + sz > b.len() { out.extend_from_slice(&b[start..]); break; }
        out.extend_from_slice(&b[start..start + sz]);
        i = start + sz + 2;
        if i >= b.len() { break; }
    }
    out
}

fn respond(s: &mut TcpStream, code: u16, body: &str) -> std::io::Result<()> {
    let reason = match code { 200 => "OK", 401 => "Unauthorized", _ => "Not Found" };
    write!(s, "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())?;
    s.flush()
}

fn handle(h: &Arc<Hub>, mut s: TcpStream) -> std::io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(15)))?;
    let peer = s.peer_addr().map(|a| a.port()).unwrap_or(0);
    let mut buf = Vec::new();
    let mut tmp = [0u8; 16384];
    let he = loop {
        let n = s.read(&mut tmp)?;
        if n == 0 { return Ok(()); }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(p) = find(&buf, b"\r\n\r\n") { break p + 4; }
        if buf.len() > 256 * 1024 { return Ok(()); }
    };
    let head = String::from_utf8_lossy(&buf[..he]).to_string();
    let mut lines = head.split("\r\n");
    let req = lines.next().unwrap_or("").to_string();
    let (mut clen, mut token, mut chunked) = (0usize, String::new(), false);
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "content-length" => clen = v.trim().parse().unwrap_or(0),
                "x-notch-token" => token = v.trim().to_string(),
                "transfer-encoding" => chunked = v.to_ascii_lowercase().contains("chunked"),
                _ => {}
            }
        }
    }
    if req.starts_with("GET /ping") { return respond(&mut s, 200, "{\"ok\":true}"); }
    if !req.starts_with("POST /hook") { return respond(&mut s, 404, ""); }
    if token != h.token { return respond(&mut s, 401, ""); }
    let mut body = buf[he..].to_vec();
    if chunked {
        while find(&body, b"0\r\n\r\n").is_none() {
            let n = s.read(&mut tmp)?;
            if n == 0 { break; }
            body.extend_from_slice(&tmp[..n]);
        }
        body = dechunk(&body);
    } else {
        while body.len() < clen {
            let n = s.read(&mut tmp)?;
            if n == 0 { break; }
            body.extend_from_slice(&tmp[..n]);
        }
    }
    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let out = on_hook(h, v, peer, &s);
    respond(&mut s, 200, &out)
}

fn peer_closed(s: &TcpStream) -> bool {
    if s.set_nonblocking(true).is_err() { return false; }
    let mut b = [0u8; 1];
    let r = s.peek(&mut b);
    let _ = s.set_nonblocking(false);
    match r {
        Ok(0) => true,
        Ok(_) => false,
        Err(e) => e.kind() != std::io::ErrorKind::WouldBlock,
    }
}

fn perm_json(behavior: &str, extra: Value) -> String {
    let mut d = json!({"behavior": behavior});
    if let (Some(o), Value::Object(e)) = (d.as_object_mut(), extra) { o.extend(e); }
    json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": d}}).to_string()
}

fn on_hook(h: &Arc<Hub>, v: Value, peer: u16, s: &TcpStream) -> String {
    let ev = v["hook_event_name"].as_str().unwrap_or("").to_string();
    let sid = v["session_id"].as_str().unwrap_or("").to_string();
    if sid.is_empty() { return String::new(); }
    let pid = if peer != 0 { win::tcp_owner(peer, PORT).unwrap_or(0) } else { 0 };
    let tool = v["tool_name"].as_str().unwrap_or("").to_string();
    let input = v.get("tool_input").cloned().unwrap_or(Value::Null);
    let now = now_ms();

    let mut st = match h.st.lock() { Ok(g) => g, Err(_) => return String::new() };
    let live = st.live.entry(sid.clone()).or_default();
    live.last = now;
    if pid != 0 { live.pid = pid; }
    if let Some(m) = v["permission_mode"].as_str() { live.mode = m.to_string(); }
    if let Some(c) = v["cwd"].as_str() { live.cwd = c.to_string(); }
    if let Some(t) = v["transcript_path"].as_str() { live.transcript = t.to_string(); }

    let clear = |st: &mut State, pred: &dyn Fn(&Pending) -> bool| { st.pending.retain(|p| !pred(p)); };

    match ev.as_str() {
        "SessionStart" => { live.state = "idle".into(); live.activity = "Sesi dimulai".into(); }
        "UserPromptSubmit" => {
            live.state = "running".into();
            live.activity = "Berpikir…".into();
            live.note.clear();
            clear(&mut st, &|p| p.session == sid);
        }
        "PreToolUse" => {
            live.state = "running".into();
            live.activity = format!("{tool} · {}", tool_summary(&tool, &input));
        }
        "PostToolUse" | "PostToolUseFailure" => {
            live.state = "running".into();
            live.activity = "Berpikir…".into();
            clear(&mut st, &|p| p.session == sid && p.tool == tool && p.input == input);
        }
        "Notification" => {
            let t = v["notification_type"].as_str().unwrap_or("");
            live.note = v["message"].as_str().unwrap_or("").to_string();
            if t == "idle_prompt" { live.state = "idle".into(); live.activity = "Menunggu instruksi".into(); }
            if t == "permission_prompt" && !st.pending.iter().any(|p| p.session == sid) {
                let l = st.live.get_mut(&sid).unwrap();
                l.state = "waiting".into();
                l.activity = "Menunggu izin di terminal".into();
            }
            let note = v["message"].as_str().unwrap_or("").to_string();
            drop(st);
            h.emit("notify", json!({"session": sid, "type": t, "message": note}));
            return String::new();
        }
        "Stop" | "StopFailure" => {
            live.state = "idle".into();
            live.activity = if ev == "Stop" { "Selesai".into() } else { "Berhenti karena error".into() };
            clear(&mut st, &|p| p.session == sid);
            drop(st);
            h.emit("stop", json!({"session": sid, "error": ev == "StopFailure"}));
            return String::new();
        }
        "SessionEnd" => {
            live.state = "ended".into();
            live.activity = "Sesi ditutup".into();
            live.pid = 0;
            clear(&mut st, &|p| p.session == sid);
        }
        "PermissionRequest" => {
            live.state = "waiting".into();
            live.activity = format!("Minta izin · {tool}");
            let managed = st.managed.iter().any(|m| (!m.session.is_empty() && m.session == sid) || (pid != 0 && m.pid == pid && !m.done));
            if st.allow_session.get(&sid).map(|s| s.contains(&tool)).unwrap_or(false) {
                st.history.push_front(json!({"ts": now, "session": sid, "tool": tool, "sum": tool_summary(&tool, &input), "d": "allow (otomatis sesi)"}));
                return perm_json("allow", json!({}));
            }
            st.next_id += 1;
            let id = st.next_id;
            let (tx, rx) = mpsc::channel();
            st.pending.push(Pending {
                id, session: sid.clone(), tool: tool.clone(), input: input.clone(),
                suggestions: v.get("permission_suggestions").cloned().unwrap_or(Value::Null),
                cwd: v["cwd"].as_str().unwrap_or("").to_string(), created: now, managed, tx,
            });
            drop(st);
            h.emit("permission", json!({"id": id, "session": sid, "tool": tool, "sum": tool_summary(&tool, &input)}));
            let deadline = Instant::now() + Duration::from_secs(if managed { MANAGED_WAIT } else { UNMANAGED_WAIT });
            let out = loop {
                match rx.recv_timeout(Duration::from_millis(400)) {
                    Ok(r) => break r,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break String::new(),
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if Instant::now() > deadline {
                            break if managed { perm_json("deny", json!({"message": "Tidak dijawab di Windows Notch (timeout)"})) } else { String::new() };
                        }
                        if peer_closed(s) { break String::new(); }
                    }
                }
            };
            if let Ok(mut st) = h.st.lock() {
                st.pending.retain(|p| p.id != id);
                if let Some(l) = st.live.get_mut(&sid) {
                    if l.state == "waiting" { l.state = "running".into(); l.activity = "Berpikir…".into(); }
                }
            }
            h.emit("resolved", json!({"id": id, "session": sid}));
            return out;
        }
        _ => {}
    }
    drop(st);
    h.emit("hook", json!({"event": ev, "session": sid}));
    String::new()
}

// ======================================================================
// Perintah untuk UI
// ======================================================================
fn scan(st: &mut State) {
    let root = claude_dir().join("projects");
    let now = now_ms();
    let Ok(dirs) = fs::read_dir(&root) else { return };
    let mut seen = HashSet::new();
    for d in dirs.flatten() {
        let Ok(files) = fs::read_dir(d.path()) else { continue };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().and_then(|x| x.to_str()) != Some("jsonl") { continue; }
            let Ok(md) = f.metadata() else { continue };
            let mtime = md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(0);
            if now - mtime > RECENT_MS && !st.tx.contains_key(&p) { continue; }
            seen.insert(p.clone());
            st.tx.entry(p.clone()).or_default().update(&p, md.len(), mtime);
        }
    }
    st.tx.retain(|p, t| seen.contains(p) && now - t.mtime <= RECENT_MS);
}

fn host_of(ppid: u32, procs: &HashMap<u32, (u32, String)>) -> String {
    let me = std::process::id();
    if ppid == me { return "Notch".into(); }
    let name = procs.get(&ppid).map(|x| x.1.to_lowercase()).unwrap_or_default();
    match name.as_str() {
        "code.exe" | "code - insiders.exe" => "VS Code".into(),
        "cursor.exe" => "Cursor".into(),
        "windsurf.exe" => "Windsurf".into(),
        "windowsterminal.exe" | "openconsole.exe" | "cmd.exe" | "powershell.exe" | "pwsh.exe" | "bash.exe" | "wezterm-gui.exe" | "alacritty.exe" | "conhost.exe" => "Terminal".into(),
        "claude.exe" => "Claude Desktop".into(),
        "" => "—".into(),
        n => n.trim_end_matches(".exe").to_string(),
    }
}

fn is_cc_path(p: &str) -> bool {
    let l = p.to_lowercase();
    !(l.contains("\\anthropicclaude\\") || l.contains("\\windowsapps\\") || l.contains("\\programs\\claude\\"))
}

pub fn snapshot(since_ms: i64) -> Result<Value, String> {
    let h = hub()?;
    let mut st = h.st.lock().map_err(|e| e.to_string())?;
    scan(&mut st);
    let now = now_ms();

    // proses
    let plist = win::processes();
    let pmap: HashMap<u32, (u32, String)> = plist.iter().map(|p| (p.pid, (p.ppid, p.name.clone()))).collect();
    let mut kids: HashMap<u32, Vec<u32>> = HashMap::new();
    for p in &plist { kids.entry(p.ppid).or_default().push(p.pid); }
    let ncpu = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
    let mut procs = Vec::new();
    let mut by_pid: HashMap<u32, Value> = HashMap::new();
    let mut alive_pids = HashSet::new();
    for p in plist.iter().filter(|p| p.name.eq_ignore_ascii_case("claude.exe")) {
        let Some(info) = win::proc_info(p.pid) else { continue };
        if !info.path.is_empty() && !is_cc_path(&info.path) { continue; }
        alive_pids.insert(p.pid);
        let cpu = match st.cpu_prev.insert(p.pid, (info.cpu_100ns, Instant::now())) {
            Some((c0, t0)) => {
                let dt = t0.elapsed().as_secs_f64();
                if dt > 0.05 { ((info.cpu_100ns.saturating_sub(c0)) as f64 / 1e7 / dt / ncpu * 100.0).min(100.0) } else { 0.0 }
            }
            None => 0.0,
        };
        let mut children = Vec::new();
        let mut q: VecDeque<(u32, u8)> = kids.get(&p.pid).map(|v| v.iter().map(|&x| (x, 1)).collect()).unwrap_or_default();
        while let Some((c, d)) = q.pop_front() {
            if children.len() > 12 { break; }
            if let Some((_, n)) = pmap.get(&c) {
                let nl = n.to_lowercase();
                if nl != "conhost.exe" && nl != "chcp.com" { children.push(json!({"pid": c, "name": n})); }
            }
            if d < 4 { if let Some(v) = kids.get(&c) { q.extend(v.iter().map(|&x| (x, d + 1))); } }
        }
        let v = json!({
            "pid": p.pid, "ppid": p.ppid, "host": host_of(p.ppid, &pmap), "cpu": (cpu * 10.0).round() / 10.0,
            "mem": info.mem, "started": info.created_ms, "children": children, "session": Value::Null,
        });
        by_pid.insert(p.pid, v.clone());
        procs.push(v);
    }
    st.cpu_prev.retain(|pid, _| alive_pids.contains(pid));

    // sesi
    let mut sessions = Vec::new();
    let mut today_tok = 0u64;
    let mut today_sessions = 0u64;
    let managed = st.managed.clone();
    let mut mapped_pids = HashSet::new();
    let mut sorted: Vec<(&PathBuf, &Tx)> = st.tx.iter().filter(|(_, t)| !t.session_id.is_empty()).collect();
    sorted.sort_by(|a, b| b.1.mtime.cmp(&a.1.mtime));
    for (_, t) in sorted.iter() {
        let tt: u64 = t.usage.iter().filter(|u| u.0 >= since_ms).map(|u| u.1).sum();
        today_tok += tt;
        if t.last_ts >= since_ms { today_sessions += 1; }
    }
    for (path, t) in sorted.into_iter().take(40) {
        let sid = &t.session_id;
        let live = st.live.get(sid).cloned().unwrap_or_default();
        let m = managed.iter().rev().find(|m| &m.session == sid);
        let mut pid = if live.pid != 0 && alive_pids.contains(&live.pid) { live.pid } else { 0 };
        if pid == 0 { if let Some(m) = m { if !m.done && alive_pids.contains(&m.pid) { pid = m.pid; } } }
        if pid != 0 { mapped_pids.insert(pid); }
        let pending = st.pending.iter().filter(|p| &p.session == sid).count();
        let fresh = now - t.mtime < 8_000;
        let hook_fresh = live.last > 0 && now - live.last < 30 * 60_000;
        let state = if pending > 0 { "waiting" }
            else if hook_fresh && live.state == "ended" { "done" }
            else if hook_fresh && live.state == "waiting" && pid != 0 { "waiting" }
            else if hook_fresh && live.state == "running" && (pid != 0 || fresh) { "running" }
            else if fresh || (t.busy && now - t.mtime < 90_000) { "running" }
            else if pid != 0 { "idle" }
            else { "done" };
        let activity = if hook_fresh && !live.activity.is_empty() && state != "done" { live.activity.clone() } else if state == "running" { t.activity.clone() } else { String::new() };
        let managed_live = m.map(|m| !m.done).unwrap_or(false);
        let title = if !t.title.is_empty() { t.title.clone() } else if !t.last_prompt.is_empty() { trunc(&t.last_prompt, 80) } else { "Sesi tanpa judul".into() };
        let proc_v = if pid != 0 { by_pid.get(&pid).cloned() } else { None };
        sessions.push(json!({
            "id": sid, "path": path.to_string_lossy(), "title": title, "prompt": t.last_prompt,
            "cwd": t.cwd, "project": base(&t.cwd), "branch": t.branch, "model": t.model, "version": t.version, "entry": t.entry,
            "start": t.first_ts, "turn": t.turn_ts, "last": t.last_ts.max(t.mtime.min(now)), "ver": t.ver,
            "tok": {"in": t.tok_in, "out": t.tok_out, "cache": t.tok_cache},
            "state": state, "activity": activity, "note": live.note, "mode": live.mode,
            "pid": pid, "proc": proc_v, "managed": m.is_some(), "managed_live": managed_live,
            "pending": pending, "hooked": live.last > 0, "subagent": now - t.sidechain_ts < 15_000,
        }));
    }
    for p in procs.iter_mut() {
        let pid = p["pid"].as_u64().unwrap_or(0) as u32;
        if mapped_pids.contains(&pid) {
            if let Some(s) = sessions.iter().find(|s| s["pid"] == pid) { p["session"] = s["id"].clone(); }
        }
    }
    let pending: Vec<Value> = st.pending.iter().map(|p| {
        let t = st.tx.values().find(|t| t.session_id == p.session);
        json!({
            "id": p.id, "session": p.session, "tool": p.tool, "sum": tool_summary(&p.tool, &p.input), "input": shrink(&p.input, 6000),
            "cwd": p.cwd, "project": base(&p.cwd), "title": t.map(|t| t.title.clone()).unwrap_or_default(),
            "created": p.created, "rules": p.suggestions.as_array().map(|a| !a.is_empty()).unwrap_or(false), "managed": p.managed,
        })
    }).collect();
    let runs: Vec<Value> = st.managed.iter().rev().take(10).map(|m| json!({
        "run": m.run_id, "pid": m.pid, "session": m.session, "cwd": m.cwd, "prompt": trunc(&m.prompt, 160), "started": m.started,
        "done": m.done, "exit": m.exit, "error": m.error, "cost": m.result.get("total_cost_usd"), "result_error": m.result.get("is_error"),
    })).collect();
    Ok(json!({
        "now": now, "sessions": sessions, "procs": procs, "pending": pending, "runs": runs,
        "today": {"tokens": today_tok, "sessions": today_sessions},
        "server": {"port": PORT, "error": st.server_err},
    }))
}

pub fn session(id: String, ver: u64) -> Result<Value, String> {
    let h = hub()?;
    let mut st = h.st.lock().map_err(|e| e.to_string())?;
    scan(&mut st);
    let Some(t) = st.tx.values().find(|t| t.session_id == id) else { return Err("Sesi tidak ditemukan".into()) };
    if t.ver == ver && ver != 0 { return Ok(Value::Null); }
    Ok(json!({"id": id, "ver": t.ver, "items": t.items.iter().collect::<Vec<_>>()}))
}

pub fn decide(id: u64, action: String, note: Option<String>) -> Result<(), String> {
    let h = hub()?;
    let mut st = h.st.lock().map_err(|e| e.to_string())?;
    let Some(pos) = st.pending.iter().position(|p| p.id == id) else { return Err("Permintaan izin sudah tidak aktif".into()) };
    let p = st.pending.remove(pos);
    let msg = note.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "Ditolak dari Windows Notch".into());
    let out = match action.as_str() {
        "allow" => perm_json("allow", json!({})),
        "allow_session" => {
            st.allow_session.entry(p.session.clone()).or_default().insert(p.tool.clone());
            perm_json("allow", json!({}))
        }
        "always" => {
            if p.suggestions.as_array().map(|a| !a.is_empty()).unwrap_or(false) { perm_json("allow", json!({"updatedPermissions": p.suggestions})) } else { perm_json("allow", json!({})) }
        }
        "deny" => perm_json("deny", json!({"message": msg})),
        "deny_stop" => perm_json("deny", json!({"message": msg, "interrupt": true})),
        _ => String::new(), // "pass": serahkan ke terminal / VS Code
    };
    st.history.push_front(json!({"ts": now_ms(), "session": p.session, "tool": p.tool, "sum": tool_summary(&p.tool, &p.input), "d": action, "project": base(&p.cwd)}));
    st.history.truncate(60);
    let _ = p.tx.send(out);
    Ok(())
}

pub fn history() -> Result<Value, String> {
    let h = hub()?;
    let st = h.st.lock().map_err(|e| e.to_string())?;
    let allow: Vec<Value> = st.allow_session.iter().map(|(s, t)| json!({"session": s, "tools": t.iter().collect::<Vec<_>>()})).collect();
    Ok(json!({"history": st.history.iter().collect::<Vec<_>>(), "allow": allow}))
}
pub fn clear_allow() -> Result<(), String> {
    hub()?.st.lock().map_err(|e| e.to_string())?.allow_session.clear();
    Ok(())
}

/// Biner Claude Code: NOTCH_CLAUDE_BIN, ~/.local/bin, PATH, npm global, lalu ekstensi VS Code terbaru.
pub fn claude_bin() -> Option<PathBuf> {
    if let Ok(b) = std::env::var("NOTCH_CLAUDE_BIN") { let p = PathBuf::from(b); if p.exists() { return Some(p); } }
    let home = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default());
    let local = home.join(".local").join("bin").join("claude.exe");
    if local.exists() { return Some(local); }
    if let Ok(path) = std::env::var("PATH") {
        for d in std::env::split_paths(&path) {
            for n in ["claude.exe", "claude.cmd"] { let p = d.join(n); if p.exists() { return Some(p); } }
        }
    }
    let npm = PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("npm").join("claude.cmd");
    if npm.exists() { return Some(npm); }
    for ext in [".vscode", ".cursor", ".windsurf", ".vscode-insiders"] {
        let Ok(rd) = fs::read_dir(home.join(ext).join("extensions")) else { continue };
        let mut c: Vec<(Vec<u64>, PathBuf)> = rd.flatten().filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            let rest = n.strip_prefix("anthropic.claude-code-")?;
            let ver: Vec<u64> = rest.split('-').next()?.split('.').filter_map(|x| x.parse().ok()).collect();
            let p = e.path().join("resources").join("native-binary").join("claude.exe");
            p.exists().then_some((ver, p))
        }).collect();
        c.sort();
        if let Some((_, p)) = c.pop() { return Some(p); }
    }
    None
}

pub fn run(cwd: String, prompt: String, resume: Option<String>, model: Option<String>, mode: Option<String>) -> Result<String, String> {
    let h = hub()?.clone();
    let bin = claude_bin().ok_or("Claude Code tidak ditemukan. Pasang CLI (claude.exe) atau set NOTCH_CLAUDE_BIN.")?;
    let cwd = cwd.trim().to_string();
    if cwd.is_empty() || !Path::new(&cwd).is_dir() { return Err(format!("Folder tidak ditemukan: {cwd}")); }
    if prompt.trim().is_empty() { return Err("Prompt kosong".into()); }
    if let Some(r) = &resume {
        let st = h.st.lock().map_err(|e| e.to_string())?;
        if st.managed.iter().any(|m| &m.session == r && !m.done) { return Err("Sesi ini masih berjalan".into()); }
    }
    let mut cmd = Command::new(&bin);
    cmd.args(["-p", "--output-format", "stream-json", "--verbose"]);
    if let Some(r) = resume.as_ref().filter(|s| !s.is_empty()) { cmd.args(["--resume", r]); }
    if let Some(m) = model.as_ref().filter(|s| !s.is_empty() && *s != "default") { cmd.args(["--model", m]); }
    if let Some(m) = mode.as_ref().filter(|s| !s.is_empty() && *s != "default") { cmd.args(["--permission-mode", m]); }
    cmd.current_dir(&cwd).env("NOTCH_MANAGED", "1").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    crate::sys::hide_console(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("Gagal menjalankan Claude Code: {e}"))?;
    if let Some(mut si) = child.stdin.take() {
        let _ = si.write_all(prompt.as_bytes());
    }
    let run_id = format!("run-{}", now_ms());
    {
        let mut st = h.st.lock().map_err(|e| e.to_string())?;
        st.managed.push(Managed {
            run_id: run_id.clone(), pid: child.id(), session: resume.clone().unwrap_or_default(), cwd: cwd.clone(),
            prompt: prompt.clone(), started: now_ms(), ..Default::default()
        });
        if st.managed.len() > 30 { st.managed.remove(0); }
    }
    h.emit("run", json!({"run": run_id, "state": "start"}));
    let rid = run_id.clone();
    std::thread::spawn(move || {
        let err_lines = Arc::new(Mutex::new(VecDeque::<String>::new()));
        let et = child.stderr.take().map(|s| {
            let el = err_lines.clone();
            std::thread::spawn(move || for l in BufReader::new(s).lines().map_while(Result::ok) {
                if let Ok(mut g) = el.lock() { g.push_back(l); if g.len() > 15 { g.pop_front(); } }
            })
        });
        if let Some(out) = child.stdout.take() {
            for l in BufReader::new(out).lines().map_while(Result::ok) {
                let Ok(v) = serde_json::from_str::<Value>(&l) else { continue };
                let ty = v["type"].as_str().unwrap_or("");
                if let Some(sid) = v["session_id"].as_str() {
                    if let Ok(mut st) = h.st.lock() {
                        if let Some(m) = st.managed.iter_mut().find(|m| m.run_id == rid) {
                            if m.session != sid { m.session = sid.to_string(); }
                        }
                    }
                }
                if ty == "system" && v["subtype"] == "init" { h.emit("run", json!({"run": rid, "state": "init", "session": v["session_id"]})); }
                if ty == "result" {
                    if let Ok(mut st) = h.st.lock() {
                        if let Some(m) = st.managed.iter_mut().find(|m| m.run_id == rid) { m.result = shrink(&v, 2000); }
                    }
                }
            }
        }
        if let Some(t) = et { let _ = t.join(); }
        let code = child.wait().ok().and_then(|s| s.code());
        let err = err_lines.lock().map(|g| g.iter().cloned().collect::<Vec<_>>().join("\n")).unwrap_or_default();
        let mut sid = String::new();
        if let Ok(mut st) = h.st.lock() {
            if let Some(m) = st.managed.iter_mut().find(|m| m.run_id == rid) {
                m.done = true;
                m.exit = code;
                if code != Some(0) { m.error = trunc(err.trim(), 600); }
                sid = m.session.clone();
            }
        }
        h.emit("run", json!({"run": rid, "state": "done", "session": sid, "exit": code}));
    });
    Ok(run_id)
}

/// Hentikan proses Claude Code (hanya PID claude.exe yang dikenal atau run milik notch).
pub fn kill(pid: u32) -> Result<(), String> {
    let h = hub()?;
    let ok = {
        let st = h.st.lock().map_err(|e| e.to_string())?;
        st.managed.iter().any(|m| m.pid == pid)
    } || win::processes().iter().any(|p| p.pid == pid && p.name.eq_ignore_ascii_case("claude.exe"));
    if !ok { return Err("PID bukan proses Claude Code".into()); }
    win::kill_tree(pid)?;
    if let Ok(mut st) = h.st.lock() {
        let sids: Vec<String> = st.live.iter().filter(|(_, l)| l.pid == pid).map(|(s, _)| s.clone()).collect();
        st.pending.retain(|p| !sids.contains(&p.session));
        for s in sids { if let Some(l) = st.live.get_mut(&s) { l.state = "ended".into(); l.activity = "Dihentikan dari notch".into(); l.pid = 0; } }
    }
    h.emit("killed", json!({"pid": pid}));
    Ok(())
}

pub fn open(kind: String, cwd: String, session: Option<String>) -> Result<(), String> {
    if !Path::new(&cwd).is_dir() { return Err(format!("Folder tidak ditemukan: {cwd}")); }
    match kind.as_str() {
        "folder" => { Command::new("explorer").arg(&cwd).spawn().map_err(|e| e.to_string())?; }
        "vscode" => {
            let mut c = Command::new("code.cmd");
            c.arg(&cwd);
            crate::sys::hide_console(&mut c);
            c.spawn().map_err(|_| "VS Code (code) tidak ada di PATH".to_string())?;
        }
        "terminal" => {
            let bin = claude_bin().ok_or("Claude Code tidak ditemukan")?;
            let mut args: Vec<String> = vec![bin.to_string_lossy().to_string()];
            if let Some(s) = session.filter(|s| !s.is_empty()) { args.push("--resume".into()); args.push(s); }
            let wt = Command::new("wt.exe").arg("-d").arg(&cwd).args(&args).spawn();
            if wt.is_err() {
                use std::os::windows::process::CommandExt;
                Command::new(&args[0]).args(&args[1..]).current_dir(&cwd).creation_flags(0x10).spawn().map_err(|e| e.to_string())?;
            }
        }
        _ => return Err("aksi tidak dikenal".into()),
    }
    Ok(())
}

// ======================================================================
// Pemasangan hook di ~/.claude/settings.json
// ======================================================================
const HOOK_EVENTS: [(&str, bool, u32); 10] = [
    ("SessionStart", false, 5), ("SessionEnd", false, 5), ("UserPromptSubmit", false, 5),
    ("PreToolUse", true, 5), ("PostToolUse", true, 5), ("PostToolUseFailure", true, 5),
    ("PermissionRequest", true, 1000), ("Notification", false, 5), ("Stop", false, 5), ("StopFailure", false, 5),
];
fn hook_url() -> String { format!("http://127.0.0.1:{PORT}/hook") }
fn is_ours(group: &Value) -> bool {
    group["hooks"].as_array().map(|a| a.iter().any(|x| x["url"].as_str().map(|u| u.contains(&format!("127.0.0.1:{PORT}/hook"))).unwrap_or(false))).unwrap_or(false)
}

pub fn hooks(action: String) -> Result<Value, String> {
    let h = hub()?;
    let path = claude_dir().join("settings.json");
    let raw = fs::read_to_string(&path).unwrap_or_else(|_| "{}".into());
    let mut v: Value = if raw.trim().is_empty() { json!({}) } else {
        serde_json::from_str(&raw).map_err(|e| format!("settings.json tidak valid, tidak diubah: {e}"))?
    };
    if !v.is_object() { return Err("settings.json bukan objek JSON".into()); }
    let status = |v: &Value| -> (bool, bool) {
        let groups = v["hooks"]["PermissionRequest"].as_array().cloned().unwrap_or_default();
        let ours: Vec<&Value> = groups.iter().filter(|g| is_ours(g)).collect();
        let current = ours.iter().any(|g| g["hooks"][0]["headers"]["x-notch-token"] == h.token.as_str());
        (!ours.is_empty(), current)
    };
    if action == "install" || action == "uninstall" {
        let bak = path.with_extension("json.notch-bak");
        if !bak.exists() && path.exists() { let _ = fs::copy(&path, &bak); }
        let hooks = v.as_object_mut().unwrap().entry("hooks").or_insert_with(|| json!({}));
        if !hooks.is_object() { *hooks = json!({}); }
        let ho = hooks.as_object_mut().unwrap();
        let names: Vec<String> = ho.keys().cloned().collect();
        for n in names {
            if let Some(arr) = ho.get_mut(&n).and_then(|a| a.as_array_mut()) { arr.retain(|g| !is_ours(g)); }
            if ho.get(&n).and_then(|a| a.as_array()).map(|a| a.is_empty()).unwrap_or(false) { ho.remove(&n); }
        }
        if action == "install" {
            for (ev, tool, timeout) in HOOK_EVENTS {
                let mut g = json!({"hooks": [{"type": "http", "url": hook_url(), "timeout": timeout, "headers": {"x-notch-token": h.token}}]});
                if tool { g["matcher"] = json!("*"); }
                ho.entry(ev.to_string()).or_insert_with(|| json!([])).as_array_mut().ok_or("hooks rusak")?.push(g);
            }
        }
        if ho.is_empty() { v.as_object_mut().unwrap().remove("hooks"); }
        if let Some(parent) = path.parent() { let _ = fs::create_dir_all(parent); }
        fs::write(&path, serde_json::to_string_pretty(&v).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
    }
    let (any, current) = status(&v);
    let server_err = h.st.lock().map(|s| s.server_err.clone()).unwrap_or_default();
    Ok(json!({
        "installed": current, "stale": any && !current, "settings": path.to_string_lossy(), "port": PORT,
        "server_ok": server_err.is_empty(), "server_error": server_err,
        "bin": claude_bin().map(|p| p.to_string_lossy().to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn iso() {
        assert_eq!(iso_ms("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(iso_ms("2026-11-10T04:05:37.912Z"), Some(1_794_283_537_912));
    }
    #[test]
    fn summary() {
        assert_eq!(tool_summary("Bash", &json!({"command": "npm run build\nnext"})), "npm run build");
        assert_eq!(tool_summary("Read", &json!({"file_path": "C:\\a\\b\\main.rs"})), "main.rs");
    }
    /// Parse transcript asli di mesin ini (bila ada) dan cetak ringkasannya.
    #[test]
    fn parse_real_transcripts() {
        let root = claude_dir().join("projects");
        let Ok(dirs) = fs::read_dir(&root) else { return };
        for d in dirs.flatten() {
            for f in fs::read_dir(d.path()).into_iter().flatten().flatten() {
                let p = f.path();
                if p.extension().and_then(|x| x.to_str()) != Some("jsonl") { continue; }
                let md = f.metadata().unwrap();
                let mut t = Tx::default();
                t.update(&p, md.len(), 0);
                let tools = t.items.iter().filter(|i| i["k"] == "tool").count();
                let done = t.items.iter().filter(|i| i["k"] == "tool" && i["st"] != "run").count();
                println!("{} | title={:?} cwd={:?} model={} items={} tools={}/{} tok_out={} busy={} act={:?}",
                    base(&p.to_string_lossy()), t.title, t.cwd, t.model, t.items.len(), done, tools, t.tok_out, t.busy, t.activity);
            }
        }
    }
    #[test]
    fn dechunk_ok() { assert_eq!(dechunk(b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n"), b"Wikipedia"); }
}
