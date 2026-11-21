# Security Policy

## Reporting a vulnerability

Please do not open a public issue for security problems. Use GitHub's private reporting:

**https://github.com/nataaaf-code/nimbus/security/advisories/new**

Include the version, Windows build, reproduction steps, and what an attacker could do. You can expect an acknowledgement within a few days and credit in the advisory once a fix ships, unless you prefer to stay anonymous.

## Supported versions

Only the latest release gets fixes. There are no long-term branches.

## What Nimbus can access

Nimbus has no backend and no telemetry. Everything below happens locally.

| Data | Where | Notes |
|---|---|---|
| Provider API keys | `%APPDATA%\Nimbus\provider_keys.json` | **Plain JSON** — readable by any process running as your user |
| Google Calendar URL | `%APPDATA%\Nimbus\config.json` | Secret iCal address (`ics_url`) for fetching events |
| Claude Code transcripts | `~/.claude/projects/` (read) | Sessions, prompts, tool calls, diffs |
| Permission hook | `127.0.0.1` loopback only | Registered in `~/.claude/settings.json`; lets the notch allow/deny tool calls |
| Audio | WASAPI loopback | Captured in memory for the visualizer; not written to disk |

Nimbus also spawns provider CLIs (`claude`, `opencode`, `agy`, `codex`, `gemini`) as child processes with your user privileges. Treat any installed CLI as part of your trust boundary.

## Known limitations

- **API keys are stored unencrypted.** Prefer the CLI login flow when possible. Moving the key store to Windows DPAPI is planned; until then, anything that can read your `%APPDATA%` can read the keys.
- **Builds are not code-signed.** SmartScreen will warn on first run. Verify the SHA-256 published with each release.
- **The permission gate answers for you.** If you allow a tool call from the notch, the agent runs it. Review the command before approving.

## Hardening your setup

- Use per-provider keys with limited scope and rotate them if you ever shared a log or screenshot.
- Keep `%APPDATA%\Nimbus` out of cloud-synced folders.
- Uninstall the Claude Code hook from the Agents tab if you stop using the permission gate.
