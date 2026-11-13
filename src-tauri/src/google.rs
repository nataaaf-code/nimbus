//! Google Calendar: OAuth "installed app" (loopback + PKCE), refresh token disimpan di %APPDATA%\WindowsNotch. BELUM diuji.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, io::{Read, Write}, net::TcpListener, path::PathBuf};

#[derive(Serialize)] pub struct Ev { pub start: String, pub title: String }
#[derive(Serialize)] pub struct GCalStatus { pub connected: bool, pub has_creds: bool }
fn er<E: std::fmt::Display>(x: E) -> String { x.to_string() }
fn dir() -> PathBuf {
    let p = PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("WindowsNotch");
    let _ = fs::create_dir_all(&p);
    p
}
fn json(f: &str) -> Option<Value> { serde_json::from_str(&fs::read_to_string(dir().join(f)).ok()?).ok() }
fn creds() -> Result<(String, String), String> {
    let c = json("config.json").ok_or("Konfigurasi Google belum diatur. Masukkan Client ID & Secret di bawah atau buat %APPDATA%\\WindowsNotch\\config.json")?;
    let g = |k: &str| c[k].as_str().map(String::from).ok_or(format!("{k} kosong di config.json"));
    Ok((g("google_client_id")?, g("google_client_secret")?))
}
pub fn connected() -> bool { json("google_token.json").is_some() }
pub fn status() -> GCalStatus {
    GCalStatus {
        connected: connected(),
        has_creds: creds().is_ok(),
    }
}
pub fn save_creds(id: String, secret: String) -> Result<(), String> {
    let d = dir();
    let val = serde_json::json!({
        "google_client_id": id.trim(),
        "google_client_secret": secret.trim()
    });
    fs::write(d.join("config.json"), serde_json::to_string_pretty(&val).map_err(er)?).map_err(er)?;
    Ok(())
}
pub fn open_config_folder() -> Result<(), String> {
    let d = dir();
    let mut c = std::process::Command::new("explorer");
    c.arg(&d);
    crate::sys::hide_console(&mut c);
    c.spawn().map_err(er)?;
    Ok(())
}
fn enc(s: &str) -> String { s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect() }
fn dec(s: &str) -> String {
    let b = s.as_bytes(); let (mut o, mut i) = (vec![], 0);
    while i < b.len() {
        match (b[i], b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok())) {
            (b'%', Some(x)) => { o.push(x); i += 3 }
            (c, _) => { o.push(c); i += 1 }
        }
    }
    String::from_utf8_lossy(&o).into()
}
fn token(form: &[(&str, &str)]) -> Result<Value, String> {
    ureq::post("https://oauth2.googleapis.com/token").send_form(form).map_err(er)?.into_json().map_err(er)
}
pub fn login() -> Result<(), String> {
    let (id, secret) = creds()?;
    let verifier: String = (0..64).map(|_| (b'a' + rand::random::<u8>() % 26) as char).collect();
    let challenge = B64.encode(Sha256::digest(verifier.as_bytes()));
    let l = TcpListener::bind("127.0.0.1:0").map_err(er)?;
    let redir = format!("http://127.0.0.1:{}", l.local_addr().map_err(er)?.port());
    let url = format!("https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={challenge}&code_challenge_method=S256&access_type=offline&prompt=consent",
        enc(&id), enc(&redir), enc("https://www.googleapis.com/auth/calendar.readonly"));
    let mut c = std::process::Command::new("rundll32"); c.args(["url.dll,FileProtocolHandler", &url]); crate::sys::hide_console(&mut c); c.spawn().map_err(er)?;
    let (mut s, _) = l.accept().map_err(er)?;
    let mut buf = [0u8; 4096]; let n = s.read(&mut buf).map_err(er)?;
    let req = String::from_utf8_lossy(&buf[..n]).to_string();
    let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nBerhasil. Kamu bisa menutup tab ini.");
    let code = req.lines().next().and_then(|l| l.split("code=").nth(1)).and_then(|r| r.split(&['&', ' '][..]).next()).map(dec).ok_or("login dibatalkan")?;
    let t = token(&[("code", &code), ("client_id", &id), ("client_secret", &secret), ("redirect_uri", &redir), ("grant_type", "authorization_code"), ("code_verifier", &verifier)])?;
    let rt = t["refresh_token"].as_str().ok_or("refresh_token tidak diterima")?;
    fs::create_dir_all(dir()).map_err(er)?;
    fs::write(dir().join("google_token.json"), serde_json::json!({ "refresh_token": rt }).to_string()).map_err(er)
}
pub fn events(min: &str, max: &str) -> Result<Vec<Ev>, String> {
    let (id, secret) = creds()?;
    let rt = json("google_token.json").and_then(|j| j["refresh_token"].as_str().map(String::from)).ok_or("belum login")?;
    let t = token(&[("client_id", &id), ("client_secret", &secret), ("refresh_token", &rt), ("grant_type", "refresh_token")])?;
    let at = t["access_token"].as_str().ok_or("gagal memperbarui token")?;
    let r: Value = ureq::get(&format!("https://www.googleapis.com/calendar/v3/calendars/primary/events?singleEvents=true&orderBy=startTime&maxResults=50&timeMin={}&timeMax={}", enc(min), enc(max)))
        .set("Authorization", &format!("Bearer {at}")).call().map_err(er)?.into_json().map_err(er)?;
    Ok(r["items"].as_array().map(|a| a.iter().map(|i| Ev {
        start: i["start"]["dateTime"].as_str().or(i["start"]["date"].as_str()).unwrap_or("").into(),
        title: i["summary"].as_str().unwrap_or("(tanpa judul)").into() }).collect()).unwrap_or_default())
}
