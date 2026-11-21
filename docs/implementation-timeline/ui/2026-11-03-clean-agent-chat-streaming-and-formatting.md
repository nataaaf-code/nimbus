# 2026-11-03 Pembersihan Output Chat AI Agent dan Formatting Markdown

## Konteks
Sebelumnya, output dari `opencode run` menyisipkan ANSI escape sequence seperti `␛[0m` (`\u241b` / `\u001b`), progress bar CLI (`> build · deepseek-v4.1-flash-free`), dan skrip eksekusi tool JavaScript mentah (`execute {"code": ...}`) langsung ke dalam bubble chat. Selain itu, formatting markdown teks dan tabel tampak berantakan karena parser inline memecah tag dengan `<br/>` yang berlebihan dan tinggi container chat terlalu sempit.

## Perubahan

1. **Backend Rust (`src-tauri/src/main.rs`)**:
   - Menambahkan flag `--format json` pada pemanggilan `opencode run --auto --format json "<prompt>"`.
   - Mengimplementasikan `clean_ansi()` untuk membersihkan ASCII escape (`\x1b`), Unicode escape symbol (`\u{241b}` / `␛`), dan C1 control sequence (`\u{009b}`).
   - Mem-parse baris output JSON streaming dari `opencode`:
     - Tipe `"text"`: dipancarkan ke frontend via event `agent-text` (hanya teks bersih respon AI).
     - Tipe `"tool_use"`: dipancarkan via `agent-status` (misal `⚙ Menjalankan execute`) untuk memperbarui status indicator tanpa mengotori bubble percakapan.
     - Tipe `"step_start"`: dipancarkan via `agent-status` (`Sedang berpikir...`).
     - Tipe `"error"`: dipancarkan via `agent-error`.
   - Menambahkan unit test `tests::test_clean_ansi`.

2. **Frontend UI (`ui/index.html`)**:
   - Memperbarui `stripAnsi()` untuk membersihkan karakter `␛` (`\u241b`), `\u001b`, dan `\u009b`.
   - Memperbaiki `renderMarkdown()` agar menangani paragraf (`<p>`), blok tabel (`<table>`), daftar (`<ul>`), header (`<h2>`-`<h4>`), dan blok kode (`<pre><code>`) secara terpisah tanpa merusak struktur HTML dengan `<br/>` acak.
   - Menata ulang layout tab AI Agent (`.p.a`) menggunakan flexbox (`flex: 1; min-height: 160px; max-height: 195px;`) dengan custom scrollbar halus sehingga nyaman dibaca dan di-scroll.
   - Memisahkan status sementara (di chip `#st`, judul `#tk`, dan bar `#ab`) dari histori chat (`#lg`), sehingga log tool dan build progress tidak menumpuk di area obrolan.

## Verifikasi
- Unit test `cargo test --bin nimbus` (sebelumnya `windows-notch`) berhasil (`test_clean_ansi`, `test_brightness_and_volume`, `test_radios`).
- Format JSON dari `opencode` divalidasi tidak memancarkan ANSI escapes atau stderr build noise ke obrolan pengguna.
