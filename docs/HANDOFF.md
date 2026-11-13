# HANDOFF — proses, status, dan cara melanjutkan

Dokumen ini dibuat agar pekerjaan bisa dilanjutkan oleh sesi/akun lain tanpa konteks percakapan.

## 1. Latar belakang
Pengguna ingin notch (dynamic island) untuk Windows: animasi halus, kontrol sistem (kecerahan, volume, Wi-Fi), Spotify, Google Calendar, catatan, todo, fokus. Palet: Coral oranye (`#FF5A36`, tint `#FFE6DF`).

**Revisi besar 2026-11-10 (v2):** Permintaan pengguna — "rombak total UI/UX dan semua fiturnya, khusus untuk AI agent jadi monitor agent AI di host ini, bisa lihat prosesnya dan full control dari sini". Jawaban: desain ulang total (visual bebas asal Coral dipertahankan, fitur lama dipertahankan semua) + hub baru khusus **Claude Code** (bukan lagi opencode/antigravity — itu keputusan eksplisit pengguna untuk revisi ini).

## 2. Keputusan desain v2
- UI lama (prototipe satu-file 75KB, 7 tab datar) **ditinggalkan**. UI baru: HTML + CSS + JS modular (`ui/css/app.css`, `ui/js/*.js`), 4 tab (**Agen, Media, Sistem, Hari ini**), jendela notch berukuran **tetap** (760×560) dengan hit-test tembus-klik berbasis rect yang dilaporkan JS ke Rust — sebelumnya jendela di-resize tiap ganti state (sumber lag).
- Tab **Agen** jadi pusat: daftar sesi Claude Code (dari manapun — terminal, VS Code, headless), detail per sesi dengan timeline tool-call live, gerbang izin Izinkan/Tolak langsung di notch (termasuk sebagai alert pill tanpa buka panel), kirim instruksi lanjutan (`claude -p --resume`), Stop (kill proses+anak), buka folder/VS Code/terminal.
- Modul `src-tauri/src/cc.rs` (baru, ~800 baris): parser transcript JSONL inkremental, server HTTP hook di `127.0.0.1:47821` dengan token rahasia, scanner proses `claude.exe` native (Win32 Toolhelp32, bukan WMI/PowerShell), pemasang/pencabut hook ke `~/.claude/settings.json`.
- Modul `src-tauri/src/win.rs` (baru): FFI Win32 langsung (proses, CPU/RAM, GetExtendedTcpTable untuk memetakan koneksi hook ke PID, kill_tree, deteksi browser di bawah notch) — menggantikan sys.rs lama yang sebagian pakai `powershell.exe` tiap polling (berat).
- `sys::system_stats` diganti native (`GetSystemTimes`, `GetIfTable2`), tanpa spawn PowerShell lagi.
- Fitur lama (Musik SMTC, Kontrol volume/kecerahan/Wi-Fi/Bluetooth, Kalender Google, Catatan, Tugas, Fokus, Clipboard, autostart, tray, hotkey global) **semua dipertahankan**, dipindah ke tab Media/Sistem/Hari ini.
- Agent opencode & antigravity **dihapus** dari UI dan backend (perintah `agent_send`/`agent_usage`/`agent_reset` dan `opencode_command`/`antigravity_command` di `main.rs` lama tidak ada lagi). `DESIGN.md` §7 (opencode/antigravity) sudah usang — abaikan, rujukan AI agent sekarang adalah bagian 3 handoff ini.

## 3. Cara kerja hub Claude Code (`cc.rs`)
1. **Transcript watch**: scan `~/.claude/projects/*/*.jsonl` (atau `$CLAUDE_CONFIG_DIR`) tiap polling (~1–2.5 dtk saat tab Agen terbuka), baca inkremental dari offset terakhir (bukan re-parse penuh). Menghasilkan judul, token, cwd, branch, timeline tool-call per sesi. Ini bekerja **tanpa hook terpasang** — itu sebabnya daftar sesi & detail sudah terisi walau hook belum dipasang pengguna.
2. **Server hook HTTP** (`127.0.0.1:47821`, token acak 160-bit disimpan di `%APPDATA%\WindowsNotch\cc_hook.json`): endpoint `POST /hook` menerima event hook Claude Code (`SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse(Failure)`, `Notification`, `Stop(Failure)`, `SessionEnd`, `PermissionRequest`). Hanya `PermissionRequest` yang **memblokir** (menunggu keputusan user via channel `mpsc`, timeout 15 menit utk sesi terkelola notch / 3 menit utk sesi luar) — event lain non-blocking (balas kosong langsung), sesuai dokumentasi resmi hooks.
3. **Pemasangan hook**: perintah `cc_hooks("install")` menulis ke `~/.claude/settings.json` (level user, berlaku semua project), tipe `http`, URL `http://127.0.0.1:PORT/hook`, header token. **Belum pernah dipasang otomatis** — tombol "Pasang" di banner/pengaturan harus diklik manual oleh pengguna. Jika notch mati, hook tetap non-blocking (connection-refused → Claude Code lanjut normal, izin ditanyakan seperti biasa di terminal/VS Code) — diverifikasi lewat dokumentasi resmi (`onFailure` default `continue`).
4. **Sesi headless**: `cc_run` menjalankan `claude -p --output-format stream-json --verbose [--resume ID] [--model M] [--permission-mode M]`, stdin diisi prompt lalu ditutup. NDJSON stdout di-parse untuk `session_id`/`result`. Biner Claude dicari via `NOTCH_CLAUDE_BIN` → `~/.local/bin/claude.exe` → PATH → npm global → ekstensi VS Code/Cursor/Windsurf terbaru (`resources/native-binary/claude.exe`).
5. **Pemetaan proses↔sesi**: `GetExtendedTcpTable` memetakan koneksi hook (port klien) ke PID `claude.exe` pengirim; juga dipakai untuk scan semua `claude.exe` berjalan (CPU/RAM/proses anak) terlepas dari hook.

## 4. Bug besar yang ditemukan & diperbaiki (penting dibaca sebelum lanjut)
**CSS pointer-events leak** (`ui/css/app.css`, awalnya baris ~180–182 & ~292): aturan `.view.on { pointer-events:auto }`, `.page.on { pointer-events:auto }`, `.menu.on { pointer-events:auto }` berlaku **tanpa syarat** — tidak peduli apakah panel "open" (`.lay[data-lay="open"]`) sedang aktif. Karena state tab/page (`.on`) tersimpan independen dari mode notch (idle/live/alert/open), begitu notch mengecil, konten tab terakhir yang dibuka (mis. tombol di tab Sistem) **tetap bisa menerima klik** di manapun ia bertumpukan secara geometris dengan kotak kecil notch — menembus `overflow:hidden` karena `pointer-events` adalah properti yang di-inherit dan override lokal menang atas `none` di ancestor manapun. Akibatnya: klik pada pil kecil/alert bisa salah sasaran ke kontrol tab tersembunyi.

**Perbaikan**: `pointer-events:auto` untuk `.view.on`/`.page.on`/`.menu.on` sekarang di-scope lewat selector turunan `.lay[data-lay="open"].on .view.on { ... }` (begitu juga `.page`/`.menu`), sehingga hanya aktif ketika panel open sungguh-sungguh sedang ditampilkan. Juga ditambah: `setOpen(false)` menutup paksa `#dMenu` (dropdown "⋯") via `core.js`.

**Cara ditemukan**: diuji lewat WebView2 remote debugging (Chrome DevTools Protocol, `--remote-debugging-port`) — klik native (`SendInput`) dan pengecekan `elementFromPoint`/`getComputedStyle` menunjukkan target klik di tombol "Izinkan" (mode alert, panel kecil) ternyata jatuh ke elemen `.mon` (monitor CPU/RAM di tab Sistem, panel besar tersembunyi). Setelah fix, diverifikasi ulang dengan skenario terburuk (tab tersimpan = "system", notch mengecil ke mode alert) — klik asli (SendInput, bukan simulasi CDP) tepat kena `#alAllow`, hook menerima respons `{"decision":{"behavior":"allow"}}` sesuai skema resmi Claude Code.

**Jika menambah layer/tab baru**: pastikan setiap elemen yang `position:absolute` dan punya state "on" sendiri (lepas dari mode notch) di-scope pointer-events-nya ke ancestor yang benar-benar menentukan apakah ia *terlihat pada ukuran notch saat ini* — jangan hanya andalkan opacity/animasi, itu tidak memengaruhi hit-testing.

## 5. Status fitur (v2)
| Fitur | Implementasi | Status |
|---|---|---|
| Notch ukuran tetap, hit-test tembus-klik | `main.rs` (`input_loop`, `set_hit`), `core.js` | Berfungsi, teruji (lihat §4) |
| Tab Agen: daftar sesi, detail, timeline tool-call | `cc.rs` + `agents.js` | Berfungsi, teruji dgn transcript nyata (lihat §6) |
| Gerbang izin (alert pill + kartu di detail) | `cc.rs` (server hook) + `agents.js` | Berfungsi, teruji end-to-end (allow & deny, native click) |
| Sesi baru / lanjutan headless (`claude -p --resume`) | `cc.rs::run` | Kode selesai; **belum diuji end-to-end** (butuh biner `claude` di PATH — lihat §6) |
| Stop / kill proses Claude Code | `cc.rs::kill` (taskkill /T /F) | Kode selesai, belum diuji langsung (hanya diverifikasi lewat unit test `win::kill_tree`) |
| Buka folder / VS Code / terminal dari detail sesi | `cc.rs::open` | Kode selesai, belum diuji |
| Pasang/copot hook ke `~/.claude/settings.json` | `cc.rs::hooks` | Kode selesai, **belum pernah dijalankan nyata** (sengaja, agar tidak mengubah settings.json pengguna tanpa izin eksplisit) |
| Tab Media (SMTC) | `media.js` | Dipindah dari v1, belum diuji ulang di v2 |
| Tab Sistem (kecerahan/volume/Wi-Fi/BT/monitor/autostart) | `system.js` | Dipindah dari v1 + monitor native baru; terlihat benar di screenshot (§6), belum diuji interaksi penuh |
| Tab Hari ini (Kalender/Tugas/Catatan/Fokus/Clipboard) | `today.js` | Dipindah dari v1, belum diuji ulang di v2 |
| Agent opencode, antigravity | — | **Dihapus** dari v2 atas permintaan eksplisit pengguna |

## 6. Yang sudah terverifikasi nyata (via WebView2 CDP + build asli, bukan asumsi)
- `cargo build` & `cargo test` bersih (0 warning, 8/8 test lulus) setelah seluruh revisi v2.
- Server hook: `GET /ping`, auth token 401 saat salah, `PreToolUse` non-blocking, `PermissionRequest` blocking dgn `peer_closed()` membersihkan antrAN saat koneksi terputus (disimulasikan dgn curl background yang dimatikan paksa).
- Alur izin penuh: trigger → notch otomatis membesar ke mode alert → klik native "Izinkan"/"Tolak" → `cc_decide` mengirim JSON persis sesuai skema resmi (`{"hookSpecificOutput":{"decision":{"behavior":"allow"}}}` / `"deny"` dgn `message`) → notch kembali mengecil.
- Tab Sistem menampilkan data nyata (CPU/RAM/Wi-Fi/Bluetooth sungguhan dari mesin uji).
- Tab Agen menampilkan **sesi Claude Code sungguhan yang sedang berjalan** (termasuk sesi penulisan handoff ini sendiri) dengan token, durasi, timeline tool-call live yang benar — murni dari transcript watcher, tanpa hook terpasang.
- `settings.json` asli pengguna (`~/.claude/settings.json`) dikonfirmasi **tidak pernah diubah** selama semua pengujian di atas (hook server diuji lewat HTTP langsung, bukan lewat instalasi nyata).

## 7. Belum terverifikasi / langkah berikutnya (urutan disarankan)
1. **Minta pengguna memasang hook** lewat tombol di notch (tab Agen → ikon gear → "Pasang / perbarui hook"), lalu uji 1 permintaan izin sungguhan dari sesi Claude Code nyata (bukan simulasi curl) untuk memastikan Claude Code CLI benar-benar mengirim `PermissionRequest` sesuai asumsi (field, format event — sudah dicocokkan dgn dokumentasi resmi tapi belum diuji dgn CLI asli mengirim event).
2. Uji `cc_run` (sesi baru & lanjutan headless) dengan biner `claude` CLI asli — pastikan resolusi path biner (`claude_bin()`) menemukan instalasi pengguna.
3. Uji Stop (kill proses) pada sesi nyata yang sedang berjalan, dan tombol buka folder/VS Code/terminal.
4. Uji ulang tab Media/Hari ini di v2 (fungsinya dipindah apa adanya dari v1 yang sudah teruji, tapi belum di-smoke-test ulang setelah pemindahan modul).
5. Uji DPI >100% dan multi-monitor (belum diuji; mesin uji ini 100%/1920×1080).
6. Pertimbangkan apakah hook sebaiknya level-project (`.claude/settings.json`) selain level-user, agar pengguna bisa memilih project mana yang termonitor.

## 8. Melanjutkan di akun lain
Unggah folder proyek (atau zip), lalu tempel prompt ini:
```
Lanjutkan proyek Windows Notch di folder ini. Baca README.md dan docs/HANDOFF.md dulu (revisi v2 — UI lama sudah diganti total, baca §2–4 soal keputusan desain dan bug CSS pointer-events yang sudah diperbaiki). Kerjakan "Langkah berikutnya" bagian 7 secara berurutan, mulai dari memasang hook Claude Code via UI. Aku di Windows, jadi minta aku menjalankan build/app dan kirim error-nya; jangan klaim sesuatu sudah berfungsi sebelum diuji nyata (gunakan WebView2 remote debugging / CDP seperti di §6 kalau perlu verifikasi UI). Setelah tiap langkah, perbarui bagian 5 dan 7 di docs/HANDOFF.md.
```
