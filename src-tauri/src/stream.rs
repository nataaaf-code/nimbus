//! Server stream audio lokal untuk pemutar Nimbus.
//!
//! googlevideo menolak permintaan range terbuka (`Range: bytes=0-`) dengan 403,
//! sementara range terbatas (`bytes=0-1023`) dilayani normal. Browser selalu
//! memakai range terbuka saat memuat `<audio>`, jadi permintaan browser
//! diterjemahkan di sini: klien lokal menerima range standar, data diambil dari
//! upstream dalam potongan terbatas.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// Ukuran potongan yang diminta ke upstream. Klien VISIONOS menerima range
/// berapa pun, tapi potongan kecil dipakai agar fallback ANDROID/IOS (yang
/// menolak range di atas ~1 MiB) tetap bisa diputar.
const WINDOW: u64 = 512 * 1024;
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

struct Entry {
    url: String,
    mime: String,
    total: u64,
}

struct Proxy {
    port: u16,
    entries: Mutex<HashMap<String, Entry>>,
}

static PROXY: OnceLock<Option<Proxy>> = OnceLock::new();

fn agent() -> &'static ureq::Agent {
    static A: OnceLock<ureq::Agent> = OnceLock::new();
    A.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(20))
            .build()
    })
}

fn proxy() -> Option<&'static Proxy> {
    PROXY.get_or_init(start).as_ref()
}

fn start() -> Option<Proxy> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            std::thread::spawn(move || {
                if let Some(p) = proxy() {
                    let _ = serve(p, conn);
                }
            });
        }
    });
    Some(Proxy {
        port,
        entries: Mutex::new(HashMap::new()),
    })
}

/// Daftarkan URL googlevideo dan kembalikan URL lokal yang bisa diputar
/// `<audio>`. `None` berarti proxy tidak tersedia; pemanggil sebaiknya memakai
/// URL asli sebagai fallback.
pub fn register(url: &str, mime: &str, total: u64) -> Option<String> {
    let p = proxy()?;
    let total = if total > 0 { total } else { probe(url) };
    if total == 0 {
        return None;
    }
    let token = format!(
        "{:016x}{:016x}",
        rand::random::<u64>(),
        rand::random::<u64>()
    );
    p.entries.lock().ok()?.insert(
        token.clone(),
        Entry {
            url: url.to_string(),
            mime: mime.to_string(),
            total,
        },
    );
    Some(format!("http://127.0.0.1:{}/s/{}", p.port, token))
}

/// Panjang file kalau `contentLength` tidak dikirim InnerTube.
fn probe(url: &str) -> u64 {
    let resp = match agent()
        .get(url)
        .set("Range", "bytes=0-0")
        .set("User-Agent", UA)
        .call()
    {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(_) => return 0,
    };
    resp.header("Content-Range")
        .and_then(|v| v.rsplit_once('/'))
        .and_then(|(_, total)| total.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

fn serve(p: &Proxy, mut s: TcpStream) -> std::io::Result<()> {
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(300)));
    let _ = s.set_nodelay(true);

    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        if head.len() > 16 * 1024 {
            return Ok(());
        }
        match s.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => head.extend_from_slice(&buf[..n]),
            Err(_) => return Ok(()),
        }
    }
    let head = String::from_utf8_lossy(&head);
    let mut request = head.lines().next().unwrap_or("").split_whitespace();
    let method = request.next().unwrap_or("");
    let path = request.next().unwrap_or("");
    let token = path
        .split('?')
        .next()
        .unwrap_or("")
        .strip_prefix("/s/")
        .unwrap_or("");

    let entry = p
        .entries
        .lock()
        .ok()
        .and_then(|m| m.get(token).map(|e| (e.url.clone(), e.mime.clone(), e.total)));
    let Some((url, mime, total)) = entry else {
        return reply(&mut s, "404 Not Found", &[], &[]);
    };

    let (status, start, end) = match parse_range(header(&head, "range").unwrap_or(""), total) {
        RangeSpec::Bounded(a, b) => ("206 Partial Content", a, b),
        RangeSpec::Full => ("200 OK", 0, total - 1),
        RangeSpec::Invalid => {
            let range = format!("bytes */{total}");
            return reply(
                &mut s,
                "416 Range Not Satisfiable",
                &[("Content-Range", range)],
                &[],
            );
        }
    };

    let mut headers = vec![
        ("Content-Type", mime),
        ("Content-Length", (end - start + 1).to_string()),
        ("Accept-Ranges", "bytes".to_string()),
        ("Cache-Control", "no-store".to_string()),
        ("Access-Control-Allow-Origin", "*".to_string()),
        ("Connection", "close".to_string()),
    ];
    if status.starts_with("206") {
        headers.push(("Content-Range", format!("bytes {start}-{end}/{total}")));
    }
    reply(&mut s, status, &headers, &[])?;

    if method.eq_ignore_ascii_case("HEAD") {
        return Ok(());
    }
    pump(&url, &mut s, start, end)
}

fn pump(url: &str, s: &mut TcpStream, mut pos: u64, end: u64) -> std::io::Result<()> {
    let mut buf = [0u8; 64 * 1024];
    while pos <= end {
        let stop = (pos + WINDOW - 1).min(end);
        let range = format!("bytes={pos}-{stop}");
        let resp = match agent()
            .get(url)
            .set("Range", &range)
            .set("User-Agent", UA)
            .set("Accept-Encoding", "identity")
            .call()
        {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            // Upstream bermasalah: putuskan saja, browser akan mengulang.
            Err(_) => return Ok(()),
        };
        if resp.status() >= 300 {
            return Ok(());
        }
        let mut body = resp.into_reader();
        let mut got = 0u64;
        loop {
            match body.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    s.write_all(&buf[..n])?;
                    got += n as u64;
                }
                Err(_) => return Ok(()),
            }
        }
        if got == 0 {
            return Ok(());
        }
        pos = pos.saturating_add(got);
    }
    Ok(())
}

fn reply(
    s: &mut TcpStream,
    status: &str,
    headers: &[(&str, String)],
    body: &[u8],
) -> std::io::Result<()> {
    let mut out = format!("HTTP/1.1 {status}\r\n");
    for (key, value) in headers {
        out.push_str(key);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    s.write_all(out.as_bytes())?;
    if !body.is_empty() {
        s.write_all(body)?;
    }
    Ok(())
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim())
    })
}

enum RangeSpec {
    /// Tidak ada header range, atau formatnya tidak dipahami: kirim utuh.
    Full,
    Bounded(u64, u64),
    Invalid,
}

fn parse_range(value: &str, total: u64) -> RangeSpec {
    let Some(spec) = value.trim().strip_prefix("bytes=") else {
        return RangeSpec::Full;
    };
    let spec = spec.trim();
    // Multi-range tidak didukung browser untuk media; layani utuh saja.
    if spec.contains(',') {
        return RangeSpec::Full;
    }
    let Some((first, last)) = spec.split_once('-') else {
        return RangeSpec::Full;
    };
    let first = first.trim();
    let last = last.trim();

    let (start, end) = if first.is_empty() {
        let Ok(suffix) = last.parse::<u64>() else {
            return RangeSpec::Full;
        };
        if suffix == 0 {
            return RangeSpec::Invalid;
        }
        (total.saturating_sub(suffix), total.saturating_sub(1))
    } else {
        let Ok(start) = first.parse::<u64>() else {
            return RangeSpec::Full;
        };
        let end = if last.is_empty() {
            total.saturating_sub(1)
        } else {
            match last.parse::<u64>() {
                Ok(end) => end.min(total.saturating_sub(1)),
                Err(_) => return RangeSpec::Full,
            }
        };
        (start, end)
    };

    if start >= total || start > end {
        RangeSpec::Invalid
    } else {
        RangeSpec::Bounded(start, end)
    }
}
