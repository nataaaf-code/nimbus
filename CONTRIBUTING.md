# Contributing to Nimbus

Thanks for taking a look. Nimbus is an early-stage Windows app, so the most useful contributions right now are bug reports with reproduction steps, provider fixes, and small UI improvements.

## Before you start

- **Nimbus is Windows-only by design.** It depends on a transparent always-on-top Tauri window, WASAPI loopback capture, and `Windows.Media.Control`.
- For anything larger than a bug fix, open an issue first so we can agree on the approach before you spend a weekend on it.
- Never commit API keys, session transcripts, or `%APPDATA%\Nimbus` contents. See [SECURITY.md](SECURITY.md).

## Setup

You need Windows 10/11, [Rust](https://rustup.rs/) with the MSVC toolchain, [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/) with the C++ workload, Node.js 18+, and WebView2.

```bash
npm install
npm run dev
```

`npm run dev` rebuilds Rust and reloads the UI on change. The UI is plain ES modules loaded straight from `ui/`, so you can also open `ui/index.html` in a browser to work on layout — system calls will show a "Run as Windows app" placeholder instead of failing.

## Build and test

```bash
cargo test  --manifest-path src-tauri/Cargo.toml
cargo build --manifest-path src-tauri/Cargo.toml
npm run build   # NSIS installer in src-tauri/target/release/bundle
```

CI runs the first two on `windows-latest` for every pull request. A PR that does not compile or breaks a test will not be merged.

Four smoke tests in `sys.rs` touch real hardware (audio, display, Wi-Fi, Bluetooth) and are `#[ignore]`d so CI stays green on a runner with no devices. Run them on your own machine with:

```bash
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored
```

## Where things live

| Path | What it does |
|---|---|
| `src-tauri/src/main.rs` | Tauri setup, commands, window and input loop |
| `src-tauri/src/agents.rs` | Provider detection, install/login, headless runs, session state |
| `src-tauri/src/cc.rs` | Claude Code transcript reader, hook installer, local HTTP hook |
| `src-tauri/src/spectrum.rs` | WASAPI loopback → Goertzel FFT → events |
| `src-tauri/src/sys.rs` | Volume, brightness, Wi-Fi, Bluetooth |
| `src-tauri/src/win.rs` | Hit-testing, browser detection, window helpers |
| `ui/js/core.js` | Notch state machine and event bus |
| `ui/js/agents.js` | Agent hub, detail view, provider setup UI |
| `ui/css/app.css` | All styles |

There is no framework and no bundler. Keep it that way unless there is a concrete reason — a new runtime dependency is a discussion, not a detail.

## Adding an agent provider

Adding a provider is still a handful of places, not one plugin file. This is the known weak spot and a welcome refactor:

1. `PROVIDER_DEFS` in `src-tauri/src/agents.rs` — id, display name, install command, fork/device-login flags.
2. `provider_bin` — binary names and search directories.
3. `provider_key_env` — env var that carries its API key.
4. `default_models` — model list shown when the CLI cannot report its own.
5. `check_provider_auth` — how to read version and login state.
6. `PROV_LBL` in `ui/js/agents.js` and a `.ptag.p-<id>` rule in `ui/css/app.css` for the session tag color.

If a provider needs a new kind of auth flow, add it behind the existing commands (`ag_install_provider`, `ag_login_oauth`, `ag_save_key`) instead of adding a parallel path.

## Code style

- `cargo fmt` before pushing. Comments explain *why*; the code already says *what*.
- UI text and comments are Indonesian today. Match the surrounding language; English is fine in new files if that reads better.
- Keep changes focused. Reformatting a file you did not otherwise touch makes review harder.
- Conventional commit prefixes: `feat:`, `fix:`, `docs:`, `refactor:`, `chore:`.

## Pull requests

- One concern per PR. Small PRs get reviewed and merged faster.
- Include a screenshot or short clip for UI changes.
- Say what you tested and on which Windows version.
- Update the README if you add a feature or change setup steps.

## Scope

In scope: new agent providers, session/timeline accuracy, notch and line-mode behaviour, performance, Windows integrations, accessibility, docs.

Ask first: cross-platform support, replacing the vanilla JS UI with a framework, large architectural rewrites, telemetry or any network backend.

By participating you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
