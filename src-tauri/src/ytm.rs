//! Integrasi YouTube Music tanpa aplikasi desktop:
//! - Pencarian, album, dan metadata via InnerTube (`youtubei/v1`)
//! - Audio dialirkan lewat proxy lokal (`stream`) dari URL InnerTube klien
//!   `VISIONOS`, dengan fallback `ANDROID`/`IOS`
//! - Lirik sinkron via LRCLIB (`lrclib.net/api`) + fallback YouTube Music InnerTube
//! - Fallback now-playing ke Windows SMTC (`sys::media`) saat pemutar internal idle

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

type R<T> = Result<T, String>;
fn err<E: std::fmt::Display>(x: E) -> String { x.to_string() }

const LRCLIB_UA: &str = "Nimbus/0.2.0 (https://github.com/nataaaf-code/nimbus)";
const INNERTUBE_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const ANDROID_UA: &str = "com.google.android.youtube/20.10.38 (Linux; U; Android 11) gzip";
const IOS_UA: &str = "com.google.ios.youtube/20.10.4 (iPhone16,2; U; CPU iOS 18_3_1 like Mac OS X;)";
const VISIONOS_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";

// Filter params untuk YouTube Music InnerTube search
const PARAM_SONGS: &str = "EgWKAQIIAWoQEAMQBBAJEAoQBRAREBAQFQ%3D%3D";
const PARAM_ALBUMS: &str = "EgWKAQIYAWoQEAMQBBAJEAoQBRAREBAQFQ%3D%3D";

fn http_agent(timeout_ms: u64) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(timeout_ms))
        .timeout_read(Duration::from_millis(timeout_ms))
        .timeout_write(Duration::from_millis(timeout_ms))
        .build()
}

fn lyrics_cache() -> &'static Mutex<HashMap<String, LyricsResult>> {
    static C: OnceLock<Mutex<HashMap<String, LyricsResult>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn album_cache() -> &'static Mutex<HashMap<String, AlbumDetail>> {
    static C: OnceLock<Mutex<HashMap<String, AlbumDetail>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn track_album_id_cache() -> &'static Mutex<HashMap<String, String>> {
    static C: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

// =========================================================================
// Model Data Publik
// =========================================================================

#[derive(Serialize, Clone, Debug)]
pub struct MediaState {
    pub source: String, // "smtc" | "none"; state pemutar internal dirakit di UI
    pub connected: bool,
    pub auth_needed: bool,
    pub has_track: bool,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub video_id: Option<String>,
    pub playlist_id: Option<String>,
    pub url: Option<String>,
    pub playing: bool,
    pub pos: f64,
    pub dur: f64,
    pub cover: Option<String>,
    pub volume: Option<u32>,
    pub muted: bool,
    pub shuffle: Option<bool>,
    pub repeat: Option<String>,     // "NONE" | "ALL" | "ONE"
    pub like_state: Option<String>, // "LIKE" | "DISLIKE" | "INDIFFERENT"
}

#[derive(Serialize, Clone, Debug)]
pub struct StreamInfo {
    pub url: String,
    pub mime: String,
    pub bitrate: u64,
    pub duration: f64,
    pub title: String,
    pub artist: String,
    pub cover: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct LyricLine {
    pub time: f64,
    pub end: Option<f64>,
    pub text: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct LyricsResult {
    pub found: bool,
    pub synced: bool,
    pub instrumental: bool,
    pub source: String,
    pub lines: Vec<LyricLine>,
    pub plain: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct AlbumTrack {
    pub index: usize,
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub duration: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct AlbumDetail {
    pub browse_id: String,
    pub title: String,
    pub artist: String,
    pub year: String,
    pub meta: String,
    pub cover: Option<String>,
    pub url: String,
    pub tracks: Vec<AlbumTrack>,
}

#[derive(Serialize, Clone, Debug)]
pub struct SearchSong {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub album_id: Option<String>,
    pub duration: String,
    pub cover: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct SearchAlbum {
    pub browse_id: String,
    pub title: String,
    pub artist: String,
    pub year: String,
    pub cover: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct SearchResult {
    pub songs: Vec<SearchSong>,
    pub albums: Vec<SearchAlbum>,
}

// =========================================================================
// Pemutar Internal: Stream InnerTube + Fallback SMTC
// =========================================================================

fn as_f64(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.parse().ok())
}

fn as_u64(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str()?.parse().ok())
}

fn runs_text(v: Option<&Value>) -> String {
    let Some(v) = v else { return String::new() };
    if let Some(s) = v.get("simpleText").and_then(|x| x.as_str()) {
        return s.to_string();
    }
    v.get("runs")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

fn best_thumb(v: Option<&Value>) -> Option<String> {
    let arr = v?
        .get("thumbnails")
        .or_else(|| v?.get("musicThumbnailRenderer")?.get("thumbnail")?.get("thumbnails"))
        .and_then(|x| x.as_array())?;
    arr.last()
        .and_then(|t| t.get("url"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
}

fn fallback_smtc() -> R<MediaState> {
    match crate::sys::media()? {
        Some(m) => Ok(MediaState {
            source: "smtc".into(),
            connected: false,
            auth_needed: false,
            has_track: true,
            title: m.title,
            artist: m.artist,
            album: None,
            video_id: None,
            playlist_id: None,
            url: None,
            playing: m.playing,
            pos: m.pos as f64,
            dur: m.dur as f64,
            cover: m.cover,
            volume: None,
            muted: false,
            shuffle: None,
            repeat: None,
            like_state: None,
        }),
        None => Ok(MediaState {
            source: "none".into(),
            connected: false,
            auth_needed: false,
            has_track: false,
            title: String::new(),
            artist: String::new(),
            album: None,
            video_id: None,
            playlist_id: None,
            url: None,
            playing: false,
            pos: 0.0,
            dur: 0.0,
            cover: None,
            volume: None,
            muted: false,
            shuffle: None,
            repeat: None,
            like_state: None,
        }),
    }
}

pub fn state() -> R<MediaState> {
    fallback_smtc()
}

fn player_request(client: &str, video_id: &str) -> R<Value> {
    let (name, version, ua, num, host, extra) = match client {
        "VISIONOS" => (
            "VISIONOS",
            "1.02",
            VISIONOS_UA,
            "101",
            "www.youtube.com",
            json!({
                "deviceMake": "Apple",
                "deviceModel": "RealityDevice17,1",
                "osName": "visionOS",
                "osVersion": "26.5.23O471"
            }),
        ),
        "IOS" => (
            "IOS",
            "20.10.4",
            IOS_UA,
            "5",
            "music.youtube.com",
            json!({
                "deviceModel": "iPhone16,2",
                "osName": "iPhone",
                "osVersion": "18.3.1.22D72"
            }),
        ),
        _ => (
            "ANDROID",
            "20.10.38",
            ANDROID_UA,
            "3",
            "music.youtube.com",
            json!({
                "androidSdkVersion": 30,
                "osName": "Android",
                "osVersion": "11"
            }),
        ),
    };
    let visitor = visitor_data();
    let mut c = json!({
        "clientName": name,
        "clientVersion": version,
        "hl": "en",
        "gl": "US",
        "userAgent": ua
    });
    if let (Some(dst), Some(src)) = (c.as_object_mut(), extra.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    // Tanpa visitorData, YouTube membatasi stream jadi cuplikan ~1 MiB.
    if let (Some(dst), Some(vd)) = (c.as_object_mut(), visitor.as_ref()) {
        dst.insert("visitorData".into(), json!(vd));
    }
    let payload = json!({
        "context": { "client": c },
        "videoId": video_id,
        "contentCheckOk": true,
        "racyCheckOk": true
    });
    let mut req = http_agent(9000)
        .post(&format!("https://{host}/youtubei/v1/player?prettyPrint=false"))
        .set("User-Agent", ua)
        .set("X-YouTube-Client-Name", num)
        .set("X-YouTube-Client-Version", version)
        .set("Origin", &format!("https://{host}"))
        .set("Referer", &format!("https://{host}/"))
        .set("Content-Type", "application/json");
    if let Some(vd) = visitor.as_ref() {
        req = req.set("X-Goog-Visitor-Id", vd);
    }
    let resp = req
        .send_json(payload)
        .map_err(|e| format!("InnerTube player gagal: {e}"))?;
    resp.into_json::<Value>().map_err(err)
}

/// visitorData dari halaman YouTube. Dipakai VISIONOS agar stream tidak
/// dipotong jadi cuplikan dan tidak kena "Sign in to confirm you're not a bot".
/// Hasil sukses di-cache; kegagalan dibiarkan kosong agar percobaan berikutnya
/// mengulang (bukan terkunci tanpa visitorData sampai aplikasi restart).
fn visitor_data() -> Option<String> {
    static C: Mutex<Option<String>> = Mutex::new(None);
    if let Ok(guard) = C.lock() {
        if let Some(v) = guard.as_ref() {
            return Some(v.clone());
        }
    }
    let body = http_agent(8000)
        .get("https://www.youtube.com/")
        .set("User-Agent", INNERTUBE_UA)
        .call()
        .ok()?
        .into_string()
        .ok()?;
    let key = "\"visitorData\":\"";
    let start = body.find(key)? + key.len();
    let rest = &body[start..];
    let end = rest.find('"')?;
    let vd = rest[..end].to_string();
    if let Ok(mut guard) = C.lock() {
        *guard = Some(vd.clone());
    }
    Some(vd)
}

fn parse_player_response(v: &Value) -> R<StreamInfo> {
    let status = v
        .pointer("/playabilityStatus/status")
        .and_then(Value::as_str)
        .unwrap_or("ERROR");
    if status != "OK" {
        let reason = v
            .pointer("/playabilityStatus/reason")
            .and_then(Value::as_str)
            .unwrap_or("tidak ada alasan");
        return Err(format!("YouTube Music menolak lagu ini ({status}): {reason}"));
    }

    let mut formats: Vec<&Value> = Vec::new();
    for key in ["/streamingData/adaptiveFormats", "/streamingData/formats"] {
        if let Some(arr) = v.pointer(key).and_then(Value::as_array) {
            formats.extend(arr.iter());
        }
    }

    let mut best: Option<(&Value, u64)> = None;
    for f in formats {
        let mime = f.get("mimeType").and_then(Value::as_str).unwrap_or("");
        if !mime.starts_with("audio/") || f.get("url").and_then(Value::as_str).is_none() {
            continue;
        }
        let bitrate = f.get("bitrate").and_then(as_u64).unwrap_or(0);
        let score = bitrate + if mime.contains("mp4") { 1_000_000 } else { 0 };
        if best.map(|(_, b)| score > b).unwrap_or(true) {
            best = Some((f, score));
        }
    }

    let (f, _) = best.ok_or("Tidak ada stream audio yang bisa diputar untuk lagu ini.")?;
    let direct = f
        .get("url")
        .and_then(Value::as_str)
        .ok_or("URL stream tidak ditemukan.")?;
    let mime = f
        .get("mimeType")
        .and_then(Value::as_str)
        .unwrap_or("audio/mp4")
        .to_string();
    let total = f.get("contentLength").and_then(as_u64).unwrap_or(0);
    // Browser memakai range terbuka yang selalu ditolak googlevideo, jadi audio
    // dialirkan lewat proxy lokal. URL asli dipakai kalau proxy gagal menyala.
    let url = crate::stream::register(direct, &mime, total)
        .unwrap_or_else(|| direct.to_string());
    let bitrate = f.get("bitrate").and_then(as_u64).unwrap_or(0);
    let title = v
        .pointer("/videoDetails/title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let artist = v
        .pointer("/videoDetails/author")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let duration = v
        .pointer("/videoDetails/lengthSeconds")
        .and_then(as_f64)
        .unwrap_or(0.0);
    let cover = v
        .pointer("/videoDetails/thumbnail/thumbnails")
        .and_then(Value::as_array)
        .and_then(|a| a.last())
        .and_then(|t| t.get("url"))
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    Ok(StreamInfo {
        url,
        mime,
        bitrate,
        duration,
        title,
        artist,
        cover,
    })
}

pub fn stream(video_id: String) -> R<StreamInfo> {
    let vid = video_id.trim();
    if vid.is_empty() {
        return Err("videoId kosong.".into());
    }
    let mut last = String::new();
    for client in ["VISIONOS", "ANDROID", "IOS"] {
        match player_request(client, vid).and_then(|v| parse_player_response(&v)) {
            Ok(info) => return Ok(info),
            Err(e) => last = e,
        }
    }
    Err(last)
}

// =========================================================================
// Lirik Sinkron (LRCLIB + Fallback YouTube Music InnerTube)
// =========================================================================

fn clean_query_piece(s: &str) -> String {
    // Buang embel-embel umum YouTube seperti "(Official Video)", "[Lyrics]", "- Topic", "feat. ..."
    let mut out = s.trim().to_string();
    if let Some(stripped) = out.strip_suffix(" - Topic") {
        out = stripped.trim().to_string();
    }
    for (open, close) in [('(', ')'), ('[', ']')] {
        loop {
            let Some(a) = out.find(open) else { break };
            let Some(b_rel) = out[a..].find(close) else { break };
            let inner = out[a + 1..a + b_rel].to_ascii_lowercase();
            let noisy = [
                "official", "video", "audio", "lyric", "lyrics", "visualizer", "hd", "4k", "mv",
                "remaster", "live", "feat", "ft.",
            ]
            .iter()
            .any(|k| inner.contains(k));
            if noisy {
                out.replace_range(a..=a + b_rel, " ");
            } else {
                break;
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_lrc(lrc: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for raw in lrc.lines() {
        let mut s = raw.trim();
        let mut times = Vec::new();
        while s.starts_with('[') {
            let Some(end) = s.find(']') else { break };
            let tag = &s[1..end];
            if let Some((mm, rest)) = tag.split_once(':') {
                if let (Ok(m), Ok(sec)) = (mm.trim().parse::<f64>(), rest.trim().parse::<f64>()) {
                    times.push(m * 60.0 + sec);
                }
            }
            s = s[end + 1..].trim_start();
        }
        let text = s.trim().to_string();
        for t in times {
            lines.push(LyricLine { time: t, end: None, text: text.clone() });
        }
    }
    lines.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal));
    lines
}

fn lrclib_parse_record(v: &Value) -> Option<LyricsResult> {
    let instrumental = v.get("instrumental").and_then(|x| x.as_bool()).unwrap_or(false);
    if instrumental {
        return Some(LyricsResult {
            found: true,
            synced: false,
            instrumental: true,
            source: "LRCLIB".into(),
            lines: Vec::new(),
            plain: None,
        });
    }
    if let Some(synced) = v.get("syncedLyrics").and_then(|x| x.as_str()).filter(|s| !s.trim().is_empty()) {
        let lines = parse_lrc(synced);
        if !lines.is_empty() {
            return Some(LyricsResult {
                found: true,
                synced: true,
                instrumental: false,
                source: "LRCLIB · Sinkron".into(),
                lines,
                plain: v.get("plainLyrics").and_then(|x| x.as_str()).map(|s| s.to_string()),
            });
        }
    }
    if let Some(plain) = v.get("plainLyrics").and_then(|x| x.as_str()).filter(|s| !s.trim().is_empty()) {
        return Some(LyricsResult {
            found: true,
            synced: false,
            instrumental: false,
            source: "LRCLIB".into(),
            lines: Vec::new(),
            plain: Some(plain.to_string()),
        });
    }
    None
}

fn fetch_lrclib(title: &str, artist: &str, album: Option<&str>, duration: Option<f64>) -> Option<LyricsResult> {
    let agent = http_agent(6000);
    let clean_t = clean_query_piece(title);
    let clean_a = clean_query_piece(artist);
    if clean_t.is_empty() {
        return None;
    }

    // 1. Coba exact match /api/get (bila belum sinkron, simpan sebagai fallback sambil cari versi sinkron)
    let mut exact_fallback: Option<LyricsResult> = None;
    let mut req = agent
        .get("https://lrclib.net/api/get")
        .set("User-Agent", LRCLIB_UA)
        .query("track_name", &clean_t)
        .query("artist_name", &clean_a);
    if let Some(al) = album.map(str::trim).filter(|s| !s.is_empty()) {
        req = req.query("album_name", al);
    }
    if let Some(d) = duration.filter(|d| *d > 5.0) {
        req = req.query("duration", &format!("{}", d.round() as i64));
    }
    if let Ok(resp) = req.call() {
        if let Ok(v) = resp.into_json::<Value>() {
            if let Some(parsed) = lrclib_parse_record(&v) {
                if parsed.synced || parsed.instrumental {
                    return Some(parsed);
                }
                exact_fallback = Some(parsed);
            }
        }
    }

    // 2. Cari di /api/search dan pilih skor terbaik (prioritas syncedLyrics + durasi terdekat)
    let q = if clean_a.is_empty() { clean_t.clone() } else { format!("{clean_t} {clean_a}") };
    if let Ok(resp) = agent
        .get("https://lrclib.net/api/search")
        .set("User-Agent", LRCLIB_UA)
        .query("q", &q)
        .call()
    {
        if let Ok(Value::Array(items)) = resp.into_json::<Value>() {
            let mut best: Option<(f64, LyricsResult)> = None;
            for item in items.iter().take(15) {
                let Some(parsed) = lrclib_parse_record(item) else { continue };
                let mut score = if parsed.synced { 100.0 } else { 40.0 };
                if let (Some(want), Some(got)) = (duration.filter(|d| *d > 5.0), item.get("duration").and_then(|x| x.as_f64())) {
                    let diff = (want - got).abs();
                    // Tolak LRC sinkron yang durasinya meleset terlalu jauh (mis. versi MV/live berbeda)
                    if parsed.synced && diff > 14.0 {
                        continue;
                    }
                    if diff <= 2.5 {
                        score += 40.0;
                    } else if diff <= 6.0 {
                        score += 20.0;
                    } else {
                        score -= diff * 2.5;
                    }
                }
                if best.as_ref().map(|b| score > b.0).unwrap_or(true) {
                    best = Some((score, parsed));
                }
            }
            if let Some((_, res)) = best {
                if res.synced || exact_fallback.is_none() {
                    return Some(res);
                }
            }
        }
    }

    exact_fallback
}

// =========================================================================
// YouTube Music InnerTube API (`youtubei/v1`)
// =========================================================================

fn innertube_post(endpoint: &str, mut payload: Value) -> R<Value> {
    let ctx = json!({
        "client": {
            "clientName": "WEB_REMIX",
            "clientVersion": "1.20250101.01.00",
            "hl": "en",
            "gl": "US"
        }
    });
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("context".into(), ctx);
    }
    let url = format!("https://music.youtube.com/youtubei/v1/{endpoint}?prettyPrint=false");
    let resp = http_agent(9000)
        .post(&url)
        .set("User-Agent", INNERTUBE_UA)
        .set("Origin", "https://music.youtube.com")
        .set("Referer", "https://music.youtube.com/")
        .set("Content-Type", "application/json")
        .send_json(payload)
        .map_err(|e| format!("InnerTube {endpoint} gagal: {e}"))?;
    resp.into_json::<Value>().map_err(err)
}

fn innertube_android_music_post(endpoint: &str, mut payload: Value) -> R<Value> {
    let ctx = json!({
        "client": {
            "clientName": "ANDROID_MUSIC",
            "clientVersion": "7.27.52",
            "androidSdkVersion": 33,
            "hl": "en",
            "gl": "US"
        }
    });
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("context".into(), ctx);
    }
    let url = format!("https://music.youtube.com/youtubei/v1/{endpoint}?prettyPrint=false");
    let resp = http_agent(9000)
        .post(&url)
        .set("User-Agent", "com.google.android.apps.youtube.music/7.27.52 (Linux; U; Android 13) gzip")
        .set("Content-Type", "application/json")
        .send_json(payload)
        .map_err(|e| format!("InnerTube ANDROID_MUSIC {endpoint} gagal: {e}"))?;
    resp.into_json::<Value>().map_err(err)
}

fn find_first_browse_id(v: &Value, prefix: &str) -> Option<String> {
    match v {
        Value::Object(map) => {
            if let Some(Value::String(s)) = map.get("browseId") {
                if s.starts_with(prefix) {
                    return Some(s.clone());
                }
            }
            for val in map.values() {
                if let Some(found) = find_first_browse_id(val, prefix) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => {
            for val in arr {
                if let Some(found) = find_first_browse_id(val, prefix) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn find_object_with_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(map) => {
            if map.contains_key(key) {
                return Some(v);
            }
            for val in map.values() {
                if let Some(found) = find_object_with_key(val, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => {
            for val in arr {
                if let Some(found) = find_object_with_key(val, key) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn parse_ms_val(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64().map(|ms| ms / 1000.0),
        Value::String(s) => s.trim().parse::<f64>().ok().map(|ms| ms / 1000.0),
        _ => None,
    }
}

fn parse_ytm_timed_lyrics(br: &Value) -> Option<LyricsResult> {
    let ld = br
        .pointer("/contents/elementRenderer/newElement/type/componentType/model/timedLyricsModel/lyricsData")
        .or_else(|| find_object_with_key(br, "timedLyricsData"))?;
    let items = ld.get("timedLyricsData")?.as_array()?;
    if items.is_empty() {
        return None;
    }

    let raw_src = ld.get("sourceMessage").and_then(|x| x.as_str()).unwrap_or("").trim();
    let provider = raw_src
        .strip_prefix("Source:")
        .or_else(|| raw_src.strip_prefix("Sumber:"))
        .unwrap_or(raw_src)
        .trim();

    let mut timed_lines = Vec::new();
    let mut plain_lines = Vec::new();

    for item in items {
        let raw_line = item.get("lyricLine").and_then(|x| x.as_str()).unwrap_or("");
        let clean_line = raw_line.replace('\r', "").trim().to_string();
        if !clean_line.is_empty() {
            plain_lines.push(clean_line.clone());
        }
        if let Some(cue) = item.get("cueRange") {
            if let Some(start) = parse_ms_val(cue.get("startTimeMilliseconds")) {
                let end = parse_ms_val(cue.get("endTimeMilliseconds")).filter(|e| *e > start);
                timed_lines.push(LyricLine {
                    time: start,
                    end,
                    text: clean_line,
                });
            }
        }
    }

    let plain = if plain_lines.is_empty() {
        None
    } else {
        Some(plain_lines.join("\n"))
    };

    if timed_lines.len() >= 2 {
        timed_lines.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal));
        let source = if provider.is_empty() {
            "YT Music · Sinkron".to_string()
        } else {
            format!("YT Music · {provider}")
        };
        return Some(LyricsResult {
            found: true,
            synced: true,
            instrumental: false,
            source,
            lines: timed_lines,
            plain,
        });
    }

    plain.map(|p| {
        let source = if provider.is_empty() {
            "YouTube Music".to_string()
        } else {
            format!("YT Music ({provider})")
        };
        LyricsResult {
            found: true,
            synced: false,
            instrumental: false,
            source,
            lines: Vec::new(),
            plain: Some(p),
        }
    })
}

fn fetch_innertube_lyrics(video_id: &str) -> Option<LyricsResult> {
    // 1. Utamakan lirik sinkron bawaan resmi YouTube Music (`ANDROID_MUSIC` timedLyricsData)
    if let Ok(next_val) = innertube_android_music_post("next", json!({ "videoId": video_id, "isAudioOnly": true })) {
        if let Some(browse_id) = find_first_browse_id(&next_val, "MPLYt") {
            if let Ok(br) = innertube_android_music_post("browse", json!({ "browseId": browse_id })) {
                if let Some(parsed) = parse_ytm_timed_lyrics(&br) {
                    return Some(parsed);
                }
            }
        }
    }

    // 2. Fallback lirik teks dari `WEB_REMIX`
    let next_val = innertube_post("next", json!({ "videoId": video_id, "isAudioOnly": true })).ok()?;
    let browse_id = find_first_browse_id(&next_val, "MPLYt")?;
    let br = innertube_post("browse", json!({ "browseId": browse_id })).ok()?;
    let shelf = br
        .pointer("/contents/sectionListRenderer/contents/0/musicDescriptionShelfRenderer")?;
    let text = runs_text(shelf.get("description"));
    if text.trim().is_empty() {
        return None;
    }
    let footer = runs_text(shelf.get("footer"));
    let source = if footer.trim().is_empty() {
        "YouTube Music".to_string()
    } else {
        format!("YT Music ({})", footer.trim())
    };
    Some(LyricsResult {
        found: true,
        synced: false,
        instrumental: false,
        source,
        lines: Vec::new(),
        plain: Some(text),
    })
}

pub fn lyrics(
    title: String,
    artist: String,
    album: Option<String>,
    duration: Option<f64>,
    video_id: Option<String>,
    force: bool,
) -> R<LyricsResult> {
    let cache_key = video_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| format!("vid:{s}"))
        .unwrap_or_else(|| format!("ta:{}|{}", title.to_lowercase(), artist.to_lowercase()));

    if !force {
        if let Ok(c) = lyrics_cache().lock() {
            if let Some(hit) = c.get(&cache_key) {
                return Ok(hit.clone());
            }
        }
    }

    // 1. Prioritas utama: lirik sinkron bawaan resmi YouTube Music per `videoId`
    let ytm_res = video_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(fetch_innertube_lyrics);

    let result = if let Some(r) = ytm_res.as_ref().filter(|r| r.synced) {
        r.clone()
    } else {
        // 2. Jika YTM hanya punya plain atau videoId kosong (SMTC), cari versi sinkron di LRCLIB
        let lrc_res = fetch_lrclib(&title, &artist, album.as_deref(), duration);
        match (lrc_res, ytm_res) {
            (Some(l), _) if l.synced || l.instrumental => l,
            (_, Some(y)) => y,
            (Some(l), None) => l,
            (None, None) => LyricsResult {
                found: false,
                synced: false,
                instrumental: false,
                source: String::new(),
                lines: Vec::new(),
                plain: None,
            },
        }
    };

    if let Ok(mut c) = lyrics_cache().lock() {
        if c.len() > 80 {
            c.clear();
        }
        c.insert(cache_key, result.clone());
    }
    Ok(result)
}

// =========================================================================
// Album Explorer & Search via InnerTube
// =========================================================================

fn parse_album_browse(browse_id: &str, v: &Value) -> R<AlbumDetail> {
    let hdr = v
        .pointer("/contents/twoColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicResponsiveHeaderRenderer")
        .or_else(|| v.pointer("/header/musicDetailHeaderRenderer"))
        .ok_or("Format halaman album YouTube Music tidak ditemukan.")?;

    let title = runs_text(hdr.get("title"));
    let artist = runs_text(hdr.get("straplineTextOne"))
        .trim()
        .to_string();
    let subtitle = runs_text(hdr.get("subtitle"));
    let second_sub = runs_text(hdr.get("secondSubtitle"));
    let cover = best_thumb(hdr.get("thumbnail"));

    // Ekstrak tahun 4 digit dari subtitle (mis. "Album • 2013")
    let year = subtitle
        .split(|c: char| !c.is_ascii_digit())
        .find(|tok| tok.len() == 4 && (tok.starts_with("19") || tok.starts_with("20")))
        .unwrap_or("")
        .to_string();

    let mut tracks = Vec::new();
    let shelf_items = v
        .pointer("/contents/twoColumnBrowseResultsRenderer/secondaryContents/sectionListRenderer/contents/0/musicShelfRenderer/contents")
        .or_else(|| v.pointer("/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicShelfRenderer/contents"))
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();

    for (idx, item) in shelf_items.iter().enumerate() {
        let Some(r) = item.get("musicResponsiveListItemRenderer") else { continue };
        let video_id = r
            .pointer("/playlistItemData/videoId")
            .or_else(|| r.pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/videoId"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if video_id.is_empty() {
            continue;
        }
        let flex = r.get("flexColumns").and_then(|x| x.as_array());
        let t_title = flex
            .and_then(|a| a.first())
            .and_then(|c| c.pointer("/musicResponsiveListItemFlexColumnRenderer/text"))
            .map(|t| runs_text(Some(t)))
            .unwrap_or_default();
        let t_artist_raw = flex
            .and_then(|a| a.get(1))
            .and_then(|c| c.pointer("/musicResponsiveListItemFlexColumnRenderer/text"))
            .map(|t| runs_text(Some(t)))
            .unwrap_or_default();
        let t_artist = if t_artist_raw.trim().is_empty() { artist.clone() } else { t_artist_raw };
        let duration = r
            .pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text")
            .map(|t| runs_text(Some(t)))
            .unwrap_or_default();

        tracks.push(AlbumTrack {
            index: idx + 1,
            video_id,
            title: t_title,
            artist: t_artist,
            duration,
        });
    }

    Ok(AlbumDetail {
        browse_id: browse_id.to_string(),
        title,
        artist,
        year,
        meta: if second_sub.is_empty() { subtitle } else { second_sub },
        cover,
        url: format!("https://music.youtube.com/browse/{browse_id}"),
        tracks,
    })
}

pub fn album(browse_id: String) -> R<AlbumDetail> {
    let bid = browse_id.trim().to_string();
    if bid.is_empty() {
        return Err("Album ID kosong.".into());
    }
    if let Ok(c) = album_cache().lock() {
        if let Some(hit) = c.get(&bid) {
            return Ok(hit.clone());
        }
    }
    let val = innertube_post("browse", json!({ "browseId": bid }))?;
    let detail = parse_album_browse(&bid, &val)?;
    if let Ok(mut c) = album_cache().lock() {
        if c.len() > 40 {
            c.clear();
        }
        c.insert(bid, detail.clone());
    }
    Ok(detail)
}

pub fn track_album(
    video_id: Option<String>,
    title: String,
    artist: String,
    album_hint: Option<String>,
) -> R<AlbumDetail> {
    let key = video_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| format!("v:{s}"))
        .unwrap_or_else(|| format!("t:{}|{}|{}", title.to_lowercase(), artist.to_lowercase(), album_hint.as_deref().unwrap_or("").to_lowercase()));

    if let Ok(c) = track_album_id_cache().lock() {
        if let Some(bid) = c.get(&key) {
            return album(bid.clone());
        }
    }

    let clean_t = clean_query_piece(&title);
    let clean_a = clean_query_piece(&artist);

    // 1. Bila ada nama album, langsung cari album itu di InnerTube
    let mut found_bid: Option<String> = None;
    if let Some(al) = album_hint.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let q = if clean_a.is_empty() { al.to_string() } else { format!("{al} {clean_a}") };
        if let Ok(v) = innertube_post("search", json!({ "query": q, "params": PARAM_ALBUMS })) {
            found_bid = find_first_browse_id(&v, "MPREb_");
        }
    }

    // 2. Fallback: cari lagunya di tab Songs dan ambil link album MPREb_ dari hasil lagu
    if found_bid.is_none() && !clean_t.is_empty() {
        let q = if clean_a.is_empty() { clean_t.clone() } else { format!("{clean_t} {clean_a}") };
        if let Ok(v) = innertube_post("search", json!({ "query": q, "params": PARAM_SONGS })) {
            found_bid = find_first_browse_id(&v, "MPREb_");
        }
    }

    // 3. Fallback terakhir: cari langsung di tab Albums pakai judul + artis
    if found_bid.is_none() && !clean_t.is_empty() {
        let q = if clean_a.is_empty() { clean_t } else { format!("{clean_t} {clean_a}") };
        if let Ok(v) = innertube_post("search", json!({ "query": q, "params": PARAM_ALBUMS })) {
            found_bid = find_first_browse_id(&v, "MPREb_");
        }
    }

    let bid = found_bid.ok_or("Album untuk lagu ini tidak ditemukan di YouTube Music.")?;
    if let Ok(mut c) = track_album_id_cache().lock() {
        if c.len() > 80 {
            c.clear();
        }
        c.insert(key, bid.clone());
    }
    album(bid)
}

fn collect_list_items<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(map) => {
            if let Some(r) = map.get("musicResponsiveListItemRenderer") {
                out.push(r);
                return;
            }
            for val in map.values() {
                collect_list_items(val, out);
            }
        }
        Value::Array(arr) => {
            for val in arr {
                collect_list_items(val, out);
            }
        }
        _ => {}
    }
}

fn parse_search_songs(v: &Value) -> Vec<SearchSong> {
    let mut items = Vec::new();
    collect_list_items(v, &mut items);
    let mut songs = Vec::new();

    for r in items {
        let video_id = r
            .pointer("/playlistItemData/videoId")
            .or_else(|| r.pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/videoId"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if video_id.is_empty() {
            continue;
        }
        let flex = r.get("flexColumns").and_then(|x| x.as_array());
        let title = flex
            .and_then(|a| a.first())
            .and_then(|c| c.pointer("/musicResponsiveListItemFlexColumnRenderer/text"))
            .map(|t| runs_text(Some(t)))
            .unwrap_or_default();
        if title.is_empty() {
            continue;
        }
        let mut artist = String::new();
        let mut album = None;
        let mut album_id = None;
        let mut duration = String::new();

        if let Some(runs) = flex
            .and_then(|a| a.get(1))
            .and_then(|c| c.pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs"))
            .and_then(|x| x.as_array())
        {
            let mut artist_parts = Vec::new();
            for run in runs {
                let txt = run.get("text").and_then(|x| x.as_str()).unwrap_or("").trim();
                if txt.is_empty() || txt == "•" || txt == "·" || txt == "&" || txt == "," || txt == "Song" {
                    continue;
                }
                let bid = run.pointer("/navigationEndpoint/browseEndpoint/browseId").and_then(|x| x.as_str());
                if let Some(b) = bid {
                    if b.starts_with("MPREb_") {
                        album = Some(txt.to_string());
                        album_id = Some(b.to_string());
                        continue;
                    }
                    if b.starts_with("UC") {
                        artist_parts.push(txt.to_string());
                        continue;
                    }
                }
                if txt.contains(':') && txt.chars().all(|c| c.is_ascii_digit() || c == ':') {
                    duration = txt.to_string();
                } else if artist_parts.is_empty() {
                    artist_parts.push(txt.to_string());
                }
            }
            artist = artist_parts.join(", ");
        }

        let cover = best_thumb(r.get("thumbnail"));
        songs.push(SearchSong { video_id, title, artist, album, album_id, duration, cover });
        if songs.len() >= 18 {
            break;
        }
    }
    songs
}

fn parse_search_albums(v: &Value) -> Vec<SearchAlbum> {
    let mut items = Vec::new();
    collect_list_items(v, &mut items);
    let mut albums = Vec::new();

    for r in items {
        let browse_id = r
            .pointer("/navigationEndpoint/browseEndpoint/browseId")
            .and_then(|x| x.as_str())
            .filter(|s| s.starts_with("MPREb_"))
            .map(|s| s.to_string());
        let Some(browse_id) = browse_id else { continue };

        let flex = r.get("flexColumns").and_then(|x| x.as_array());
        let title = flex
            .and_then(|a| a.first())
            .and_then(|c| c.pointer("/musicResponsiveListItemFlexColumnRenderer/text"))
            .map(|t| runs_text(Some(t)))
            .unwrap_or_default();
        let sub = flex
            .and_then(|a| a.get(1))
            .and_then(|c| c.pointer("/musicResponsiveListItemFlexColumnRenderer/text"))
            .map(|t| runs_text(Some(t)))
            .unwrap_or_default();

        let year = sub
            .split(|c: char| !c.is_ascii_digit())
            .find(|tok| tok.len() == 4 && (tok.starts_with("19") || tok.starts_with("20")))
            .unwrap_or("")
            .to_string();

        // Bersihkan "Album • " dan tahun dari baris artis
        let artist = sub
            .split('•')
            .map(str::trim)
            .filter(|p| {
                !p.is_empty()
                    && !matches!(p.to_ascii_lowercase().as_str(), "album" | "ep" | "single")
                    && *p != year
            })
            .collect::<Vec<_>>()
            .join(" · ");

        let cover = best_thumb(r.get("thumbnail"));
        albums.push(SearchAlbum { browse_id, title, artist, year, cover });
        if albums.len() >= 14 {
            break;
        }
    }
    albums
}

pub fn search(query: String, filter: Option<String>) -> R<SearchResult> {
    let q = query.trim().to_string();
    if q.is_empty() {
        return Ok(SearchResult { songs: Vec::new(), albums: Vec::new() });
    }
    let f = filter.unwrap_or_else(|| "songs".into());
    let mut songs = Vec::new();
    let mut albums = Vec::new();

    if f == "songs" || f == "all" {
        let v = innertube_post("search", json!({ "query": q, "params": PARAM_SONGS }))?;
        songs = parse_search_songs(&v);
    }
    if f == "albums" || f == "all" {
        let v = innertube_post("search", json!({ "query": q, "params": PARAM_ALBUMS }))?;
        albums = parse_search_albums(&v);
    }
    Ok(SearchResult { songs, albums })
}

pub fn open_url(url: String) -> R<()> {
    let u = url.trim();
    if !(u.starts_with("https://music.youtube.com/") || u.starts_with("https://www.youtube.com/")) {
        return Err("URL tidak diizinkan.".into());
    }
    let mut cmd = Command::new("cmd");
    cmd.args(["/c", "start", "", u]);
    crate::sys::hide_console(&mut cmd);
    cmd.spawn().map_err(err)?;
    Ok(())
}

// =========================================================================
// Unit Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lrc_handles_timestamps_and_sorting() {
        let raw = "[00:12.50] Baris dua\n[00:05.10] Baris satu\n[01:02.00][01:10.25] Chorus";
        let lines = parse_lrc(raw);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].text, "Baris satu");
        assert!((lines[0].time - 5.10).abs() < 0.01);
        assert_eq!(lines[1].text, "Baris dua");
        assert!((lines[2].time - 62.0).abs() < 0.01);
        assert_eq!(lines[3].text, "Chorus");
        assert!((lines[3].time - 70.25).abs() < 0.01);
    }

    #[test]
    fn clean_query_strips_youtube_noise() {
        assert_eq!(clean_query_piece("Instant Crush (Official Video)"), "Instant Crush");
        assert_eq!(clean_query_piece("Daft Punk - Topic"), "Daft Punk");
        assert_eq!(clean_query_piece("Get Lucky (Radio Edit) [Lyrics]"), "Get Lucky (Radio Edit)");
    }

    #[test]
    fn parse_album_browse_extracts_header_and_tracks() {
        let sample = json!({
            "contents": {
                "twoColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "musicResponsiveHeaderRenderer": {
                                            "title": { "runs": [{ "text": "Random Access Memories" }] },
                                            "straplineTextOne": { "runs": [{ "text": "Daft Punk" }] },
                                            "subtitle": { "runs": [{ "text": "Album • 2013" }] },
                                            "secondSubtitle": { "runs": [{ "text": "13 songs • 74 min" }] },
                                            "thumbnail": { "musicThumbnailRenderer": { "thumbnail": { "thumbnails": [{ "url": "https://img/cover.jpg" }] } } }
                                        }
                                    }]
                                }
                            }
                        }
                    }],
                    "secondaryContents": {
                        "sectionListRenderer": {
                            "contents": [{
                                "musicShelfRenderer": {
                                    "contents": [{
                                        "musicResponsiveListItemRenderer": {
                                            "playlistItemData": { "videoId": "IluRBvnYMoY" },
                                            "flexColumns": [{
                                                "musicResponsiveListItemFlexColumnRenderer": {
                                                    "text": { "runs": [{ "text": "Give Life Back to Music" }] }
                                                }
                                            }],
                                            "fixedColumns": [{
                                                "musicResponsiveListItemFixedColumnRenderer": {
                                                    "text": { "runs": [{ "text": "4:36" }] }
                                                }
                                            }]
                                        }
                                    }]
                                }
                            }]
                        }
                    }
                }
            }
        });
        let detail = parse_album_browse("MPREb_test", &sample).unwrap();
        assert_eq!(detail.title, "Random Access Memories");
        assert_eq!(detail.artist, "Daft Punk");
        assert_eq!(detail.year, "2013");
        assert_eq!(detail.tracks.len(), 1);
        assert_eq!(detail.tracks[0].video_id, "IluRBvnYMoY");
        assert_eq!(detail.tracks[0].duration, "4:36");
    }

    #[test]
    fn parse_ytm_timed_lyrics_extracts_cue_ranges_and_breaks() {
        let sample = json!({
            "contents": {
                "elementRenderer": {
                    "newElement": {
                        "type": {
                            "componentType": {
                                "model": {
                                    "timedLyricsModel": {
                                        "lyricsData": {
                                            "sourceMessage": "Source: Musixmatch",
                                            "timedLyricsData": [
                                                { "lyricLine": "\r", "cueRange": { "startTimeMilliseconds": "0", "endTimeMilliseconds": "13520" } },
                                                { "lyricLine": "Yeah", "cueRange": { "startTimeMilliseconds": "13520", "endTimeMilliseconds": "16240" } },
                                                { "lyricLine": "I've been tryna call", "cueRange": { "startTimeMilliseconds": "16240", "endTimeMilliseconds": "19500" } }
                                            ]
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
        let res = parse_ytm_timed_lyrics(&sample).unwrap();
        assert!(res.found);
        assert!(res.synced);
        assert_eq!(res.source, "YT Music · Musixmatch");
        assert_eq!(res.lines.len(), 3);
        assert_eq!(res.lines[0].text, "");
        assert_eq!(res.lines[0].end, Some(13.52));
        assert_eq!(res.lines[1].text, "Yeah");
        assert!((res.lines[1].time - 13.52).abs() < 0.001);
        assert_eq!(res.lines[1].end, Some(16.24));
    }
}
