// Google Calendar via URL ICS rahasia — tanpa OAuth, tanpa client_id/secret.
// Dapatkan URL dari: calendar.google.com → Setelan → pilih kalender → "Alamat rahasia dalam format iCal"
use serde::Serialize;
use std::{fs, path::PathBuf};

#[derive(Serialize)]
pub struct Ev {
    pub start: String,
    pub title: String,
}

#[derive(Serialize)]
pub struct IcalStatus {
    pub has_url: bool,
}

fn er<E: std::fmt::Display>(x: E) -> String { x.to_string() }

fn dir() -> PathBuf {
    let p = PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("Nimbus");
    let _ = fs::create_dir_all(&p);
    p
}

fn ics_url() -> Option<String> {
    let s = fs::read_to_string(dir().join("config.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    v["ics_url"].as_str().map(String::from)
}

pub fn status() -> IcalStatus {
    IcalStatus { has_url: ics_url().is_some() }
}

pub fn clear_url() -> Result<(), String> {
    let path = dir().join("config.json");
    if path.exists() { fs::remove_file(&path).map_err(er)?; }
    Ok(())
}

pub fn save_url(url: String) -> Result<(), String> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err("URL tidak boleh kosong".into());
    }
    let val = serde_json::json!({ "ics_url": url });
    fs::write(dir().join("config.json"), serde_json::to_string_pretty(&val).map_err(er)?).map_err(er)
}

// ICS "DTSTART" value → "YYYY-MM-DD" or "YYYY-MM-DDTHH:MM:SSZ"
fn ics_date(val: &str) -> String {
    let val = val.trim();
    if val.len() >= 15 && val.as_bytes().get(8) == Some(&b'T') {
        let z = if val.ends_with('Z') { "Z" } else { "" };
        format!("{}-{}-{}T{}:{}:{}{}", &val[..4], &val[4..6], &val[6..8], &val[9..11], &val[11..13], &val[13..15], z)
    } else if val.len() == 8 {
        format!("{}-{}-{}", &val[..4], &val[4..6], &val[6..8])
    } else {
        val.to_string()
    }
}

fn ics_unescape(s: &str) -> String {
    s.replace("\\n", "\n").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\")
}

// RFC 5545 §3.1 — lines may be folded with CRLF+SPACE or CRLF+TAB
fn unfold(s: &str) -> String {
    s.replace("\r\n ", "").replace("\r\n\t", "").replace("\n ", "").replace("\n\t", "")
}

pub fn events(min: &str, max: &str) -> Result<Vec<Ev>, String> {
    let url = ics_url().ok_or("URL ICS belum diatur")?;
    let body = ureq::get(&url).call().map_err(er)?.into_string().map_err(er)?;

    // min/max are ISO strings from JS like "2026-11-13T00:00:00.000Z" — take date portion
    let min_d = &min[..min.len().min(10)];
    let max_d = &max[..max.len().min(10)];

    let unfolded = unfold(&body);
    let mut result = Vec::new();
    let mut in_event = false;
    let mut start = String::new();
    let mut title = String::new();

    for line in unfolded.lines() {
        let line = line.trim_end_matches('\r');
        match line {
            "BEGIN:VEVENT" => {
                in_event = true;
                start = String::new();
                title = String::new();
            }
            "END:VEVENT" => {
                if in_event && !start.is_empty() && !title.is_empty() {
                    let s_date = &start[..start.len().min(10)];
                    if s_date >= min_d && s_date < max_d {
                        result.push(Ev { start: start.clone(), title: title.clone() });
                    }
                }
                in_event = false;
            }
            _ if in_event => {
                if let Some(colon) = line.find(':') {
                    let key = line[..colon].split(';').next().unwrap_or("").to_uppercase();
                    let val = &line[colon + 1..];
                    match key.as_str() {
                        "DTSTART" => start = ics_date(val),
                        "SUMMARY" => title = ics_unescape(val),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    result.sort_by(|a, b| a.start.cmp(&b.start));
    Ok(result)
}
