# Privacy

QuotaPulse is designed to be Windows-local, read-only, and minimal.

## What It Reads

- **Codex Desktop login state**: Reads `%CODEX_HOME%\auth.json` or `%USERPROFILE%\.codex\auth.json` (max 256 KB) solely to query Codex quota. The account identifier may be read from the auth file or JWT payload only to populate the required request header.
- **Antigravity quota state**: Prefers invoking the local official Antigravity CLI (`%USERPROFILE%\.gemini\bin\agy.exe` or `PATH` / `ANTIGRAVITY_CLI_PATH`) with `--log-file %TEMP%\quotapulse-agy-poll.log --output-format json --print-timeout 15s -p "/usage"` and `AGY_CLI_DISABLE_AUTO_UPDATE=true` (zero model turns, single overwritten poll log file, without reading any OS keyring or OAuth token directly). If `agy.exe` is unavailable, falls back to opening local `%APPDATA%\Antigravity\User\globalStorage\state.vscdb` (or `Antigravity IDE` / `%USERPROFILE%\.gemini\oauth_creds.json` / `ANTIGRAVITY_AUTH_PATH`) strictly in read-only mode (`SQLITE_OPEN_READ_ONLY`) solely to extract the active Google OAuth access token for quota queries.

## What It Stores

QuotaPulse stores only UI preferences in `%APPDATA%\app.quotafloat.desktop\preferences.json` (with a `.json.bak` atomic backup):

- locked (click-through) state
- always-on-top state
- language (`zh-CN` or `en`)
- selected tray quota source (`codex`, `codex_weekly`, `gemini`, `gemini_weekly`, `antigravity_3p`, `antigravity_3p_weekly`, or `gemini_argon_weekly`)
- service visibility toggles (`showCodex` and `showAntigravity`, at least one always enabled)
- legacy rotation/pin fields for backward compatibility with Quota Float v0.1.x

It never copies or persists Codex tokens, Google OAuth tokens, refresh tokens, account IDs, raw quota responses, prompts, chat history, or local auth paths.

## What It Sends

The app only calls these quota-related HTTPS endpoints directly from the local Windows desktop process (plus GitHub Releases when checking for signed app updates):

- `https://chatgpt.com/backend-api/wham/usage`
- `https://chatgpt.com/backend-api/wham/rate-limit-reset-credits`
- `https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist`
- `https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary`
- `https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist` (fallback)
- `https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary` (fallback)

No telemetry, analytics, crash reporting, or third-party tracking is included.

## Logging

Logs are intentionally generic. They never include tokens, account IDs, raw backend responses, request headers, local auth paths, or personal file paths.

## Accuracy Boundary

QuotaPulse displays quota windows returned directly by the Codex and Antigravity quota services. It does not estimate quota from local token usage and never fabricates percentages when a response is unavailable or changes shape.
