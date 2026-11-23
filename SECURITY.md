# Security

## Supported Use

QuotaPulse is a Windows-only local desktop utility that reads Codex (GPT) and Antigravity (Gemini, Claude & GPT, Gemini 4 Argon) quotas using your existing local login states.

## Reporting Issues

Please do not open public issues containing tokens, account IDs, `state.vscdb` extracts, raw backend responses, screenshots with personal data, or local user paths. Redact sensitive information before sharing logs or bug reports.

## Security Boundaries

- Never persists Codex or Antigravity credentials on disk or in logs.
- Never embeds third-party OAuth client secrets in source code or release binaries.
- Opens Antigravity's `state.vscdb` strictly with `SQLITE_OPEN_READ_ONLY` and a short busy timeout; never modifies external databases or auth files.
- Marks `Authorization` and `ChatGPT-Account-Id` headers as sensitive in `reqwest`.
- Caps JSON auth file reads at 256 KB and HTTP quota responses at 1 MB.
- Disables HTTP redirects (`redirect::Policy::none()`) on all quota requests.
- Never redeems reset credits or modifies account settings.
- Uses a gentler 60-second background cadence for Antigravity quota polling to avoid aggressive request loops.
