use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::{Duration, SystemTime},
};

use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
    Engine,
};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use serde_json::Value;

use crate::models::{ProviderSnapshot, QuotaPool, UsageWindow};

const DAILY_BASE: &str = "https://daily-cloudcode-pa.googleapis.com";
const PROD_BASE: &str = "https://cloudcode-pa.googleapis.com";
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;
const MAX_AUTH_BYTES: u64 = 256 * 1024;
const MAX_DECODE_DEPTH: usize = 12;
const CLI_PRINT_TIMEOUT: &str = "15s";
const CLI_HARD_TIMEOUT: Duration = Duration::from_secs(20);
const CLI_AUTH_CHECK_TIMEOUT: Duration = Duration::from_secs(10);
const CLI_LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const CLI_POLL_LOG_FILENAME: &str = "quotapulse-agy-poll.log";
const CLI_DISABLE_AUTO_UPDATE_ENV: &str = "AGY_CLI_DISABLE_AUTO_UPDATE";
const CLI_AUTH_ACTION_ENV: &str = "AGY_CLI_CDE_AUTH_ACTION";
const CLI_NONINTERACTIVE_HEADLESS_ENV: &str = "AGY_CLI_NONINTERACTIVE_HEADLESS";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const STATE_DB_KEYS: &[&str] = &[
    "antigravityUnifiedStateSync.oauthToken",
    "jetskiStateSync.agentManagerInitState",
    "antigravityAuthStatus",
];

#[derive(Default, Clone)]
pub struct SessionCache {
    token_fingerprint: Option<u64>,
    project: Option<String>,
    plan: Option<String>,
}

struct Auth {
    access_token: String,
    fingerprint: u64,
}

fn fingerprint_token(token: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    token.hash(&mut hasher);
    hasher.finish()
}

fn is_valid_google_access_token(candidate: &str) -> bool {
    let trimmed = candidate.trim();
    trimmed.starts_with("ya29.")
        && trimmed.len() >= 20
        && trimmed.len() <= 2048
        && trimmed
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !byte.is_ascii_whitespace())
}

fn decode_base64_any(input: &str) -> Option<Vec<u8>> {
    let cleaned: String = input.chars().filter(|ch| !ch.is_whitespace()).collect();
    if cleaned.len() < 8 {
        return None;
    }
    STANDARD
        .decode(&cleaned)
        .or_else(|_| STANDARD_NO_PAD.decode(&cleaned))
        .or_else(|_| URL_SAFE.decode(&cleaned))
        .or_else(|_| URL_SAFE_NO_PAD.decode(&cleaned))
        .ok()
}

fn read_varint(bytes: &[u8], offset: &mut usize) -> Option<u64> {
    let mut result = 0u64;
    let mut shift = 0u32;
    while *offset < bytes.len() && shift < 64 {
        let byte = bytes[*offset];
        *offset += 1;
        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some(result);
        }
        shift += 7;
    }
    None
}

fn extract_protobuf_length_delimited(bytes: &[u8]) -> Vec<&[u8]> {
    let mut chunks = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let Some(tag) = read_varint(bytes, &mut offset) else {
            break;
        };
        let field_number = tag >> 3;
        let wire_type = tag & 0x07;
        if field_number == 0 {
            break;
        }
        match wire_type {
            0 => {
                if read_varint(bytes, &mut offset).is_none() {
                    break;
                }
            }
            1 => {
                if offset + 8 > bytes.len() {
                    break;
                }
                offset += 8;
            }
            2 => {
                let Some(len) = read_varint(bytes, &mut offset) else {
                    break;
                };
                let len = len as usize;
                if offset.saturating_add(len) > bytes.len() {
                    break;
                }
                chunks.push(&bytes[offset..offset + len]);
                offset += len;
            }
            5 => {
                if offset + 4 > bytes.len() {
                    break;
                }
                offset += 4;
            }
            _ => break,
        }
    }
    chunks
}

fn extract_token_from_json(value: &Value, depth: usize) -> Option<String> {
    if depth > MAX_DECODE_DEPTH {
        return None;
    }
    match value {
        Value::String(text) => extract_token_from_str(text, depth + 1),
        Value::Object(map) => {
            for key in ["access_token", "accessToken", "oauthToken", "token"] {
                if let Some(child) = map.get(key) {
                    if let Some(found) = extract_token_from_json(child, depth + 1) {
                        return Some(found);
                    }
                }
            }
            for child in map.values() {
                if let Some(found) = extract_token_from_json(child, depth + 1) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| extract_token_from_json(item, depth + 1)),
        _ => None,
    }
}

fn find_embedded_google_access_token(bytes: &[u8]) -> Option<String> {
    const PREFIX: &[u8] = b"ya29.";
    let mut start = 0usize;
    while start + PREFIX.len() < bytes.len() {
        let Some(pos) = bytes[start..]
            .windows(PREFIX.len())
            .position(|window| window == PREFIX)
        else {
            break;
        };
        let token_start = start + pos;
        let mut token_end = token_start + PREFIX.len();
        while token_end < bytes.len() && token_end - token_start < 2048 {
            let b = bytes[token_end];
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
                token_end += 1;
            } else {
                break;
            }
        }
        if token_end - token_start >= 32 {
            if let Ok(candidate) = std::str::from_utf8(&bytes[token_start..token_end]) {
                if is_valid_google_access_token(candidate) {
                    return Some(candidate.to_owned());
                }
            }
        }
        start = token_end.max(token_start + 1);
    }
    None
}

fn extract_token_from_bytes(bytes: &[u8], depth: usize) -> Option<String> {
    if depth > MAX_DECODE_DEPTH || bytes.is_empty() {
        return None;
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        if is_valid_google_access_token(text) {
            return Some(text.trim().to_owned());
        }
        if let Some(token) = extract_token_from_str(text, depth) {
            return Some(token);
        }
    }
    for chunk in extract_protobuf_length_delimited(bytes) {
        if let Some(token) = extract_token_from_bytes(chunk, depth + 1) {
            return Some(token);
        }
    }
    find_embedded_google_access_token(bytes)
}

fn extract_token_from_str(raw: &str, depth: usize) -> Option<String> {
    if depth > MAX_DECODE_DEPTH {
        return None;
    }
    let trimmed = raw.trim();
    if is_valid_google_access_token(trimmed) {
        return Some(trimmed.to_owned());
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') || trimmed.starts_with('"') {
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            if let Some(token) = extract_token_from_json(&value, depth + 1) {
                return Some(token);
            }
        }
    }
    if let Some(decoded) = decode_base64_any(trimmed) {
        if let Some(token) = extract_token_from_bytes(&decoded, depth + 1) {
            return Some(token);
        }
    }
    None
}

enum DbReadOutcome {
    Found(String),
    NoToken,
    Unreadable,
}

fn open_vscdb_readonly(path: &Path) -> Option<Connection> {
    if let Ok(conn) = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        let _ = conn.busy_timeout(Duration::from_millis(500));
        return Some(conn);
    }
    let normalized = path.to_string_lossy().replace('\\', "/");
    let uri = format!("file:///{}?mode=ro&immutable=1", normalized.trim_start_matches('/'));
    Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()
}

fn token_from_value_ref(value: ValueRef<'_>) -> Option<String> {
    match value {
        ValueRef::Text(bytes) => {
            let text = std::str::from_utf8(bytes).ok()?;
            extract_token_from_str(text, 0)
        }
        ValueRef::Blob(bytes) => extract_token_from_bytes(bytes, 0),
        _ => None,
    }
}

fn load_token_from_vscdb(path: &Path) -> DbReadOutcome {
    let Some(conn) = open_vscdb_readonly(path) else {
        return DbReadOutcome::Unreadable;
    };
    if let Ok(mut stmt) = conn.prepare("SELECT value FROM ItemTable WHERE key = ?1") {
        for key in STATE_DB_KEYS {
            if let Ok(Some(token)) = stmt.query_row([key], |row| {
                Ok(row.get_ref(0).ok().and_then(token_from_value_ref))
            }) {
                return DbReadOutcome::Found(token);
            }
        }
    } else {
        return DbReadOutcome::Unreadable;
    }

    // Fallback: inspect other auth/oauth rows in ItemTable (read-only, capped).
    if let Ok(mut stmt) = conn.prepare(
        "SELECT value FROM ItemTable WHERE lower(key) LIKE '%oauth%' OR lower(key) LIKE '%antigravity%auth%' LIMIT 32",
    ) {
        if let Ok(mut rows) = stmt.query([]) {
            while let Ok(Some(row)) = rows.next() {
                if let Some(token) = row.get_ref(0).ok().and_then(token_from_value_ref) {
                    return DbReadOutcome::Found(token);
                }
            }
        }
    }

    DbReadOutcome::NoToken
}

fn load_token_from_json_file(path: &Path) -> DbReadOutcome {
    let Ok(metadata) = fs::metadata(path) else {
        return DbReadOutcome::Unreadable;
    };
    if !metadata.is_file() || metadata.len() > MAX_AUTH_BYTES {
        return DbReadOutcome::Unreadable;
    }
    let Ok(raw) = fs::read_to_string(path) else {
        return DbReadOutcome::Unreadable;
    };
    match extract_token_from_str(&raw, 0) {
        Some(token) => DbReadOutcome::Found(token),
        None => DbReadOutcome::NoToken,
    }
}

fn latest_modified(path: &Path) -> SystemTime {
    let main_time = fs::metadata(path)
        .and_then(|meta| meta.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut wal_name = path.as_os_str().to_os_string();
    wal_name.push("-wal");
    let wal_time = fs::metadata(PathBuf::from(wal_name))
        .and_then(|meta| meta.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    main_time.max(wal_time)
}

fn candidate_auth_paths() -> Vec<PathBuf> {
    if let Some(explicit) = std::env::var_os("ANTIGRAVITY_AUTH_PATH") {
        if !explicit.is_empty() {
            return vec![PathBuf::from(explicit)];
        }
    }
    let mut paths = Vec::new();
    if let Some(config_dir) = dirs::config_dir() {
        for folder in ["Antigravity IDE", "Antigravity", "Antigravity-IDE"] {
            paths.push(
                config_dir
                    .join(folder)
                    .join("User")
                    .join("globalStorage")
                    .join("state.vscdb"),
            );
        }
    }
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".gemini").join("antigravity-cli").join("oauth_creds.json"));
        paths.push(home.join(".gemini").join("antigravity-cli").join("credentials.json"));
        paths.push(home.join(".gemini").join("oauth_creds.json"));
        paths.push(home.join(".gemini").join("jetski-standalone-oauth-token"));
    }
    // Sort existing paths by freshest modification time (including SQLite -wal).
    let mut indexed: Vec<(usize, SystemTime, PathBuf)> = paths
        .into_iter()
        .enumerate()
        .map(|(index, path)| {
            let modified = if path.exists() {
                latest_modified(&path)
            } else {
                SystemTime::UNIX_EPOCH
            };
            (index, modified, path)
        })
        .collect();
    indexed.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    indexed.into_iter().map(|(_, _, path)| path).collect()
}

fn load_auth_candidates() -> Result<Vec<Auth>, (&'static str, &'static str)> {
    let mut saw_existing_file = false;
    let mut saw_unreadable = false;
    let mut candidates: Vec<Auth> = Vec::new();
    for path in candidate_auth_paths() {
        if !path.exists() {
            continue;
        }
        saw_existing_file = true;
        let is_sqlite = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "vscdb" | "db" | "sqlite"));
        let outcome = if is_sqlite {
            load_token_from_vscdb(&path)
        } else {
            load_token_from_json_file(&path)
        };
        match outcome {
            DbReadOutcome::Found(access_token) => {
                let fingerprint = fingerprint_token(&access_token);
                if !candidates.iter().any(|item| item.fingerprint == fingerprint) {
                    candidates.push(Auth {
                        access_token,
                        fingerprint,
                    });
                }
            }
            DbReadOutcome::Unreadable => saw_unreadable = true,
            DbReadOutcome::NoToken => {}
        }
    }
    if !candidates.is_empty() {
        return Ok(candidates);
    }
    if !saw_existing_file {
        Err((
            "signed_out",
            "No local Antigravity login file found. Open Antigravity and sign in.",
        ))
    } else if saw_unreadable {
        Err((
            "unavailable",
            "Unable to read local Antigravity state file (in use). Retry shortly.",
        ))
    } else {
        Err((
            "signed_out",
            "No active Antigravity session token found. Open Antigravity to sign in.",
        ))
    }
}

fn headers(auth: &Auth) -> Result<HeaderMap, &'static str> {
    let mut result = HeaderMap::new();
    let mut bearer = HeaderValue::from_str(&format!("Bearer {}", auth.access_token))
        .map_err(|_| "Antigravity login data is invalid.")?;
    bearer.set_sensitive(true);
    result.insert(AUTHORIZATION, bearer);
    result.insert(ACCEPT, HeaderValue::from_static("application/json"));
    result.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    Ok(result)
}

async fn limited_json(mut response: reqwest::Response) -> Result<Value, ()> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES)
    {
        return Err(());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
        if bytes.len().saturating_add(chunk.len()) as u64 > MAX_RESPONSE_BYTES {
            return Err(());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| ())
}

fn safe_http_failure(status: reqwest::StatusCode) -> (&'static str, &'static str) {
    match status.as_u16() {
        401 => (
            "signed_out",
            "Antigravity session expired (401). Open Antigravity to refresh login.",
        ),
        403 => (
            "unavailable",
            "Antigravity quota endpoint denied access (403).",
        ),
        429 => (
            "unavailable",
            "Antigravity quota service is rate limited. It will retry automatically.",
        ),
        _ => (
            "unavailable",
            "Antigravity quota service is temporarily unavailable.",
        ),
    }
}

fn pick_str<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn extract_project_and_plan(value: &Value) -> (Option<String>, Option<String>) {
    let project = value
        .get("cloudaicompanionProject")
        .or_else(|| value.get("cloudaicompanion_project"))
        .and_then(|item| {
            item.as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    pick_str(item, &["id", "projectId", "project_id", "name"]).map(str::to_owned)
                })
        });
    let raw_plan = value
        .get("currentTier")
        .or_else(|| value.get("current_tier"))
        .and_then(|tier| pick_str(tier, &["id", "name"]))
        .or_else(|| pick_str(value, &["tier", "plan", "planType", "plan_type"]));
    let plan = raw_plan.map(normalize_plan_badge);
    (project, plan)
}

fn normalize_plan_badge(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("ultra") {
        "ULTRA".into()
    } else if lower.contains("pro") {
        "PRO".into()
    } else if lower.contains("enterprise") {
        "ENT".into()
    } else if lower.contains("standard") {
        "STD".into()
    } else if lower.contains("free") {
        "FREE".into()
    } else {
        raw.chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .take(10)
            .collect::<String>()
            .to_uppercase()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowKind {
    Short,
    Weekly,
}

fn classify_bucket_window(value: &Value) -> Option<WindowKind> {
    let duration = value
        .get("windowSeconds")
        .or_else(|| value.get("window_seconds"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if duration > 0 {
        if duration.abs_diff(18_000) <= 120 {
            return Some(WindowKind::Short);
        }
        if duration.abs_diff(604_800) <= 3600 {
            return Some(WindowKind::Weekly);
        }
    }
    let hint = [
        pick_str(value, &["window", "windowType", "window_type"]),
        pick_str(value, &["bucketId", "bucket_id", "id"]),
        pick_str(value, &["displayName", "display_name", "name"]),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase();

    if hint.contains("5h")
        || hint.contains("5_hour")
        || hint.contains("five_hour")
        || hint.contains("five hour")
        || hint.contains("short")
    {
        Some(WindowKind::Short)
    } else if hint.contains("week") || hint.contains("7d") {
        Some(WindowKind::Weekly)
    } else {
        None
    }
}

fn parse_bucket_window(value: &Value, kind: WindowKind) -> Option<UsageWindow> {
    let remaining_percent = if let Some(pct) = value
        .get("remainingPercent")
        .or_else(|| value.get("remaining_percent"))
        .and_then(Value::as_f64)
    {
        pct
    } else if let Some(frac) = value
        .get("remainingFraction")
        .or_else(|| value.get("remaining_fraction"))
        .and_then(Value::as_f64)
    {
        if frac <= 1.0 {
            frac * 100.0
        } else {
            frac
        }
    } else if let Some(used_pct) = value
        .get("usedPercent")
        .or_else(|| value.get("used_percent"))
        .and_then(Value::as_f64)
    {
        100.0 - used_pct
    } else if let Some(used_frac) = value
        .get("usedFraction")
        .or_else(|| value.get("used_fraction"))
        .and_then(Value::as_f64)
    {
        100.0 - if used_frac <= 1.0 { used_frac * 100.0 } else { used_frac }
    } else {
        // Protobuf3 JSON omits zero-valued float fields (`remainingFraction: 0.0`).
        // Accept omitted fraction as 0% only when the bucket has both identity and reset/window metadata.
        let has_identity = pick_str(value, &["bucketId", "bucket_id", "window", "windowType", "window_type"]).is_some();
        let has_reset = pick_str(value, &["resetTime", "reset_time", "resetsAt", "resets_at", "resetAt", "reset_at"]).is_some();
        if has_identity && has_reset {
            0.0
        } else {
            return None;
        }
    };

    let resets_at = pick_str(
        value,
        &[
            "resetTime",
            "reset_time",
            "resetsAt",
            "resets_at",
            "resetAt",
            "reset_at",
        ],
    )
    .map(str::to_owned);

    let window_seconds = value
        .get("windowSeconds")
        .or_else(|| value.get("window_seconds"))
        .and_then(Value::as_u64)
        .filter(|secs| *secs > 0)
        .unwrap_or(match kind {
            WindowKind::Short => 18_000,
            WindowKind::Weekly => 604_800,
        });

    Some(UsageWindow {
        remaining_percent: remaining_percent.clamp(0.0, 100.0),
        resets_at,
        window_seconds,
    })
}

#[derive(Debug, PartialEq)]
struct ParsedQuota {
    short_window: Option<UsageWindow>,
    weekly_window: Option<UsageWindow>,
    extra_pools: Vec<QuotaPool>,
}

fn classify_group_id(display_name: &str, buckets: &[Value]) -> (String, String) {
    let mut combined = display_name.to_ascii_lowercase();
    for bucket in buckets {
        if let Some(id) = pick_str(bucket, &["bucketId", "bucket_id", "id"]) {
            combined.push(' ');
            combined.push_str(&id.to_ascii_lowercase());
        }
    }
    let is_primary_gemini_group = display_name.to_ascii_lowercase().contains("gemini models")
        || buckets.iter().any(|bucket| {
            pick_str(bucket, &["bucketId", "bucket_id", "id"]).is_some_and(|id| {
                let lower = id.to_ascii_lowercase();
                lower.starts_with("gemini-") && !lower.contains("argon")
            })
        });
    if combined.contains("argon") && !is_primary_gemini_group {
        let label = if display_name.is_empty() {
            "Gemini 4 Argon".to_string()
        } else {
            display_name.to_string()
        };
        return ("gemini_argon".into(), label);
    }
    if combined.contains("3p") || combined.contains("claude") || combined.contains("gpt") {
        return ("antigravity_3p".into(), "Claude & GPT".into());
    }
    if combined.contains("gemini") {
        return ("gemini".into(), "Gemini Models".into());
    }
    let slug: String = display_name
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    let id = slug.trim_matches('_').to_string();
    (
        if id.is_empty() { "antigravity_extra".into() } else { id },
        if display_name.is_empty() { "Other Models".into() } else { display_name.into() },
    )
}

fn parse_group_buckets(buckets: &[Value]) -> (Option<UsageWindow>, Option<UsageWindow>) {
    let mut short_window = None;
    let mut weekly_window = None;
    for bucket in buckets {
        let Some(kind) = classify_bucket_window(bucket) else {
            continue;
        };
        let Some(window) = parse_bucket_window(bucket, kind) else {
            continue;
        };
        match kind {
            WindowKind::Short => {
                if short_window.is_none() {
                    short_window = Some(window);
                }
            }
            WindowKind::Weekly => {
                if weekly_window.is_none() {
                    weekly_window = Some(window);
                }
            }
        }
    }
    (short_window, weekly_window)
}

fn parse_quota_summary(value: &Value) -> Option<ParsedQuota> {
    let root = value
        .get("command")
        .and_then(|command| command.get("data"))
        .filter(|data| data.is_object())
        .or_else(|| value.get("response").filter(|resp| resp.is_object()))
        .unwrap_or(value);
    let mut gemini_short = None;
    let mut gemini_weekly = None;
    let mut extra_pools: Vec<QuotaPool> = Vec::new();

    if let Some(groups) = root.get("groups").and_then(Value::as_array) {
        for group in groups {
            let display_name = pick_str(group, &["displayName", "display_name", "name"]).unwrap_or("");
            let Some(buckets) = group.get("buckets").and_then(Value::as_array) else {
                continue;
            };
            let (short_window, weekly_window) = parse_group_buckets(buckets);
            if short_window.is_none() && weekly_window.is_none() {
                continue;
            }
            let (pool_id, label) = classify_group_id(display_name, buckets);
            if pool_id == "gemini" {
                gemini_short = gemini_short.or(short_window);
                gemini_weekly = gemini_weekly.or(weekly_window);
            } else if let Some(existing) = extra_pools.iter_mut().find(|pool| pool.id == pool_id) {
                existing.short_window = existing.short_window.clone().or(short_window);
                existing.weekly_window = existing.weekly_window.clone().or(weekly_window);
            } else {
                extra_pools.push(QuotaPool {
                    id: pool_id,
                    display_name: label,
                    short_window,
                    weekly_window,
                });
            }
        }
    } else if let Some(buckets) = root
        .get("quotaSummary")
        .or_else(|| root.get("quota_summary"))
        .and_then(|summary| summary.get("buckets"))
        .or_else(|| root.get("buckets"))
        .and_then(Value::as_array)
    {
        for bucket in buckets {
            let Some(kind) = classify_bucket_window(bucket) else {
                continue;
            };
            let Some(window) = parse_bucket_window(bucket, kind) else {
                continue;
            };
            let bucket_id = pick_str(bucket, &["bucketId", "bucket_id", "id"]).unwrap_or("");
            let (pool_id, label) = classify_group_id(bucket_id, std::slice::from_ref(bucket));
            if pool_id == "gemini" || (!bucket_id.contains("3p") && !bucket_id.contains("argon")) {
                match kind {
                    WindowKind::Short => gemini_short = gemini_short.or(Some(window)),
                    WindowKind::Weekly => gemini_weekly = gemini_weekly.or(Some(window)),
                }
            } else if let Some(existing) = extra_pools.iter_mut().find(|pool| pool.id == pool_id) {
                match kind {
                    WindowKind::Short => existing.short_window = existing.short_window.clone().or(Some(window)),
                    WindowKind::Weekly => existing.weekly_window = existing.weekly_window.clone().or(Some(window)),
                }
            } else {
                extra_pools.push(QuotaPool {
                    id: pool_id,
                    display_name: label,
                    short_window: matches!(kind, WindowKind::Short).then_some(window.clone()),
                    weekly_window: matches!(kind, WindowKind::Weekly).then_some(window),
                });
            }
        }
    }

    extra_pools.sort_by_key(|pool| match pool.id.as_str() {
        "antigravity_3p" => 0,
        "gemini_argon" => 1,
        _ => 2,
    });

    if gemini_short.is_none() && gemini_weekly.is_none() && extra_pools.is_empty() {
        return None;
    }

    Some(ParsedQuota {
        short_window: gemini_short,
        weekly_window: gemini_weekly,
        extra_pools,
    })
}

async fn resolve_session_metadata(
    client: &reqwest::Client,
    request_headers: &HeaderMap,
    auth: &Auth,
    cache: &Mutex<SessionCache>,
) -> (Option<String>, Option<String>) {
    if let Ok(guard) = cache.lock() {
        if guard.token_fingerprint == Some(auth.fingerprint) && guard.project.is_some() {
            return (guard.project.clone(), guard.plan.clone());
        }
    }
    let body = serde_json::json!({
        "metadata": {
            "ideType": "IDE_UNSPECIFIED",
            "platform": "WINDOWS",
            "pluginType": "GEMINI"
        }
    });
    for base in [DAILY_BASE, PROD_BASE] {
        let url = format!("{base}/v1internal:loadCodeAssist");
        let Ok(response) = client
            .post(&url)
            .headers(request_headers.clone())
            .json(&body)
            .send()
            .await
        else {
            continue;
        };
        if !response.status().is_success() {
            continue;
        }
        if let Ok(json) = limited_json(response).await {
            let (project, plan) = extract_project_and_plan(&json);
            if project.is_some() || plan.is_some() {
                if let Ok(mut guard) = cache.lock() {
                    guard.token_fingerprint = Some(auth.fingerprint);
                    guard.project = project.clone();
                    guard.plan = plan.clone();
                }
                return (project, plan);
            }
        }
    }
    (None, None)
}

pub fn launch_antigravity() -> Result<(), String> {
    let mut candidates = Vec::new();
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        candidates.push(local_app_data.join("Programs").join("Antigravity IDE").join("Antigravity IDE.exe"));
        candidates.push(local_app_data.join("Programs").join("Antigravity IDE").join("Antigravity.exe"));
        candidates.push(local_app_data.join("Programs").join("Antigravity").join("Antigravity.exe"));
        candidates.push(local_app_data.join("Programs").join("antigravity").join("Antigravity.exe"));
        candidates.push(local_app_data.join("Antigravity IDE").join("Antigravity IDE.exe"));
        candidates.push(local_app_data.join("Antigravity").join("Antigravity.exe"));
    }
    if let Some(program_files) = std::env::var_os("ProgramFiles").map(PathBuf::from) {
        candidates.push(program_files.join("Antigravity IDE").join("Antigravity IDE.exe"));
        candidates.push(program_files.join("Antigravity IDE").join("Antigravity.exe"));
        candidates.push(program_files.join("Antigravity").join("Antigravity.exe"));
    }
    if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from) {
        candidates.push(program_files_x86.join("Antigravity IDE").join("Antigravity IDE.exe"));
        candidates.push(program_files_x86.join("Antigravity").join("Antigravity.exe"));
    }
    for exe in candidates {
        if exe.is_file() {
            return std::process::Command::new(exe)
                .spawn()
                .map(|_| ())
                .map_err(|_| "Unable to launch Antigravity IDE.".to_string());
        }
    }
    if let Some(vscode_exe) = find_vscode_with_antigravity_extension() {
        return std::process::Command::new(vscode_exe)
            .spawn()
            .map(|_| ())
            .map_err(|_| "Unable to launch Visual Studio Code.".to_string());
    }
    std::process::Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", "https://antigravity.google/"])
        .spawn()
        .map(|_| ())
        .map_err(|_| "Unable to open Antigravity.".to_string())
}

fn find_vscode_with_antigravity_extension() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let ext_dir = home.join(".vscode").join("extensions");
    let has_extension = fs::read_dir(ext_dir).ok()?.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("google.google-antigravity"))
    });
    if !has_extension {
        return None;
    }
    let mut vscode_candidates = Vec::new();
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        vscode_candidates.push(
            local_app_data
                .join("Programs")
                .join("Microsoft VS Code")
                .join("Code.exe"),
        );
    }
    if let Some(program_files) = std::env::var_os("ProgramFiles").map(PathBuf::from) {
        vscode_candidates.push(program_files.join("Microsoft VS Code").join("Code.exe"));
    }
    vscode_candidates.into_iter().find(|path| path.is_file())
}

fn cli_binary_name() -> &'static str {
    if cfg!(windows) {
        "agy.exe"
    } else {
        "agy"
    }
}

fn candidate_cli_paths() -> Vec<PathBuf> {
    if let Some(explicit) = std::env::var_os("ANTIGRAVITY_CLI_PATH") {
        if !explicit.is_empty() {
            return vec![PathBuf::from(explicit)];
        }
    }
    let bin = cli_binary_name();
    let mut paths = Vec::new();
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".gemini").join("bin").join(bin));
        paths.push(home.join(".gemini").join("antigravity").join("bin").join(bin));
        paths.push(home.join(".local").join("bin").join(bin));
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        paths.push(local_app_data.join("Programs").join("Antigravity").join(bin));
        paths.push(
            local_app_data
                .join("Programs")
                .join("Antigravity IDE")
                .join(bin),
        );
        paths.push(local_app_data.join("Antigravity").join(bin));
    }
    if let Some(path_env) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_env) {
            if !dir.as_os_str().is_empty() {
                paths.push(dir.join(bin));
            }
        }
    }
    let mut deduped = Vec::new();
    for path in paths {
        if !deduped.contains(&path) {
            deduped.push(path);
        }
    }
    deduped
}

fn find_cli_binary() -> Option<PathBuf> {
    candidate_cli_paths().into_iter().find(|path| path.is_file())
}

fn parse_json_from_cli_stdout(stdout: &[u8]) -> Option<Value> {
    if stdout.is_empty() || stdout.len() as u64 > MAX_RESPONSE_BYTES {
        return None;
    }
    if let Ok(value) = serde_json::from_slice::<Value>(stdout) {
        if value.is_object() {
            return Some(value);
        }
    }
    let text = String::from_utf8_lossy(stdout);
    for line in text.lines().rev() {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            if value.is_object() {
                return Some(value);
            }
        }
    }
    None
}

fn classify_cli_error(hints: &str) -> (&'static str, &'static str) {
    let lower = hints.to_ascii_lowercase();
    if lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("resource_exhausted")
    {
        (
            "unavailable",
            "Antigravity quota service is rate limited. It will retry automatically.",
        )
    } else if lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("deadline exceeded")
    {
        (
            "unavailable",
            "Antigravity CLI timed out while reading quota.",
        )
    } else if lower.contains("network")
        || lower.contains("dns")
        || lower.contains("dial tcp")
        || lower.contains("no such host")
        || lower.contains("tls handshake timeout")
        || lower.contains("unreachable")
        || lower.contains("connection refused")
    {
        (
            "unavailable",
            "Network unavailable. It will retry automatically.",
        )
    } else if lower.contains("not logged in")
        || lower.contains("not signed in")
        || lower.contains("sign in")
        || lower.contains("login required")
        || lower.contains("authentication required")
        || lower.contains("authentication failed")
        || lower.contains("to log in")
        || lower.contains("unauthenticated")
        || lower.contains("no refresh token")
        || lower.contains("invalid_grant")
        || lower.contains("saved token invalid")
        || lower.contains("empty token")
        || lower.contains("401")
        || lower.contains("oauth")
    {
        (
            "signed_out",
            "Antigravity is not signed in. Sign in directly from QuotaPulse.",
        )
    } else {
        (
            "unavailable",
            "Antigravity quota service is temporarily unavailable.",
        )
    }
}

fn parse_cli_usage_output(
    stdout: &[u8],
    stderr: &[u8],
    exit_ok: bool,
) -> Result<ParsedQuota, (&'static str, &'static str)> {
    if stdout.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(("unavailable", "Antigravity quota response format has changed."));
    }

    if let Some(payload) = parse_json_from_cli_stdout(stdout) {
        if let Some(parsed) = parse_quota_summary(&payload) {
            return Ok(parsed);
        }

        let mut hints = String::new();
        for key in ["status", "error", "message", "response"] {
            if let Some(text) = payload.get(key).and_then(Value::as_str) {
                hints.push_str(text);
                hints.push(' ');
            }
        }
        if !stderr.is_empty() {
            hints.push_str(&String::from_utf8_lossy(&stderr[..stderr.len().min(4096)]));
        }

        if !exit_ok
            || payload
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|status| !status.eq_ignore_ascii_case("SUCCESS"))
        {
            return Err(classify_cli_error(&hints));
        }

        return Err((
            "unavailable",
            "Antigravity quota response is missing supported usage windows.",
        ));
    }

    let mut hints = String::new();
    if !stdout.is_empty() {
        hints.push_str(&String::from_utf8_lossy(&stdout[..stdout.len().min(4096)]));
        hints.push(' ');
    }
    if !stderr.is_empty() {
        hints.push_str(&String::from_utf8_lossy(&stderr[..stderr.len().min(4096)]));
    }
    if !exit_ok {
        return Err(classify_cli_error(&hints));
    }
    Err(("unavailable", "Antigravity quota response format has changed."))
}

fn cli_poll_log_path() -> PathBuf {
    std::env::temp_dir().join(CLI_POLL_LOG_FILENAME)
}

fn apply_hidden_process_flags(command: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

fn build_cli_auth_check_command(cli_path: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(cli_path);
    command
        .current_dir(std::env::temp_dir())
        .env(CLI_AUTH_ACTION_ENV, "check")
        .env(CLI_DISABLE_AUTO_UPDATE_ENV, "true")
        .env(CLI_NONINTERACTIVE_HEADLESS_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    apply_hidden_process_flags(&mut command);
    command
}

fn build_cli_login_command(cli_path: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(cli_path);
    command
        .current_dir(std::env::temp_dir())
        .env(CLI_AUTH_ACTION_ENV, "login")
        .env(CLI_DISABLE_AUTO_UPDATE_ENV, "true")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    apply_hidden_process_flags(&mut command);
    command
}

fn build_cli_usage_command(cli_path: &Path, log_path: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(cli_path);
    command
        .arg("--log-file")
        .arg(log_path)
        .args([
            "--output-format",
            "json",
            "--print-timeout",
            CLI_PRINT_TIMEOUT,
            "-p",
            "/usage",
        ])
        .current_dir(std::env::temp_dir())
        .env(CLI_DISABLE_AUTO_UPDATE_ENV, "true")
        .env(CLI_NONINTERACTIVE_HEADLESS_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    apply_hidden_process_flags(&mut command);
    command
}

async fn check_cli_auth(cli_path: &Path) -> Result<(), (&'static str, &'static str)> {
    let mut command = build_cli_auth_check_command(cli_path);
    let child = command.spawn().map_err(|_| {
        (
            "unavailable",
            "Antigravity CLI could not be started.",
        )
    })?;

    let output = match tokio::time::timeout(CLI_AUTH_CHECK_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(_)) => {
            return Err((
                "unavailable",
                "Antigravity quota service is temporarily unavailable.",
            ))
        }
        Err(_) => {
            return Err((
                "unavailable",
                "Antigravity CLI timed out while reading quota.",
            ))
        }
    };

    if output.status.success() {
        return Ok(());
    }

    let mut hints = String::new();
    if !output.stdout.is_empty() {
        hints.push_str(&String::from_utf8_lossy(
            &output.stdout[..output.stdout.len().min(4096)],
        ));
        hints.push(' ');
    }
    if !output.stderr.is_empty() {
        hints.push_str(&String::from_utf8_lossy(
            &output.stderr[..output.stderr.len().min(4096)],
        ));
    }
    if hints
        .to_ascii_lowercase()
        .contains("unknown agy_cli_cde_auth_action")
    {
        return Err(("unavailable", "Antigravity CLI version is unsupported."));
    }
    if hints.trim().is_empty() {
        return Err((
            "signed_out",
            "Antigravity is not signed in. Sign in directly from QuotaPulse.",
        ));
    }
    Err(classify_cli_error(&hints))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignInOutcome {
    Authenticated,
    LaunchedExternalApp,
    Cancelled,
}

pub async fn sign_in_via_cli_or_launch(
    mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<SignInOutcome, String> {
    if let Some(cli_path) = find_cli_binary() {
        let mut child = build_cli_login_command(&cli_path)
            .spawn()
            .map_err(|_| "Unable to start Antigravity sign-in.".to_string())?;
        tokio::select! {
            res = child.wait() => match res {
                Ok(status) if status.success() => Ok(SignInOutcome::Authenticated),
                _ => Err("Gemini sign-in did not complete. Please try again.".to_string()),
            },
            _ = &mut cancel_rx => {
                let _ = child.kill().await;
                Ok(SignInOutcome::Cancelled)
            },
            _ = tokio::time::sleep(CLI_LOGIN_TIMEOUT) => {
                let _ = child.kill().await;
                Err("Browser sign-in timed out. Please try again.".to_string())
            },
        }
    } else {
        launch_antigravity()?;
        Ok(SignInOutcome::LaunchedExternalApp)
    }
}

async fn fetch_via_cli(cli_path: &Path) -> Result<ParsedQuota, (&'static str, &'static str)> {
    // Run non-interactive auth pre-flight first so signed-out state never spawns `agy -p /usage`,
    // which otherwise opens a manual OOB verification code tab in the browser every poll cycle.
    check_cli_auth(cli_path).await?;
    let log_path = cli_poll_log_path();
    let _ = fs::write(&log_path, b"");
    let mut command = build_cli_usage_command(cli_path, &log_path);

    let child = command.spawn().map_err(|_| {
        (
            "unavailable",
            "Antigravity CLI could not be started.",
        )
    })?;

    let output = match tokio::time::timeout(CLI_HARD_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(_)) => {
            return Err((
                "unavailable",
                "Antigravity quota service is temporarily unavailable.",
            ))
        }
        Err(_) => {
            return Err((
                "unavailable",
                "Antigravity CLI timed out while reading quota.",
            ))
        }
    };

    parse_cli_usage_output(&output.stdout, &output.stderr, output.status.success())
}

fn snapshot_from_parsed(parsed: ParsedQuota, plan: Option<String>) -> ProviderSnapshot {
    ProviderSnapshot {
        provider: "gemini".into(),
        display_name: "GEMINI".into(),
        plan,
        short_window: parsed.short_window,
        weekly_window: parsed.weekly_window,
        extra_pools: parsed.extra_pools,
        reset_credits: None,
        reset_credit_expires_at: Vec::new(),
        updated_at: chrono::Utc::now().to_rfc3339(),
        status: "ok".into(),
        message: None,
    }
}

async fn fetch_via_local_auth(
    client: &reqwest::Client,
    cache: &Mutex<SessionCache>,
) -> Result<ProviderSnapshot, (&'static str, &'static str)> {
    let auth_candidates = load_auth_candidates()?;

    let mut last_failure: Option<(&'static str, &'static str)> = None;
    for auth in auth_candidates {
        let request_headers = match headers(&auth) {
            Ok(value) => value,
            Err(message) => {
                last_failure = Some(("signed_out", message));
                continue;
            }
        };

        let (project, plan) = resolve_session_metadata(client, &request_headers, &auth, cache).await;
        let body = match &project {
            Some(project_id) => serde_json::json!({ "project": project_id }),
            None => serde_json::json!({}),
        };

        for base in [DAILY_BASE, PROD_BASE] {
            let url = format!("{base}/v1internal:retrieveUserQuotaSummary");
            let response = match client
                .post(&url)
                .headers(request_headers.clone())
                .json(&body)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => resp,
                Ok(resp) => {
                    let status = resp.status();
                    let failure = safe_http_failure(status);
                    if matches!(status.as_u16(), 401 | 403) {
                        if let Ok(mut guard) = cache.lock() {
                            *guard = SessionCache::default();
                        }
                    }
                    last_failure = Some(failure);
                    if status.as_u16() == 401 {
                        // Try next candidate auth file if available.
                        break;
                    }
                    continue;
                }
                Err(_) => {
                    last_failure = Some((
                        "unavailable",
                        "Network unavailable. It will retry automatically.",
                    ));
                    continue;
                }
            };

            let Ok(payload) = limited_json(response).await else {
                last_failure = Some(("unavailable", "Antigravity quota response format has changed."));
                continue;
            };

            if let Some(parsed) = parse_quota_summary(&payload) {
                return Ok(snapshot_from_parsed(parsed, plan));
            }

            last_failure = Some((
                "unavailable",
                "Antigravity quota response is missing supported usage windows.",
            ));
        }
    }

    Err(last_failure.unwrap_or((
        "unavailable",
        "Antigravity quota service is temporarily unavailable.",
    )))
}

pub async fn fetch_snapshot(
    client: &reqwest::Client,
    cache: &Mutex<SessionCache>,
) -> ProviderSnapshot {
    let mut cli_failure: Option<(&'static str, &'static str)> = None;
    if let Some(cli_path) = find_cli_binary() {
        match fetch_via_cli(&cli_path).await {
            Ok(parsed) => return snapshot_from_parsed(parsed, None),
            Err(failure) => cli_failure = Some(failure),
        }
    }

    match fetch_via_local_auth(client, cache).await {
        Ok(snapshot) => snapshot,
        Err(local_failure) => {
            let (status, message) = match cli_failure {
                Some(cli_err) if local_failure.0 == "signed_out" => cli_err,
                _ => local_failure,
            };
            ProviderSnapshot::failure_for("gemini", "GEMINI", status, message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_varint(mut value: u64, out: &mut Vec<u8>) {
        loop {
            let mut byte = (value & 0x7F) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                break;
            }
        }
    }

    fn encode_len_field(field_number: u64, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        encode_varint((field_number << 3) | 2, &mut out);
        encode_varint(data.len() as u64, &mut out);
        out.extend_from_slice(data);
        out
    }

    #[test]
    fn decodes_nested_base64_protobuf_oauth_token() {
        let token = "ya29.a0AfB_byExampleTokenForTesting1234567890";
        let mut inner = encode_len_field(1, token.as_bytes());
        inner.extend(encode_len_field(2, b"1//0gExampleRefreshToken"));
        let inner_b64 = STANDARD.encode(&inner);
        let outer = encode_len_field(6, inner_b64.as_bytes());
        let outer_b64 = STANDARD.encode(&outer);

        assert_eq!(extract_token_from_str(&outer_b64, 0).as_deref(), Some(token));
    }

    #[test]
    fn decodes_json_oauth_creds_format() {
        let token = "ya29.a0JsonTokenExampleForUnitTesting987654";
        let json = serde_json::json!({
            "access_token": token,
            "refresh_token": "1//refresh"
        })
        .to_string();
        assert_eq!(extract_token_from_str(&json, 0).as_deref(), Some(token));
    }

    #[test]
    fn parses_all_three_antigravity_groups_and_omitted_zero_fraction() {
        let payload = serde_json::json!({
            "response": {
                "groups": [
                    {
                        "displayName": "Gemini Models",
                        "buckets": [
                            {
                                "bucketId": "gemini-weekly",
                                "window": "weekly",
                                "remainingFraction": 0.97,
                                "resetTime": "2026-11-26T15:00:00Z"
                            },
                            {
                                "bucketId": "gemini-5h",
                                "window": "5h",
                                "remainingFraction": 0.93,
                                "resetTime": "2026-11-19T20:03:00Z"
                            }
                        ]
                    },
                    {
                        "displayName": "Claude and GPT models",
                        "buckets": [
                            {
                                "bucketId": "3p-weekly",
                                "window": "weekly",
                                "remainingFraction": 0.44,
                                "resetTime": "2026-11-26T15:00:00Z"
                            },
                            {
                                "bucketId": "3p-5h",
                                "window": "5h",
                                "resetTime": "2026-11-19T20:52:00Z"
                            }
                        ]
                    },
                    {
                        "displayName": "Gemini 4 Argon",
                        "buckets": [
                            {
                                "bucketId": "gemini-argon-wtu-weekly",
                                "window": "weekly",
                                "remainingFraction": 1.0
                            }
                        ]
                    }
                ]
            }
        });

        let parsed = parse_quota_summary(&payload).expect("should parse all groups");
        assert_eq!(parsed.short_window.as_ref().unwrap().remaining_percent, 93.0);
        assert_eq!(parsed.weekly_window.as_ref().unwrap().remaining_percent, 97.0);
        assert_eq!(parsed.extra_pools.len(), 2);

        let third_party = &parsed.extra_pools[0];
        assert_eq!(third_party.id, "antigravity_3p");
        assert_eq!(third_party.display_name, "Claude & GPT");
        assert_eq!(third_party.short_window.as_ref().unwrap().remaining_percent, 0.0);
        assert_eq!(third_party.weekly_window.as_ref().unwrap().remaining_percent, 44.0);

        let argon = &parsed.extra_pools[1];
        assert_eq!(argon.id, "gemini_argon");
        assert_eq!(argon.display_name, "Gemini 4 Argon");
        assert!(argon.short_window.is_none());
        assert_eq!(argon.weekly_window.as_ref().unwrap().remaining_percent, 100.0);
    }

    #[test]
    fn parses_flat_quota_summary_buckets_and_rejects_empty_responses() {
        let flat = serde_json::json!({
            "quotaSummary": {
                "buckets": [
                    {
                        "windowType": "WINDOW_5_HOUR",
                        "remainingFraction": 0.85,
                        "resetTime": "2026-11-19T22:00:00Z"
                    },
                    {
                        "windowType": "WINDOW_WEEKLY",
                        "remainingFraction": 0.92,
                        "resetTime": "2026-11-26T00:00:00Z"
                    }
                ]
            }
        });
        let parsed = parse_quota_summary(&flat).expect("should parse flat buckets");
        assert_eq!(parsed.short_window.unwrap().remaining_percent, 85.0);
        assert_eq!(parsed.weekly_window.unwrap().remaining_percent, 92.0);

        let empty = serde_json::json!({ "response": { "groups": [] } });
        assert!(parse_quota_summary(&empty).is_none());
    }

    #[test]
    fn extracts_project_and_normalizes_plan_badge() {
        let response = serde_json::json!({
            "cloudaicompanionProject": { "id": "proj-123" },
            "currentTier": { "id": "g1-pro-tier", "name": "Google AI Pro" }
        });
        let (project, plan) = extract_project_and_plan(&response);
        assert_eq!(project.as_deref(), Some("proj-123"));
        assert_eq!(plan.as_deref(), Some("PRO"));
        assert_eq!(normalize_plan_badge("g1-ultra-tier"), "ULTRA");
        assert_eq!(normalize_plan_badge("free-tier"), "FREE");
    }

    #[test]
    fn decodes_deeply_nested_protobuf_topic_row_value_oauth_token() {
        let token = "ya29.a0DeepNestedTokenForRealWorldSync1234567890";
        let oauth_info = encode_len_field(1, token.as_bytes());
        let row_value = encode_len_field(1, STANDARD.encode(&oauth_info).as_bytes());
        let topic_entry = encode_len_field(2, &row_value);
        let topic = encode_len_field(1, &topic_entry);
        let encoded = STANDARD.encode(&topic);

        assert_eq!(extract_token_from_str(&encoded, 0).as_deref(), Some(token));
    }

    #[test]
    fn reads_blob_and_text_values_from_vscdb_item_table() {
        let token = "ya29.a0SqliteBlobTokenTestExample1234567890";
        let blob_payload = encode_len_field(1, token.as_bytes());
        let temp_path = std::env::temp_dir().join(format!(
            "quotapulse-vscdb-test-{}.vscdb",
            std::process::id()
        ));
        let _ = fs::remove_file(&temp_path);
        {
            let conn = Connection::open(&temp_path).expect("create temp db");
            conn.execute(
                "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
                rusqlite::params!["antigravityUnifiedStateSync.oauthToken", blob_payload],
            )
            .unwrap();
        }
        let outcome = load_token_from_vscdb(&temp_path);
        let _ = fs::remove_file(&temp_path);
        match outcome {
            DbReadOutcome::Found(found) => assert_eq!(found, token),
            _ => panic!("expected token to be extracted from BLOB row"),
        }
    }

    #[test]
    fn extracts_embedded_google_token_from_opaque_binary_wrapper() {
        let token = "ya29.a0OpaqueWrapperFallbackTokenExample987654321";
        let mut bytes = vec![0xFF, 0x00, 0xFE, 0x1B];
        bytes.extend_from_slice(token.as_bytes());
        bytes.extend_from_slice(&[0x00, 0xFF]);

        assert_eq!(extract_token_from_bytes(&bytes, 0).as_deref(), Some(token));
    }

    #[test]
    fn parses_agy_cli_usage_json_with_string_response_and_all_three_pools() {
        let cli_json = serde_json::json!({
            "conversation_id": "",
            "status": "SUCCESS",
            "response": "Gemini Models\tWeekly Limit Remaining\t88%\t2026-11-26T07:43:29Z\n",
            "duration_seconds": 0,
            "num_turns": 0,
            "usage": { "total_tokens": 0 },
            "command": {
                "name": "usage",
                "data": {
                    "groups": [
                        {
                            "name": "Gemini Models",
                            "buckets": [
                                {
                                    "id": "gemini-weekly",
                                    "name": "Weekly Limit Remaining",
                                    "window": "weekly",
                                    "remaining_fraction": 0.8756,
                                    "reset_time": "2026-11-26T07:43:29Z"
                                },
                                {
                                    "id": "gemini-5h",
                                    "name": "Five Hour Limit Remaining",
                                    "window": "5h",
                                    "remaining_fraction": 0.6921,
                                    "reset_time": "2026-11-19T19:18:58Z"
                                }
                            ]
                        },
                        {
                            "name": "Claude and GPT models",
                            "buckets": [
                                {
                                    "id": "3p-weekly",
                                    "window": "weekly",
                                    "remaining_fraction": 0.2421,
                                    "reset_time": "2026-11-26T08:32:28Z"
                                },
                                {
                                    "id": "3p-5h",
                                    "window": "5h",
                                    "remaining_fraction": 0.5784,
                                    "reset_time": "2026-11-19T21:12:41Z"
                                }
                            ]
                        },
                        {
                            "name": "Gemini 4 Argon",
                            "buckets": [
                                {
                                    "id": "gemini-argon-wtu-weekly",
                                    "window": "weekly",
                                    "remaining_fraction": 1.0,
                                    "reset_time": "2026-11-26T16:53:48Z"
                                }
                            ]
                        }
                    ]
                }
            }
        });
        let raw = format!("Notice: noisy log line\n{}\n", cli_json);
        let parsed = parse_cli_usage_output(raw.as_bytes(), b"", true).expect("should parse agy CLI output");
        assert!((parsed.short_window.as_ref().unwrap().remaining_percent - 69.21).abs() < 0.01);
        assert!((parsed.weekly_window.as_ref().unwrap().remaining_percent - 87.56).abs() < 0.01);
        assert_eq!(parsed.extra_pools.len(), 2);
        assert_eq!(parsed.extra_pools[0].id, "antigravity_3p");
        assert!((parsed.extra_pools[0].short_window.as_ref().unwrap().remaining_percent - 57.84).abs() < 0.01);
        assert_eq!(parsed.extra_pools[1].id, "gemini_argon");
        assert_eq!(parsed.extra_pools[1].weekly_window.as_ref().unwrap().remaining_percent, 100.0);
    }

    #[test]
    fn classifies_agy_cli_errors_safely() {
        let signed_out = parse_cli_usage_output(
            b"",
            b"Error: You are not logged into Antigravity (C:\\Users\\secret\\path)",
            false,
        )
        .expect_err("expected signed_out error");
        assert_eq!(signed_out.0, "signed_out");
        assert!(!signed_out.1.contains("secret"));

        let timed_out = parse_cli_usage_output(b"", b"context deadline exceeded", false)
            .expect_err("expected timeout error");
        assert_eq!(timed_out.0, "unavailable");
        assert!(timed_out.1.to_ascii_lowercase().contains("timed out"));

        let rate_limited = parse_cli_usage_output(
            br#"{"status":"ERROR","error":"HTTP 429 RESOURCE_EXHAUSTED"}"#,
            b"",
            false,
        )
        .expect_err("expected rate limit error");
        assert_eq!(rate_limited.0, "unavailable");
        assert!(rate_limited.1.to_ascii_lowercase().contains("rate limited"));

        let headless_auth = parse_cli_usage_output(
            b"Error: authentication required. Run 'agy' to log in, then retry.",
            b"",
            false,
        )
        .expect_err("expected signed_out error for headless auth prompt");
        assert_eq!(headless_auth.0, "signed_out");
    }

    #[test]
    fn parses_gemini_ultra_weekly_only_with_and_without_merged_argon() {
        let ultra_with_3p = serde_json::json!({
            "status": "SUCCESS",
            "command": {
                "name": "usage",
                "data": {
                    "groups": [
                        {
                            "name": "Gemini Models",
                            "buckets": [
                                {
                                    "id": "gemini-weekly",
                                    "window": "weekly",
                                    "remaining_fraction": 0.91,
                                    "reset_time": "2026-11-29T03:05:47Z"
                                }
                            ]
                        },
                        {
                            "name": "Claude and GPT models",
                            "buckets": [
                                {
                                    "id": "3p-weekly",
                                    "window": "weekly",
                                    "remaining_fraction": 0.56,
                                    "reset_time": "2026-11-26T08:32:28Z"
                                }
                            ]
                        }
                    ]
                }
            }
        });
        let parsed = parse_quota_summary(&ultra_with_3p).expect("Ultra weekly-only should parse");
        assert!(parsed.short_window.is_none());
        assert_eq!(parsed.weekly_window.as_ref().unwrap().remaining_percent, 91.0);
        assert_eq!(parsed.extra_pools.len(), 1);
        assert_eq!(parsed.extra_pools[0].id, "antigravity_3p");
        assert!(parsed.extra_pools[0].short_window.is_none());
        assert!(
            (parsed.extra_pools[0]
                .weekly_window
                .as_ref()
                .unwrap()
                .remaining_percent
                - 56.0)
                .abs()
                < 0.01
        );
        let snap = snapshot_from_parsed(parsed, Some("ULTRA".into()));
        assert_eq!(snap.status, "ok");
        assert!(snap.has_any_window());

        let ultra_merged_argon = serde_json::json!({
            "response": {
                "groups": [
                    {
                        "displayName": "Gemini Models (incl. Argon)",
                        "buckets": [
                            {
                                "bucketId": "gemini-weekly",
                                "window": "weekly",
                                "remainingFraction": 0.84,
                                "resetTime": "2026-11-29T03:05:47Z"
                            }
                        ]
                    }
                ]
            }
        });
        let parsed_merged = parse_quota_summary(&ultra_merged_argon)
            .expect("Ultra merged Argon group should map to primary Gemini pool");
        assert!(parsed_merged.short_window.is_none());
        assert_eq!(parsed_merged.weekly_window.as_ref().unwrap().remaining_percent, 84.0);
        assert!(parsed_merged.extra_pools.is_empty());
        let snap_merged = snapshot_from_parsed(parsed_merged, Some("ULTRA".into()));
        assert_eq!(snap_merged.status, "ok");
        assert!(snap_merged.has_any_window());
    }

    #[test]
    fn cli_poll_log_path_targets_single_temp_file() {
        let log_path = cli_poll_log_path();
        assert_eq!(log_path.parent(), Some(std::env::temp_dir().as_path()));
        assert_eq!(
            log_path.file_name().and_then(|name| name.to_str()),
            Some("quotapulse-agy-poll.log")
        );
    }

    #[test]
    fn builds_silent_single_log_cli_command() {
        let cli_path = Path::new("agy.exe");
        let log_path = cli_poll_log_path();
        let command = build_cli_usage_command(cli_path, &log_path);
        let std_cmd = command.as_std();
        let args: Vec<&std::ffi::OsStr> = std_cmd.get_args().collect();
        assert_eq!(
            args,
            vec![
                std::ffi::OsStr::new("--log-file"),
                log_path.as_os_str(),
                std::ffi::OsStr::new("--output-format"),
                std::ffi::OsStr::new("json"),
                std::ffi::OsStr::new("--print-timeout"),
                std::ffi::OsStr::new("15s"),
                std::ffi::OsStr::new("-p"),
                std::ffi::OsStr::new("/usage"),
            ]
        );
        let envs: Vec<(&std::ffi::OsStr, Option<&std::ffi::OsStr>)> = std_cmd.get_envs().collect();
        assert!(envs.contains(&(
            std::ffi::OsStr::new("AGY_CLI_DISABLE_AUTO_UPDATE"),
            Some(std::ffi::OsStr::new("true"))
        )));
        assert!(envs.contains(&(
            std::ffi::OsStr::new("AGY_CLI_NONINTERACTIVE_HEADLESS"),
            Some(std::ffi::OsStr::new("1"))
        )));
        assert!(!envs.iter().any(|(k, _)| *k == std::ffi::OsStr::new("SSH_CLIENT")));
    }

    #[test]
    fn builds_silent_auth_check_and_loopback_login_commands() {
        let cli_path = Path::new("agy.exe");

        let check_cmd = build_cli_auth_check_command(cli_path);
        let check_std = check_cmd.as_std();
        assert_eq!(check_std.get_args().count(), 0);
        let check_envs: Vec<(&std::ffi::OsStr, Option<&std::ffi::OsStr>)> =
            check_std.get_envs().collect();
        assert!(check_envs.contains(&(
            std::ffi::OsStr::new("AGY_CLI_CDE_AUTH_ACTION"),
            Some(std::ffi::OsStr::new("check"))
        )));
        assert!(check_envs.contains(&(
            std::ffi::OsStr::new("AGY_CLI_DISABLE_AUTO_UPDATE"),
            Some(std::ffi::OsStr::new("true"))
        )));
        assert!(check_envs.contains(&(
            std::ffi::OsStr::new("AGY_CLI_NONINTERACTIVE_HEADLESS"),
            Some(std::ffi::OsStr::new("1"))
        )));

        let login_cmd = build_cli_login_command(cli_path);
        let login_std = login_cmd.as_std();
        assert_eq!(login_std.get_args().count(), 0);
        let login_envs: Vec<(&std::ffi::OsStr, Option<&std::ffi::OsStr>)> =
            login_std.get_envs().collect();
        assert!(login_envs.contains(&(
            std::ffi::OsStr::new("AGY_CLI_CDE_AUTH_ACTION"),
            Some(std::ffi::OsStr::new("login"))
        )));
        assert!(login_envs.contains(&(
            std::ffi::OsStr::new("AGY_CLI_DISABLE_AUTO_UPDATE"),
            Some(std::ffi::OsStr::new("true"))
        )));
    }

    #[test]
    fn classifies_offline_oauth_refresh_as_unavailable_not_signed_out() {
        let offline = classify_cli_error(
            r#"Post "https://oauth2.googleapis.com/token": dial tcp: lookup oauth2.googleapis.com: no such host"#,
        );
        assert_eq!(offline.0, "unavailable");

        let signed_out = classify_cli_error("Authentication failed: no refresh token");
        assert_eq!(signed_out.0, "signed_out");
    }

    #[tokio::test]
    #[ignore = "requires local agy CLI login"]
    async fn live_agy_cli_fetch_snapshot_smoke() {
        let cli = find_cli_binary().expect("agy CLI binary should be discovered");
        let parsed = fetch_via_cli(&cli)
            .await
            .expect("agy CLI /usage should return quota pools");
        assert!(parsed.short_window.is_some());
        assert!(parsed.weekly_window.is_some());
        assert!(!parsed.extra_pools.is_empty());
    }
}
