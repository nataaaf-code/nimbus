# DESIGN.md — Nimbus ("Coral")

> **Catatan:** Ini adalah spesifikasi desain awal (v1, sebelumnya bernama *Windows Notch*). Untuk arsitektur **Nimbus v0.2.0** terkini (4 tab, Multi-Agent Hub, WASAPI spectrum, dan iCal Google Calendar), lihat [README.md](README.md) dan [docs/HANDOFF.md](docs/HANDOFF.md).

Konsep referensi (prototipe interaktif): https://claude.ai/artifact/QKx4oF8nHMV34zS23WNohy
Prototipe itu adalah sumber kebenaran untuk tampilan dan gerak. Implementasi nyata harus terasa identik.

## 1. Ringkasan
Notch adalah pill hitam yang menempel di tepi atas layar Windows, bergaya dynamic island. Saat idle ia kecil dan menampilkan status ringkas. Saat diklik ia mengembang dengan animasi pegas menjadi panel berisi tab: Musik, Kontrol, AI Agent, Kalender, Catatan, Tugas, Fokus.

Prinsip: ringan, selalu di atas, tidak mengganggu, semua perubahan ukuran dan konten beranimasi halus, satu warna aksen.

## 2. Design tokens
| Token | Nilai | Pemakaian |
|---|---|---|
| `--a` aksen Coral | `#FF5A36` | Satu-satunya aksen: progress, tombol utama, tile aktif, tab aktif |
| `--t` tint | `#FFE6DF` | Chip aktif, banner izin |
| `--k` notch | `#161616` | Badan notch |
| `--k2` | `#1C1C1C` | Latar dalam (log, nav) |
| `--c` kartu | `#262626` | Tile, input, kartu event |
| `--g3` | `#3D3D3D` | Track progress/slider, hover |
| `--g` teks redup | `#A9A9A9` | Label, waktu |
| `--wh` teks | `#F5F4F2` | Teks utama |

Aturan: aksen maksimal sekitar 10% area. Teks di atas aksen selalu `#161616`.
Tipografi: Segoe UI (fallback system-ui); judul 14–16px/600, isi 12–13px, mikro 10–11px; log dan kode pakai Consolas 11px.
Radius: notch bawah 22px (idle) dan 30px (terbuka), kartu 12–20px, kontrol bulat 99px. Telinga lengkung kiri/kanan 16px.

## 3. Anatomi dan state ukuran
Posisi: tengah-atas monitor utama, menempel y=0. Lebar x tinggi:

| State | Ukuran |
|---|---|
| Idle | 230 x 36 |
| Hover | 256 x 40 |
| Alert (agent minta izin) | 290 x 36, berdenyut oranye |
| Terbuka | lebar 520; tinggi Musik 196, Kontrol 256, AI 290 (346 saat ada izin), Kalender 262, Catatan 256, Tugas 278, Fokus 224 |

Isi idle: kiri equalizer (aktif saat musik main), kanan titik status agent + teks ("opencode 62%" atau "Perlu izin").
Tab bar pill di bawah (7 tombol, 66px) dengan indikator oranye yang meluncur antar tab.

## 4. Motion
- Resize notch: 600ms `cubic-bezier(.34,1.35,.4,1)` (overshoot pegas) untuk lebar, tinggi, dan radius.
- Layer idle/terbuka: 400ms `cubic-bezier(.2,.8,.2,1)`; konten masuk dengan delay 120ms, dari blur 8px + translateY(-8px) + scale .97 ke normal.
- Panel tab: geser 18px + fade; anak panel muncul bertahap (stagger 50ms, `rise` 500ms).
- Indikator tab: 500ms spring `cubic-bezier(.34,1.4,.4,1)`.
- Equalizer 4 batang 800ms alternate; art album melayang 3s saat musik main; ring fokus 1s linear; alert pulse 1.6s.
- Hormati `prefers-reduced-motion`: matikan overshoot, blur, dan loop; sisakan fade 150ms.

## 5. Modul
**Musik.** Now Playing dari Windows SMTC (tanpa login) + opsional Spotify Web API (OAuth PKCE) untuk playlist/like. Kontrol prev/play/next, progress, art. Jika belum terhubung tampil overlay blur dengan tombol "Hubungkan Spotify".
**Kontrol.** Slider kecerahan dan volume (gaya isi penuh, tanpa thumb), tile toggle Wi-Fi, Bluetooth, Night light, Jangan ganggu. Perubahan langsung diterapkan ke sistem.
**AI Agent.** Chip agent (opencode, antigravity), status, tugas aktif, progress, log 4 baris, kolom kirim perintah. Permintaan izin tampil sebagai banner tint dengan Izinkan/Tolak dan memicu state Alert.
**Kalender.** Google Calendar (OAuth loopback + PKCE, Calendar API v3). Strip 7 hari, daftar acara hari terpilih, acara berikutnya bergaris aksen. Overlay "Hubungkan Google" saat belum login.
**Catatan.** Textarea cepat dengan autosave (debounce 600ms), penghitung karakter, tombol "Kirim ke agent" yang meneruskan isi catatan sebagai perintah ke agent aktif.
**Tugas.** Daftar todo dengan checkbox beranimasi (centang menggambar, teks dicoret), progress bar selesai/total, tambah dengan Enter.
**Fokus.** Timer pomodoro: ring progress, Fokus 25 / Istirahat 5, Mulai/Jeda/Reset, hitungan sesi hari ini.

## 6. Arsitektur yang disarankan
- Tauri 2 (Rust) + UI web (React + TypeScript + Tailwind atau CSS variables seperti prototipe). UI dipindah langsung dari prototipe.
- Jendela: tanpa frame, transparan, always-on-top, skip taskbar, posisi tengah-atas. Saat idle, area di luar pill harus click-through (mis. `WS_EX_TRANSPARENT` dinamis atau region hit-test); saat terbuka, klik di luar menutup.
- Penyimpanan lokal (catatan, tugas, token) di app data dengan token terenkripsi (Windows Credential Manager / DPAPI).

| Fitur | Integrasi Windows |
|---|---|
| Volume | Core Audio (`IAudioEndpointVolume`) via crate `windows` |
| Kecerahan | WMI `WmiMonitorBrightnessMethods` (laptop) / DDC-CI (monitor eksternal) |
| Wi-Fi, Bluetooth | WinRT radio API (`Windows.Devices.Radios`) |
| Now Playing | SMTC `GlobalSystemMediaTransportControlsSessionManager` |
| Night light, Jangan ganggu | Setting sistem; verifikasi API yang tersedia di versi Windows target |

## 7. AI control di host
Gunakan antarmuka adapter agar agent mudah ditambah:
```ts
interface AgentAdapter {
  id: string;                       // "opencode" | "antigravity"
  status(): Promise<{state: "idle"|"running"|"waiting"; task?: string; progress?: number}>;
  send(prompt: string): Promise<void>;
  onEvent(cb: (e: LogEvent | PermissionRequest) => void): () => void;
  respond(permissionId: string, allow: boolean): Promise<void>;
}
```
- opencode: cek dokumentasi terbaru untuk mode server/SDK-nya dan hubungkan lewat HTTP/event stream lokal.
- antigravity: verifikasi dulu apakah ada API/CLI resmi untuk kontrol; jika tidak, mulai dengan adapter read-only (deteksi proses dan tail log) lalu tingkatkan.
- Keamanan: kontrol agent hanya dari mesin lokal (bind 127.0.0.1), setiap aksi berisiko (menjalankan perintah, menulis file) wajib lewat banner Izinkan/Tolak, simpan riwayat keputusan, jangan pernah auto-approve secara default.

## 8. Aksesibilitas
Semua kontrol bisa dioperasikan keyboard (Tab/Enter/Esc), ada fokus ring aksen, kontras teks minimal 4.5:1, label ARIA pada tombol ikon, hotkey global (mis. `Ctrl+Alt+Space`) untuk buka/tutup notch.

## 9. Ide fitur berikutnya
Riwayat clipboard, notifikasi ringkas (toast di dalam notch), cuaca, monitor CPU/RAM/jaringan, peluncur cepat (aplikasi/file), screenshot cepat, kontrol multi-monitor, tema/aksen alternatif (Lime, Biru, Ungu dari palet yang sama).

Implementasi lanjutan yang sudah masuk: hotkey global `Ctrl+Alt+Space`, tray dan autostart Windows, clipboard history lokal-only, serta ringkasan CPU/RAM/jaringan di tab Catatan. Notifikasi ringkas masih perlu adapter Windows yang nyata sebelum ditampilkan sebagai fitur selesai.

## 10. Kriteria selesai (MVP)
1. Notch muncul tengah-atas, idle/hover/terbuka dengan animasi sesuai bagian 3–4, 60 fps.
2. Tujuh tab berfungsi; Musik, Kontrol, Catatan, Tugas, Fokus bekerja tanpa login.
3. Kalender terhubung ke Google dan menampilkan acara asli.
4. Minimal satu agent (opencode) terhubung: status, log, kirim perintah, dan gerbang izin berfungsi.
5. Klik di luar menutup notch; saat idle klik menembus ke aplikasi di bawahnya.
