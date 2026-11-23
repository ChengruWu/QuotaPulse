# Contributing

Thanks for helping improve **QuotaPulse** (Windows).

## Before Opening Issues

Do not paste tokens, account IDs, raw backend responses, `state.vscdb` contents, local auth paths, or screenshots containing personal data.

## Development (Windows)

```powershell
npm install
npm test
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
```

Use `npm run tauri dev` on Windows for desktop testing. Browser preview (`npm run dev`) uses mock dual-service data and cannot verify real Codex or Antigravity quota reads.

## Pull Requests

- Keep changes focused and Windows-native (do not reintroduce macOS/iOS/Android build targets or branches).
- Preserve the privacy and security boundaries documented in `PRIVACY.md` and `SECURITY.md`.
- Do not add telemetry, hardcoded OAuth client secrets, or raw response logging.
- Add or update Vitest and Rust unit tests when changing quota parsing, snapshot merging, tray icon rendering, or window geometry.
