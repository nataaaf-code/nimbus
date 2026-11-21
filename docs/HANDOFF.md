# HANDOFF — Nimbus (proses, status, dan cara melanjutkan)

Dokumen ini dibuat agar pekerjaan pada proyek **Nimbus** (sebelumnya *Windows Notch*) bisa dilanjutkan oleh sesi/akun lain tanpa konteks percakapan.

## 1. Latar belakang
Pengguna menginginkan floating notch (dynamic island) untuk Windows: animasi halus, kontrol sistem (kecerahan, volume, Wi-Fi, Bluetooth), media + audio spectrum, Google Calendar, catatan, todo, fokus, serta hub AI agent. Palet: Coral oranye (`#FF5A36`, tint `#FFE6DF`).

- **Revisi v2 (2026-11-10):** Rombak total UI/UX ke HTML + CSS + JS modular (`ui/css/app.css`, `ui/js/*.js`) dengan 4 tab (**Agen, Media, Sistem, Hari ini**), jendela tetap (`760×560`), dan hub **Claude Code** (`cc.rs`, `win.rs`).
- **Rilis v0.2.0 — Nimbus (2026-11-19):** Upgrade besar menjadi **Nimbus Multi-Provider Agent Hub** (`agents.rs`) yang menyatukan **Claude Code**, **OpenCode**, **Antigravity**, **Codex**, dan **Gemini CLI** (termasuk install CLI, login OAuth/Device/API key dari dalam notch, rename/hapus sesi terintegrasi, lampiran, switcher model/effort/mode, fork, dan pengecekan provider paralel dengan timeout), spektrum audio WASAPI real-time (`spectrum.rs`), serta Google Calendar via secret iCal URL (`google.rs`).

## 2. Keputusan desain (v0.2.0)
- UI modular vanilla ES modules (`ui/css/app.css`, `ui/js/*.js`), tanpa framework/bundler, 4 tab (**Agen, Media, Sistem, Hari ini**), jendela tetap `760×560` dengan hit-test tembus-klik berbasis rect (`set_hit`) — mencegah lag resize jendela.
- **Tab Agen (`agents.rs`, `cc.rs`, `agents.js`)**: daftar sesi terpadu lintas provider, filter provider & pencarian, halaman **Providers & Auth** untuk install CLI / login / API key, detail sesi dengan timeline tool-call live, gerbang izin Claude Code (`PermissionRequest` Izinkan/Tolak di notch atau alert pill), rename/hapus sesi, lampiran file/gambar, mode fork, Stop (kill pohon proses), serta buka folder/VS Code/terminal.
- **`src-tauri/src/agents.rs`**: registry provider (`PROVIDER_DEFS`), deteksi biner, auth & model sync dengan background refresh + hard timeout agar snapshot tidak pernah macet, scanner sesi lokal (OpenCode, Codex, Gemini, Antigravity), dan runner headless streaming.
- **`src-tauri/src/cc.rs`**: parser transcript JSONL Claude Code inkremental, server HTTP hook di `127.0.0.1:47821` dengan token rahasia, scanner proses `claude.exe` native (Win32 Toolhelp32), pemasang/pencabut hook ke `~/.claude/settings.json`.
- **`src-tauri/src/win.rs`**: FFI Win32 langsung (proses, CPU/RAM, `GetExtendedTcpTable`, `kill_tree`, picker file/folder, deteksi browser di bawah notch untuk line mode).
- **`src-tauri/src/spectrum.rs`**: WASAPI loopback capture + Goertzel FFT 52 band ~60 fps untuk tab Media dan mini-spectrum saat idle.
- **`src-tauri/src/google.rs`**: parser feed iCal rahasia Google Calendar (`ics_url` di `%APPDATA%\Nimbus\config.json`), tanpa perlu setup OAuth client.
- **Pemutar musik (`ytm.rs`, `stream.rs`, `media.js`)**: tanpa aplikasi eksternal. Pencarian/album/lirik lewat InnerTube + LRCLIB, audio dialirkan lewat proxy HTTP lokal (`127.0.0.1:<port>`) karena googlevideo menolak `Range: bytes=0-` dari browser. Klien `VISIONOS` + `visitorData` dipakai agar stream tidak dipotong jadi cuplikan ~1 MiB.

## 3. Cara kerja hub Agent (`cc.rs` & `agents.rs`)
1. **Transcript & session watch**: `cc.rs` memindai `~/.claude/projects/*/*.jsonl` (atau `$CLAUDE_CONFIG_DIR`) secara inkremental dari offset terakhir. `agents.rs` menggabungkannya dengan sesi OpenCode (`opencode session list`), Codex (`~/.codex/sessions`), Gemini (`~/.gemini/tmp`), dan Antigravity (`~/.gemini/antigravity/brain`).
2. **Server hook HTTP** (`127.0.0.1:47821`, token acak 160-bit disimpan di `%APPDATA%\Nimbus\cc_hook.json`): endpoint `POST /hook` menerima event hook Claude Code (`SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse(Failure)`, `Notification`, `Stop(Failure)`, `SessionEnd`, `PermissionRequest`). Hanya `PermissionRequest` yang **memblokir** (channel `mpsc`, timeout 15 menit untuk sesi terkelola Nimbus / 3 menit untuk sesi luar).
3. **Pemasangan hook**: `cc_hooks("install")` menulis ke `~/.claude/settings.json` (level user), tipe `http`, URL `http://127.0.0.1:PORT/hook`, header `x-notch-token`. Dipasang secara eksplisit oleh pengguna melalui tombol di UI. Jika Nimbus mati, hook bersifat non-blocking (`connection-refused` → Claude Code lanjut normal di terminal/VS Code).
4. **Sesi headless**: `ag_run` / `cc_run` menjalankan masing-masing CLI (`claude -p`, `opencode run`, `agy -p`, `codex exec`, `gemini -p`) secara headless, menginjeksi API key dari `%APPDATA%\Nimbus\provider_keys.json` jika ada, dan mem-parse stream JSON ke timeline secara real-time.
5. **Pemetaan proses↔sesi**: `GetExtendedTcpTable` memetakan koneksi hook ke PID `claude.exe` pengirim dan memantau CPU/RAM/proses anak.

## 4. Bug besar yang ditemukan & diperbaiki (penting dibaca sebelum lanjut)
**CSS pointer-events leak** (`ui/css/app.css`): aturan `.view.on { pointer-events:auto }`, `.page.on { pointer-events:auto }`, `.menu.on { pointer-events:auto }` awalnya berlaku tanpa syarat — tidak peduli apakah panel "open" (`.lay[data-lay="open"]`) sedang aktif. Akibatnya, saat notch mengecil ke mode alert/idle, konten tab tersembunyi masih bisa mencegat klik.

**Perbaikan**: `pointer-events:auto` untuk `.view.on`/`.page.on`/`.menu.on` di-scope lewat `.lay[data-lay="open"].on .view.on` (begitu juga `.page`/`.menu`), dan `setOpen(false)` menutup paksa `#dMenu` via `core.js`.

**Jika menambah layer/tab baru**: pastikan setiap elemen `position:absolute` dengan state `.on` sendiri di-scope `pointer-events`-nya ke ancestor yang benar-benar menentukan visibilitas pada ukuran notch saat ini.

## 5. Status fitur (v0.2.0)
| Fitur | Implementasi | Status |
|---|---|---|
| Notch ukuran tetap, hit-test tembus-klik, line mode | `main.rs`, `win.rs`, `core.js` | Berfungsi & teruji |
| Tab Agen: multi-provider (Claude, OpenCode, Antigravity, Codex, Gemini) | `agents.rs`, `cc.rs`, `agents.js` | Berfungsi (Claude & OpenCode paling teruji; Codex/Gemini/Antigravity lewat adapter yang sama) |
| Providers & Auth: deteksi CLI, install, login, API key, sync model | `agents.rs`, `agents.js` | Berfungsi; pengecekan paralel + timeout mencegah UI stuck |
| Gerbang izin Claude Code (alert pill + kartu detail) | `cc.rs`, `agents.js` | Berfungsi & teruji |
| Tab Media (pemutar InnerTube + antrean + lirik sinkron + spektrum WASAPI 52-band) | `ytm.rs`, `stream.rs`, `spectrum.rs`, `sys.rs`, `media.js` | Berfungsi & teruji |
| Tab Sistem (kecerahan/volume/Wi-Fi/BT/monitor/autostart) | `sys.rs`, `win.rs`, `system.js` | Berfungsi (4 test hardware di `#[ignore]` untuk CI) |
| Tab Hari ini (Kalender iCal/Tugas/Catatan/Fokus/Clipboard) | `google.rs`, `today.js` | Berfungsi |

## 6. Verifikasi & CI
- `cargo test --locked --manifest-path src-tauri/Cargo.toml`: seluruh unit test non-hardware lulus; 4 smoke test hardware di `sys.rs` diberi `#[ignore]` dan dapat dijalankan manual dengan `-- --ignored`.
- CI GitHub Actions (`.github/workflows/ci.yml`) menjalankan `cargo test` dan `cargo build` di `windows-latest`.
- `npm run build` menghasilkan installer NSIS `Nimbus_0.2.0_x64-setup.exe` di `src-tauri/target/release/bundle/nsis/`.

## 7. Langkah pengembangan berikutnya
1. Tambah pengujian end-to-end lebih lanjut untuk adapter **Codex**, **Gemini CLI**, dan **Antigravity** pada Windows host nyata.
2. Migrasi penyimpanan `%APPDATA%\Nimbus\provider_keys.json` dari plain JSON ke Windows DPAPI (`CryptProtectData`) sesuai catatan di [SECURITY.md](../SECURITY.md).
3. Pertimbangkan dukungan DPI >100% dan multi-monitor yang lebih komprehensif.
4. Pertimbangkan opsi hook Claude Code level-project (`.claude/settings.json`) selain level-user.

## 8. Melanjutkan di sesi/akun lain
Buka folder proyek `nimbus`, lalu gunakan prompt awal seperti ini:
```
Lanjutkan proyek Nimbus di folder ini. Baca README.md dan docs/HANDOFF.md dulu (terutama §2–4 mengenai arsitektur Multi-Provider Agent Hub v0.2.0 dan scoping CSS pointer-events). Periksa bagian 7 di docs/HANDOFF.md untuk prioritas berikutnya, uji perubahan dengan cargo test / npm run dev, dan perbarui dokumentasi bila ada perubahan fitur.
```
