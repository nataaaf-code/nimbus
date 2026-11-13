// Cuaca real-time: lokasi dari IP (ipinfo.io) → suhu dari Open-Meteo. Tanpa API key.
use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Serialize, Clone)]
pub struct Weather {
    pub temp: f32,
    pub code: u16,
    pub city: String,
    pub desc: String,
}

static CACHE: Mutex<Option<(Weather, Instant)>> = Mutex::new(None);
const TTL: Duration = Duration::from_secs(1800);

fn er<E: std::fmt::Display>(x: E) -> String { x.to_string() }

fn wmo_desc(code: u16) -> &'static str {
    match code {
        0 => "Cerah",
        1..=3 => "Berawan",
        45 | 48 => "Berkabut",
        51..=55 => "Gerimis",
        61..=65 => "Hujan",
        71..=77 => "Salju",
        80..=82 => "Hujan lebat",
        85 | 86 => "Salju lebat",
        95..=99 => "Badai",
        _ => "—",
    }
}

pub fn get() -> Result<Weather, String> {
    if let Ok(g) = CACHE.lock() {
        if let Some((ref w, t)) = *g {
            if t.elapsed() < TTL { return Ok(w.clone()); }
        }
    }

    let loc: serde_json::Value = ureq::get("https://ipinfo.io/json")
        .call().map_err(er)?.into_json().map_err(er)?;
    let city = loc["city"].as_str().unwrap_or("—").to_string();
    let coords = loc["loc"].as_str().unwrap_or("0,0");
    let (lat, lon) = coords.split_once(',').unwrap_or(("0", "0"));

    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m,weathercode&timezone=auto&forecast_days=1"
    );
    let wx: serde_json::Value = ureq::get(&url).call().map_err(er)?.into_json().map_err(er)?;

    let temp = wx["current"]["temperature_2m"].as_f64().unwrap_or(0.0) as f32;
    let code = wx["current"]["weathercode"].as_u64().unwrap_or(0) as u16;

    let w = Weather { temp, code, city, desc: wmo_desc(code).to_string() };
    if let Ok(mut g) = CACHE.lock() { *g = Some((w.clone(), Instant::now())); }
    Ok(w)
}
