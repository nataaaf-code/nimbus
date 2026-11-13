# Dua Agent di Tab AI: OpenCode + Antigravity

**Tanggal**: 2026-11-04  
**Domain**: ui / sys  
**Status**: Terverifikasi (cargo check + CLI stream-json + syntax check UI); belum dijalankan di jendela Tauri

## Latar Belakang
Tab AI Agent hanya bisa mengirim perintah ke OpenCode. Pengguna ingin mengontrol dua agent dari notch yang sama. Antigravity CLI (`agy`) sudah terpasang dan menyediakan mode headless dengan NDJSON, sehingga bisa dijadikan backend kedua.

## Perubahan

### Backend (`src-tauri/src/main.rs`)
- `agent_send` sekarang menerima `agent: "opencode" | "antigravity"` (default opencode).
- Menambahkan `antigravity_command()`: urutan deteksi `NOTCH_ANTIGRAVITY_BIN` → `%LOCALAPPDATA%\agy\bin\agy.exe` → `agy.exe` di PATH.
- Menambahkan `agent_send_antigravity()`: menjalankan `agy -p "<prompt>" --output-format stream-json`, membaca NDJSON baris demi baris, memetakan `step_update.agent_response.text_delta` ke teks chat, `step_update.tool` ke status tool, dan `result` ke usage/error/done.
- `conversation_id` dari event `result` disimpan di `AGY_CONVERSATION`; perintah berikutnya memakai `--conversation` agar percakapan antigravity berlanjut. Command baru `agent_reset` menghapus sesi tersebut.
- Semua event agent (`agent-text`, `agent-status`, `agent-error`, `agent-done`) kini berpayload objek `{ agent, ... }` supaya dua proses yang berjalan bersamaan tidak tertukar.
- `NOTCH_ANTIGRAVITY_CWD` (fallback `NOTCH_AGENT_CWD`) dan `hide_console` tetap dipakai.

### UI (`ui/index.html`)
- Chip `opencode` dan `antigravity` menjadi tombol pemilih agent. Pilihan terakhir disimpan di localStorage (`notch.agent`).
- Setiap agent punya state terpisah: pesan, status, progress, dan task. Agent yang tidak sedang dilihat tetap menerima update di belakang.
- Panel AI menampilkan usage sesuai agent aktif: OpenCode memakai langkah/perintah/tool hari ini; Antigravity memakai token masuk/keluar/reasoning/turn dari event `result`.
- Chip usage diberi `visibility: visible` eksplisit saat ditampilkan dan `white-space: nowrap` agar tidak menyusut/terlipat di baris chip.
- Tombol `↺` memulai sesi baru: membersihkan percakapan di UI dan (untuk antigravity) memanggil `agent_reset` agar tidak memakai `--conversation` lama.
- Catatan cepat “Kirim ke agent” mengirim ke agent yang sedang dipilih.

## Verifikasi
- `cargo check --manifest-path src-tauri/Cargo.toml` berhasil.
- `agy -p "Reply with exactly: ok" --output-format stream-json` mengembalikan `init`, `step_update` (`user_input`, `agent_response` dengan `text_delta`, `result` dengan `usage`), sesuai parser.
- Ekstraksi `<script>` dari `ui/index.html` lolos `node --check`.
- Layout dicek dengan Edge headless pada 560x400 untuk kedua agent: chip, baris statistik 4 kolom, log, dan input tidak overflow.
- Belum dijalankan di jendela Tauri: perlu uji kirim perintah ke kedua agent, pergantian chip saat satu agent berjalan, dan lanjutan percakapan antigravity.
