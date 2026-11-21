// Jadwal Sholat Resmi (Bimas Islam Kemenag RI via api.myquran.com/v2)
// + Kalender Hijriah Bulanan (api.aladhan.com/v1 gToHCalendar) + Auto-deteksi Kota & Kiblat.
// Offline-first: unduh sekali ke %APPDATA%\Nimbus\sholat, lalu auto-update hanya jika data server berbeda.
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const MYQURAN_BASE: &str = "https://api.myquran.com/v2";
const ALADHAN_BASE: &str = "https://api.aladhan.com/v1";
const KAABA_LAT: f64 = 21.422487;
const KAABA_LON: f64 = 39.826206;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CityItem {
    pub id: String,
    pub lokasi: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct DetectedCity {
    pub id: String,
    pub lokasi: String,
    pub detected_name: String,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub qibla_deg: Option<f64>,
    pub kaaba_km: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PrayerDay {
    pub date: String,       // YYYY-MM-DD
    pub day: u32,           // 1..=31
    pub label: String,      // "Sabtu, 21/11/2026"
    pub imsak: String,
    pub subuh: String,
    pub terbit: String,
    pub dhuha: String,
    pub dzuhur: String,
    pub ashar: String,
    pub maghrib: String,
    pub isya: String,
    pub hijri_day: Option<u32>,
    pub hijri_month: Option<u32>,
    pub hijri_month_name: Option<String>,
    pub hijri_month_ar: Option<String>,
    pub hijri_year: Option<String>,
    pub hijri_label: Option<String>,
    pub holidays: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MonthSchedule {
    pub city_id: String,
    pub lokasi: String,
    pub daerah: String,
    pub year: i32,
    pub month: u32,
    pub source: String,
    pub qibla_deg: Option<f64>,
    pub kaaba_km: Option<f64>,
    pub hijri_range: String,
    pub days: Vec<PrayerDay>,
}

#[derive(Clone, Debug)]
struct HijriInfo {
    day: u32,
    month: u32,
    month_name: String,
    month_ar: String,
    year: String,
    holidays: Vec<String>,
}

static CITIES_CACHE: OnceLock<Mutex<Option<Vec<CityItem>>>> = OnceLock::new();
static CITIES_SYNCED: AtomicBool = AtomicBool::new(false);
static MONTH_CACHE: OnceLock<Mutex<HashMap<(String, i32, u32), MonthSchedule>>> = OnceLock::new();
static HIJRI_CACHE: OnceLock<Mutex<HashMap<(i32, u32), (HashMap<u32, HijriInfo>, Instant)>>> = OnceLock::new();

fn er<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn cache_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("Nimbus").join("sholat")
}

fn sanitize_cid(city_id: &str) -> String {
    let s: String = city_id
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if s.is_empty() { "1301".into() } else { s }
}

fn cities_path(dir: &Path) -> PathBuf {
    dir.join("cities.json")
}

fn month_path(dir: &Path, cid: &str, year: i32, month: u32) -> PathBuf {
    dir.join(format!("month_{}_{}_{:02}.json", sanitize_cid(cid), year, month))
}

fn read_json_file<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let raw = fs::read(path).ok()?;
    serde_json::from_slice(&raw).ok()
}

fn write_json_file<T: Serialize>(path: &Path, val: &T) {
    let Some(parent) = path.parent() else { return };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(bytes) = serde_json::to_vec(val) else { return };
    let tmp = path.with_extension("json.tmp");
    if fs::write(&tmp, &bytes).is_ok() {
        if fs::rename(&tmp, path).is_err() {
            let _ = fs::write(path, &bytes);
            let _ = fs::remove_file(&tmp);
        }
    } else {
        let _ = fs::write(path, &bytes);
    }
}

fn schedule_differs(local: &MonthSchedule, remote: &MonthSchedule) -> bool {
    local.city_id != remote.city_id
        || local.lokasi != remote.lokasi
        || local.daerah != remote.daerah
        || local.year != remote.year
        || local.month != remote.month
        || local.hijri_range != remote.hijri_range
        || local.days != remote.days
}

fn preserve_hijri_if_missing(local: &MonthSchedule, remote: &mut MonthSchedule) {
    let remote_has = remote.days.iter().any(|d| d.hijri_day.is_some());
    let local_has = local.days.iter().any(|d| d.hijri_day.is_some());
    if remote_has || !local_has {
        return;
    }
    let by_day: HashMap<u32, &PrayerDay> = local.days.iter().map(|d| (d.day, d)).collect();
    for rd in &mut remote.days {
        if let Some(ld) = by_day.get(&rd.day) {
            rd.hijri_day = ld.hijri_day;
            rd.hijri_month = ld.hijri_month;
            rd.hijri_month_name = ld.hijri_month_name.clone();
            rd.hijri_month_ar = ld.hijri_month_ar.clone();
            rd.hijri_year = ld.hijri_year.clone();
            rd.hijri_label = ld.hijri_label.clone();
            rd.holidays = ld.holidays.clone();
        }
    }
    if remote.hijri_range.is_empty() {
        remote.hijri_range = local.hijri_range.clone();
    }
}

fn apply_coords(sched: &mut MonthSchedule, lat: Option<f64>, lon: Option<f64>) {
    if let (Some(la), Some(lo)) = (lat, lon) {
        sched.qibla_deg = Some(qibla_bearing(la, lo));
        sched.kaaba_km = Some(kaaba_distance_km(la, lo));
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(7))
        .timeout_read(Duration::from_secs(12))
        .build()
}

fn get_json(url: &str) -> Result<Value, String> {
    agent()
        .get(url)
        .set("User-Agent", "Nimbus/0.2 (Windows)")
        .call()
        .map_err(er)?
        .into_json()
        .map_err(er)
}

fn hijri_month_id(num: u32, fallback: &str) -> String {
    match num {
        1 => "Muharram".into(),
        2 => "Safar".into(),
        3 => "Rabiul Awal".into(),
        4 => "Rabiul Akhir".into(),
        5 => "Jumadil Awal".into(),
        6 => "Jumadil Akhir".into(),
        7 => "Rajab".into(),
        8 => "Syaban".into(),
        9 => "Ramadan".into(),
        10 => "Syawal".into(),
        11 => "Zulkaidah".into(),
        12 => "Zulhijah".into(),
        _ => fallback.to_string(),
    }
}

/// Arah kiblat dalam derajat searah jarum jam dari Utara (0..360).
pub fn qibla_bearing(lat: f64, lon: f64) -> f64 {
    let phi1 = lat.to_radians();
    let phi2 = KAABA_LAT.to_radians();
    let d_lon = (KAABA_LON - lon).to_radians();
    let y = d_lon.sin() * phi2.cos();
    let x = phi1.cos() * phi2.sin() - phi1.sin() * phi2.cos() * d_lon.cos();
    (y.atan2(x).to_degrees() + 360.0) % 360.0
}

/// Jarak ke Ka'bah (km) menggunakan rumus Haversine.
pub fn kaaba_distance_km(lat: f64, lon: f64) -> f64 {
    const R: f64 = 6371.0;
    let d_lat = (KAABA_LAT - lat).to_radians();
    let d_lon = (KAABA_LON - lon).to_radians();
    let a = (d_lat / 2.0).sin().powi(2)
        + lat.to_radians().cos() * KAABA_LAT.to_radians().cos() * (d_lon / 2.0).sin().powi(2);
    2.0 * R * a.sqrt().asin()
}

fn parse_cities_value(v: &Value) -> Vec<CityItem> {
    v.get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let id = item.get("id")?.as_str()?.trim().to_string();
                    let lokasi = item.get("lokasi")?.as_str()?.trim().to_string();
                    if id.is_empty() || lokasi.is_empty() {
                        return None;
                    }
                    Some(CityItem { id, lokasi })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn fetch_cities_remote() -> Result<Vec<CityItem>, String> {
    let url = format!("{MYQURAN_BASE}/sholat/kota/semua");
    let v = get_json(&url)?;
    let list = parse_cities_value(&v);
    if list.is_empty() {
        return Err("Daftar kota Kemenag tidak tersedia".into());
    }
    Ok(list)
}

fn spawn_cities_sync_if_needed(dir: PathBuf, local: Vec<CityItem>) {
    if CITIES_SYNCED.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(move || {
        if let Ok(remote) = fetch_cities_remote() {
            if remote != local {
                write_json_file(&cities_path(&dir), &remote);
                if let Some(lock) = CITIES_CACHE.get() {
                    if let Ok(mut g) = lock.lock() {
                        *g = Some(remote);
                    }
                }
            }
        }
    });
}

fn all_cities() -> Result<Vec<CityItem>, String> {
    let lock = CITIES_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(g) = lock.lock() {
        if let Some(ref list) = *g {
            if !list.is_empty() {
                return Ok(list.clone());
            }
        }
    }

    let dir = cache_dir();
    if let Some(disk_list) = read_json_file::<Vec<CityItem>>(&cities_path(&dir)) {
        if !disk_list.is_empty() {
            if let Ok(mut g) = lock.lock() {
                *g = Some(disk_list.clone());
            }
            spawn_cities_sync_if_needed(dir, disk_list.clone());
            return Ok(disk_list);
        }
    }

    let list = fetch_cities_remote()?;
    CITIES_SYNCED.store(true, Ordering::Release);
    write_json_file(&cities_path(&dir), &list);
    if let Ok(mut g) = lock.lock() {
        *g = Some(list.clone());
    }
    Ok(list)
}

fn match_city_from_list(list: &[CityItem], query: &str) -> Option<CityItem> {
    let q = query.trim().to_uppercase();
    if q.is_empty() {
        return None;
    }
    // Prioritas 1: cocok persis "KOTA <Q>" atau "<Q>"
    let kota_q = format!("KOTA {q}");
    if let Some(c) = list.iter().find(|c| c.lokasi == kota_q || c.lokasi == q) {
        return Some(c.clone());
    }
    // Prioritas 2: mengandung "KOTA " dan query
    if let Some(c) = list
        .iter()
        .find(|c| c.lokasi.starts_with("KOTA ") && c.lokasi.contains(&q))
    {
        return Some(c.clone());
    }
    // Prioritas 3: mengandung query (kota/kabupaten)
    if let Some(c) = list.iter().find(|c| c.lokasi.contains(&q)) {
        return Some(c.clone());
    }
    // Prioritas 4: kata pertama query (misal "South Jakarta" -> "JAKARTA")
    for token in q.split_whitespace().rev() {
        if token.len() < 4 {
            continue;
        }
        if let Some(c) = list.iter().find(|c| c.lokasi.contains(token)) {
            return Some(c.clone());
        }
    }
    None
}

pub fn search_cities(query: String) -> Result<Vec<CityItem>, String> {
    let list = all_cities()?;
    let q = query.trim().to_uppercase();
    if q.is_empty() {
        // Tampilkan kota-kota utama dulu lalu urutan daftar
        let featured = [
            "1301", "1219", "1638", "1433", "1505", "0228", "1634", "2622", "1107", "1221",
            "1222", "1225", "1434", "1709", "0610", "2113", "3301",
        ];
        let mut out: Vec<CityItem> = featured
            .iter()
            .filter_map(|id| list.iter().find(|c| c.id == *id).cloned())
            .collect();
        for item in &list {
            if out.len() >= 60 {
                break;
            }
            if !out.iter().any(|x| x.id == item.id) {
                out.push(item.clone());
            }
        }
        return Ok(out);
    }
    let tokens: Vec<&str> = q.split_whitespace().collect();
    let mut matches: Vec<CityItem> = list
        .into_iter()
        .filter(|c| tokens.iter().all(|t| c.lokasi.contains(t)))
        .collect();
    matches.sort_by_key(|c| {
        let starts = c.lokasi.starts_with(&format!("KOTA {q}")) || c.lokasi == q;
        let is_kota = c.lokasi.starts_with("KOTA ");
        (!starts, !is_kota, c.lokasi.len())
    });
    matches.truncate(60);
    Ok(matches)
}

pub fn detect_city() -> Result<DetectedCity, String> {
    let loc = get_json("https://ipinfo.io/json").ok();
    let city_raw = loc
        .as_ref()
        .and_then(|v| v.get("city"))
        .and_then(|s| s.as_str())
        .unwrap_or("Jakarta")
        .to_string();
    let region_raw = loc
        .as_ref()
        .and_then(|v| v.get("region"))
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let (lat, lon) = loc
        .as_ref()
        .and_then(|v| v.get("loc"))
        .and_then(|s| s.as_str())
        .and_then(|s| s.split_once(','))
        .and_then(|(a, b)| Some((a.parse::<f64>().ok()?, b.parse::<f64>().ok()?)))
        .map(|(a, b)| (Some(a), Some(b)))
        .unwrap_or((Some(-6.2088), Some(106.8456)));

    let matched = all_cities().ok().and_then(|list| {
        match_city_from_list(&list, &city_raw)
            .or_else(|| match_city_from_list(&list, &region_raw))
            .or_else(|| list.into_iter().find(|c| c.id == "1301"))
    });

    let (id, lokasi) = match matched {
        Some(c) => (c.id, c.lokasi),
        None => ("1301".into(), "KOTA JAKARTA".into()),
    };

    let (qibla_deg, kaaba_km) = match (lat, lon) {
        (Some(la), Some(lo)) => (Some(qibla_bearing(la, lo)), Some(kaaba_distance_km(la, lo))),
        _ => (None, None),
    };

    Ok(DetectedCity {
        id,
        lokasi,
        detected_name: city_raw,
        lat,
        lon,
        qibla_deg,
        kaaba_km,
    })
}

fn parse_hijri_calendar(v: &Value) -> HashMap<u32, HijriInfo> {
    let mut map = HashMap::new();
    let Some(arr) = v.get("data").and_then(|d| d.as_array()) else {
        return map;
    };
    for entry in arr {
        let g_day = entry
            .pointer("/gregorian/day")
            .and_then(|x| x.as_str().and_then(|s| s.parse::<u32>().ok()).or_else(|| x.as_u64().map(|n| n as u32)));
        let Some(gd) = g_day else { continue };
        let Some(h) = entry.get("hijri") else { continue };
        let h_day = h
            .get("day")
            .and_then(|x| x.as_str().and_then(|s| s.parse::<u32>().ok()).or_else(|| x.as_u64().map(|n| n as u32)))
            .unwrap_or(1);
        let h_month = h
            .pointer("/month/number")
            .and_then(|x| x.as_u64().map(|n| n as u32).or_else(|| x.as_str()?.parse::<u32>().ok()))
            .unwrap_or(1);
        let en = h.pointer("/month/en").and_then(|x| x.as_str()).unwrap_or("");
        let ar = h.pointer("/month/ar").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let year = h
            .get("year")
            .and_then(|x| x.as_str().map(|s| s.to_string()).or_else(|| x.as_u64().map(|n| n.to_string())))
            .unwrap_or_default();
        let holidays = h
            .get("holidays")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(|t| t.trim().to_string()))
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        map.insert(
            gd,
            HijriInfo {
                day: h_day,
                month: h_month,
                month_name: hijri_month_id(h_month, en),
                month_ar: ar,
                year,
                holidays,
            },
        );
    }
    map
}

fn fetch_hijri_month(year: i32, month: u32) -> HashMap<u32, HijriInfo> {
    let lock = HIJRI_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(g) = lock.lock() {
        if let Some((ref map, t)) = g.get(&(year, month)) {
            if t.elapsed() < Duration::from_secs(12 * 3600) {
                return map.clone();
            }
        }
    }
    let url = format!("{ALADHAN_BASE}/gToHCalendar/{month}/{year}");
    let map = get_json(&url)
        .map(|v| parse_hijri_calendar(&v))
        .unwrap_or_default();
    if !map.is_empty() {
        if let Ok(mut g) = lock.lock() {
            g.insert((year, month), (map.clone(), Instant::now()));
        }
    }
    map
}

fn parse_myquran_month(v: &Value, hijri_map: &HashMap<u32, HijriInfo>) -> Result<(String, String, Vec<PrayerDay>), String> {
    let data = v.get("data").ok_or("Format respons jadwal sholat tidak valid")?;
    let lokasi = data.get("lokasi").and_then(|s| s.as_str()).unwrap_or("KOTA JAKARTA").to_string();
    let daerah = data.get("daerah").and_then(|s| s.as_str()).unwrap_or("").to_string();
    let arr = data
        .get("jadwal")
        .and_then(|j| j.as_array())
        .ok_or("Jadwal bulanan tidak ditemukan")?;

    let mut days = Vec::with_capacity(arr.len());
    for (idx, item) in arr.iter().enumerate() {
        let date = item.get("date").and_then(|s| s.as_str()).unwrap_or("").to_string();
        let day = date
            .rsplit('-')
            .next()
            .and_then(|d| d.parse::<u32>().ok())
            .unwrap_or((idx + 1) as u32);
        let get_t = |k: &str| item.get(k).and_then(|s| s.as_str()).unwrap_or("--:--").trim().to_string();
        let h = hijri_map.get(&day);
        let hijri_label = h.map(|hi| format!("{} {} {} H", hi.day, hi.month_name, hi.year));
        days.push(PrayerDay {
            date,
            day,
            label: get_t("tanggal"),
            imsak: get_t("imsak"),
            subuh: get_t("subuh"),
            terbit: get_t("terbit"),
            dhuha: get_t("dhuha"),
            dzuhur: get_t("dzuhur"),
            ashar: get_t("ashar"),
            maghrib: get_t("maghrib"),
            isya: get_t("isya"),
            hijri_day: h.map(|hi| hi.day),
            hijri_month: h.map(|hi| hi.month),
            hijri_month_name: h.map(|hi| hi.month_name.clone()),
            hijri_month_ar: h.map(|hi| hi.month_ar.clone()),
            hijri_year: h.map(|hi| hi.year.clone()),
            hijri_label,
            holidays: h.map(|hi| hi.holidays.clone()).unwrap_or_default(),
        });
    }
    Ok((lokasi, daerah, days))
}

fn build_hijri_range(days: &[PrayerDay]) -> String {
    let first = days.iter().find(|d| d.hijri_month_name.is_some());
    let last = days.iter().rev().find(|d| d.hijri_month_name.is_some());
    match (first, last) {
        (Some(f), Some(l)) => {
            let fm = f.hijri_month_name.as_deref().unwrap_or("");
            let fy = f.hijri_year.as_deref().unwrap_or("");
            let lm = l.hijri_month_name.as_deref().unwrap_or("");
            let ly = l.hijri_year.as_deref().unwrap_or("");
            if fm == lm && fy == ly {
                format!("{fm} {fy} H")
            } else if fy == ly {
                format!("{fm} – {lm} {fy} H")
            } else {
                format!("{fm} {fy} – {lm} {ly} H")
            }
        }
        _ => String::new(),
    }
}

fn load_local_month(dir: &Path, cid: &str, year: i32, month: u32) -> Option<MonthSchedule> {
    let cache = MONTH_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(g) = cache.lock() {
        if let Some(sched) = g.get(&(cid.to_string(), year, month)) {
            return Some(sched.clone());
        }
    }
    let disk: MonthSchedule = read_json_file(&month_path(dir, cid, year, month))?;
    if disk.days.is_empty() {
        return None;
    }
    if let Ok(mut g) = cache.lock() {
        g.insert((cid.to_string(), year, month), disk.clone());
    }
    Some(disk)
}

fn fetch_month_remote(
    cid: &str,
    year: i32,
    month: u32,
    lat: Option<f64>,
    lon: Option<f64>,
) -> Result<MonthSchedule, String> {
    let url = format!("{MYQURAN_BASE}/sholat/jadwal/{cid}/{year}/{month:02}");
    let v = get_json(&url)?;
    if v.get("status").and_then(|b| b.as_bool()) == Some(false) {
        let msg = v
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Jadwal sholat tidak ditemukan");
        return Err(msg.to_string());
    }

    let hijri_map = fetch_hijri_month(year, month);
    let (lokasi, daerah, days) = parse_myquran_month(&v, &hijri_map)?;
    if days.is_empty() {
        return Err("Jadwal bulanan kosong".into());
    }
    let hijri_range = build_hijri_range(&days);
    let (qibla_deg, kaaba_km) = match (lat, lon) {
        (Some(la), Some(lo)) => (Some(qibla_bearing(la, lo)), Some(kaaba_distance_km(la, lo))),
        _ => (Some(qibla_bearing(-6.2088, 106.8456)), Some(kaaba_distance_km(-6.2088, 106.8456))),
    };

    Ok(MonthSchedule {
        city_id: cid.to_string(),
        lokasi,
        daerah,
        year,
        month,
        source: "Bimas Islam Kemenag RI".into(),
        qibla_deg,
        kaaba_km,
        hijri_range,
        days,
    })
}

fn save_if_changed(
    dir: &Path,
    local: Option<&MonthSchedule>,
    mut remote: MonthSchedule,
) -> (MonthSchedule, bool) {
    if let Some(loc) = local {
        preserve_hijri_if_missing(loc, &mut remote);
        if !schedule_differs(loc, &remote) {
            return (remote, false);
        }
    }
    write_json_file(&month_path(dir, &remote.city_id, remote.year, remote.month), &remote);
    let cache = MONTH_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut g) = cache.lock() {
        g.insert((remote.city_id.clone(), remote.year, remote.month), remote.clone());
    }
    (remote, true)
}

pub fn month_schedule(
    city_id: String,
    year: i32,
    month: u32,
    lat: Option<f64>,
    lon: Option<f64>,
    force: bool,
) -> Result<MonthSchedule, String> {
    if !(1..=12).contains(&month) {
        return Err("Bulan tidak valid (1..12)".into());
    }
    let cid = sanitize_cid(&city_id);
    let dir = cache_dir();
    let local = load_local_month(&dir, &cid, year, month);

    if !force {
        if let Some(mut sched) = local {
            apply_coords(&mut sched, lat, lon);
            return Ok(sched);
        }
    }

    match fetch_month_remote(&cid, year, month, lat, lon) {
        Ok(remote) => {
            let (mut saved, _) = save_if_changed(&dir, local.as_ref(), remote);
            apply_coords(&mut saved, lat, lon);
            Ok(saved)
        }
        Err(err) => {
            if let Some(mut sched) = local {
                apply_coords(&mut sched, lat, lon);
                return Ok(sched);
            }
            Err(err)
        }
    }
}

/// Cek server; simpan & kembalikan Some(jadwal) HANYA jika data server berbeda dari lokal.
/// Kembalikan Ok(None) jika data sama persis; Err jika gagal koneksi (offline).
pub fn sync_month(
    city_id: String,
    year: i32,
    month: u32,
    lat: Option<f64>,
    lon: Option<f64>,
) -> Result<Option<MonthSchedule>, String> {
    if !(1..=12).contains(&month) {
        return Ok(None);
    }
    let cid = sanitize_cid(&city_id);
    let dir = cache_dir();
    let local = load_local_month(&dir, &cid, year, month);

    let remote = fetch_month_remote(&cid, year, month, lat, lon)?;
    let (mut saved, changed) = save_if_changed(&dir, local.as_ref(), remote);
    if !changed {
        return Ok(None);
    }
    apply_coords(&mut saved, lat, lon);
    Ok(Some(saved))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn qibla_for_jakarta_is_west_northwest() {
        let deg = qibla_bearing(-6.2088, 106.8456);
        assert!(deg > 294.0 && deg < 296.0, "unexpected Jakarta qibla: {deg}");
        let km = kaaba_distance_km(-6.2088, 106.8456);
        assert!(km > 7800.0 && km < 8100.0, "unexpected Jakarta distance: {km}");
    }

    #[test]
    fn match_city_prefers_exact_kota() {
        let list = vec![
            CityItem { id: "1204".into(), lokasi: "KAB. BANDUNG".into() },
            CityItem { id: "1219".into(), lokasi: "KOTA BANDUNG".into() },
            CityItem { id: "1301".into(), lokasi: "KOTA JAKARTA".into() },
        ];
        assert_eq!(match_city_from_list(&list, "Bandung").unwrap().id, "1219");
        assert_eq!(match_city_from_list(&list, "South Jakarta").unwrap().id, "1301");
    }

    #[test]
    fn parses_myquran_and_hijri_month() {
        let hijri_json = json!({
            "data": [{
                "gregorian": { "day": "01" },
                "hijri": {
                    "day": "21",
                    "year": "1448",
                    "month": { "number": 5, "en": "Jumada al-ula", "ar": "جمادى الأولى" },
                    "holidays": []
                }
            }]
        });
        let hmap = parse_hijri_calendar(&hijri_json);
        assert_eq!(hmap.get(&1).unwrap().month_name, "Jumadil Awal");

        let mq_json = json!({
            "status": true,
            "data": {
                "lokasi": "KOTA JAKARTA",
                "daerah": "DKI JAKARTA",
                "jadwal": [{
                    "tanggal": "Minggu, 01/11/2026",
                    "imsak": "03:59",
                    "subuh": "04:09",
                    "terbit": "05:23",
                    "dhuha": "05:51",
                    "dzuhur": "11:40",
                    "ashar": "14:55",
                    "maghrib": "17:49",
                    "isya": "19:00",
                    "date": "2026-11-01"
                }]
            }
        });
        let (lok, daerah, days) = parse_myquran_month(&mq_json, &hmap).unwrap();
        assert_eq!(lok, "KOTA JAKARTA");
        assert_eq!(daerah, "DKI JAKARTA");
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].day, 1);
        assert_eq!(days[0].subuh, "04:09");
        assert_eq!(days[0].hijri_label.as_deref(), Some("21 Jumadil Awal 1448 H"));
        assert_eq!(build_hijri_range(&days), "Jumadil Awal 1448 H");
    }

    #[test]
    fn disk_cache_roundtrip_and_diff_update() {
        let tmp = std::env::temp_dir().join(format!("nimbus_sholat_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);

        let base = MonthSchedule {
            city_id: "1301".into(),
            lokasi: "KOTA JAKARTA".into(),
            daerah: "DKI JAKARTA".into(),
            year: 2026,
            month: 11,
            source: "Bimas Islam Kemenag RI".into(),
            qibla_deg: Some(295.15),
            kaaba_km: Some(7912.0),
            hijri_range: "Jumadil Awal 1448 H".into(),
            days: vec![PrayerDay {
                date: "2026-11-01".into(),
                day: 1,
                label: "Minggu, 01/11/2026".into(),
                imsak: "03:59".into(),
                subuh: "04:09".into(),
                terbit: "05:23".into(),
                dhuha: "05:51".into(),
                dzuhur: "11:40".into(),
                ashar: "14:55".into(),
                maghrib: "17:49".into(),
                isya: "19:00".into(),
                hijri_day: Some(21),
                hijri_month: Some(5),
                hijri_month_name: Some("Jumadil Awal".into()),
                hijri_month_ar: Some("جمادى الأولى".into()),
                hijri_year: Some("1448".into()),
                hijri_label: Some("21 Jumadil Awal 1448 H".into()),
                holidays: vec![],
            }],
        };

        // 1. Pertama kali: simpan ke disk
        let (_, changed1) = save_if_changed(&tmp, None, base.clone());
        assert!(changed1);
        let loaded: MonthSchedule = read_json_file(&month_path(&tmp, "1301", 2026, 11)).unwrap();
        assert!(!schedule_differs(&loaded, &base));

        // 2. Data identik (meski Hijriah remote kosong sementara): tidak berubah & Hijriah lokal dipertahankan
        let mut remote_no_hijri = base.clone();
        remote_no_hijri.hijri_range.clear();
        remote_no_hijri.days[0].hijri_day = None;
        remote_no_hijri.days[0].hijri_label = None;
        let (merged, changed2) = save_if_changed(&tmp, Some(&loaded), remote_no_hijri);
        assert!(!changed2);
        assert_eq!(merged.days[0].hijri_day, Some(21));

        // 3. Ada perubahan jam sholat di server: terdeteksi beda dan ditulis ulang ke disk
        let mut remote_updated = base.clone();
        remote_updated.days[0].subuh = "04:10".into();
        let (_, changed3) = save_if_changed(&tmp, Some(&loaded), remote_updated);
        assert!(changed3);
        let reloaded: MonthSchedule = read_json_file(&month_path(&tmp, "1301", 2026, 11)).unwrap();
        assert_eq!(reloaded.days[0].subuh, "04:10");

        let _ = fs::remove_dir_all(&tmp);
    }
}
