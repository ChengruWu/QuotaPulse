use std::{fs, path::PathBuf};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION};
use serde_json::Value;

use crate::models::{ProviderSnapshot, UsageWindow};

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const CREDITS_URL: &str = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;
const MAX_AUTH_BYTES: u64 = 256 * 1024;

struct Auth {
    access_token: String,
    account_id: Option<String>,
}

fn auth_path() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
        .map(|home| home.join("auth.json"))
}

fn pick_string<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| value.get(*key)?.as_str())
}

fn account_id_from_jwt(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    pick_string(
        &value,
        &[
            "https://api.openai.com/auth.chatgpt_account_id",
            "chatgpt_account_id",
        ],
    )
    .map(str::to_owned)
}

fn load_auth() -> Result<Auth, &'static str> {
    let path = auth_path().ok_or("Codex login was not found.")?;
    let metadata = fs::metadata(&path).map_err(|_| "Please sign in to Codex Desktop first.")?;
    if !metadata.is_file() || metadata.len() > MAX_AUTH_BYTES {
        return Err("Codex login data is unavailable.");
    }
    let raw = fs::read_to_string(path).map_err(|_| "Please sign in to Codex Desktop first.")?;
    let value: Value = serde_json::from_str(&raw).map_err(|_| "Codex login format has changed.")?;
    let tokens = value.get("tokens").unwrap_or(&value);
    let access_token = pick_string(tokens, &["access_token", "accessToken"])
        .ok_or("Codex login expired. Please sign in again.")?
        .to_owned();
    let account_id = pick_string(tokens, &["account_id", "accountId"])
        .map(str::to_owned)
        .or_else(|| account_id_from_jwt(&access_token));
    Ok(Auth {
        access_token,
        account_id,
    })
}

fn headers(auth: &Auth) -> Result<HeaderMap, &'static str> {
    let mut result = HeaderMap::new();
    let mut bearer = HeaderValue::from_str(&format!("Bearer {}", auth.access_token))
        .map_err(|_| "Codex login data is invalid.")?;
    bearer.set_sensitive(true);
    result.insert(AUTHORIZATION, bearer);
    result.insert(ACCEPT, HeaderValue::from_static("application/json"));
    result.insert("originator", HeaderValue::from_static("Codex Desktop"));
    result.insert("OAI-Product-Sku", HeaderValue::from_static("CODEX"));
    if let Some(account_id) = &auth.account_id {
        let mut value =
            HeaderValue::from_str(account_id).map_err(|_| "Account identifier is invalid.")?;
        value.set_sensitive(true);
        result.insert("ChatGPT-Account-Id", value);
    }
    Ok(result)
}

fn number_with_key<'a>(value: &'a Value, keys: &[&'a str]) -> Option<(&'a str, f64)> {
    keys.iter()
        .find_map(|key| value.get(*key)?.as_f64().map(|number| (*key, number)))
}

fn integer(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| {
        let value = value.get(*key)?;
        value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|item| u64::try_from(item).ok()))
    })
}

fn timestamp(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        let item = value.get(*key)?;
        if let Some(text) = item.as_str() {
            return Some(text.to_owned());
        }
        item.as_i64()
            .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
            .map(|time| time.to_rfc3339())
    })
}

fn collect_reset_credit_expirations(value: &Value) -> Vec<String> {
    fn visit(value: &Value, output: &mut Vec<String>) {
        match value {
            Value::Array(items) => {
                for item in items {
                    visit(item, output);
                }
            }
            Value::Object(map) => {
                if let Some(time) = timestamp(
                    value,
                    &[
                        "expires_at",
                        "expiresAt",
                        "expiration_time",
                        "expirationTime",
                        "expires",
                    ],
                ) {
                    output.push(time);
                }
                for key in [
                    "credits",
                    "reset_credits",
                    "resetCredits",
                    "available",
                    "items",
                    "grants",
                ] {
                    if let Some(child) = map.get(key) {
                        visit(child, output);
                    }
                }
            }
            _ => {}
        }
    }

    let mut expirations = Vec::new();
    visit(value, &mut expirations);
    expirations.sort();
    expirations.dedup();
    expirations
}

fn reset_credit_count(value: &Value) -> Option<u64> {
    integer(
        value,
        &[
            "available_count",
            "availableCount",
            "remaining",
            "count",
            "quantity",
        ],
    )
}

fn parse_reset_credit_data(dedicated: Option<&Value>, usage: &Value) -> (Option<u64>, Vec<String>) {
    let embedded = usage
        .get("rate_limit_reset_credits")
        .or_else(|| usage.get("rateLimitResetCredits"));
    let count = dedicated
        .and_then(reset_credit_count)
        .or_else(|| embedded.and_then(reset_credit_count));
    let embedded_expirations = embedded
        .map(collect_reset_credit_expirations)
        .unwrap_or_default();
    let dedicated_expirations = dedicated
        .map(collect_reset_credit_expirations)
        .unwrap_or_default();
    let expirations = if dedicated_expirations.is_empty() {
        embedded_expirations
    } else {
        dedicated_expirations
    };
    (count, expirations)
}

fn scale_ratio_field(key: &str, value: f64) -> bool {
    matches!(
        key,
        "remaining_ratio" | "remainingRatio" | "used_ratio" | "usedRatio" | "utilization"
    ) || (!key.contains("percent") && !key.contains("pct") && value <= 1.0)
}

fn parse_window(value: Option<&Value>) -> Option<UsageWindow> {
    let value = value?;
    let remaining_percent = if let Some((key, remaining)) = number_with_key(
        value,
        &[
            "remaining_percent",
            "remainingPercent",
            "remaining_pct",
            "remainingPct",
            "remaining_ratio",
            "remainingRatio",
            "remaining",
        ],
    ) {
        if scale_ratio_field(key, remaining) {
            remaining * 100.0
        } else {
            remaining
        }
    } else {
        let (key, used) = number_with_key(
            value,
            &[
                "used_percent",
                "usedPercent",
                "used_pct",
                "usedPct",
                "used_ratio",
                "usedRatio",
                "utilization",
                "used",
            ],
        )?;
        let used_percent = if scale_ratio_field(key, used) {
            used * 100.0
        } else {
            used
        };
        100.0 - used_percent
    };
    Some(UsageWindow {
        remaining_percent: remaining_percent.clamp(0.0, 100.0),
        resets_at: timestamp(
            value,
            &[
                "reset_at",
                "resetAt",
                "resets_at",
                "resetsAt",
                "reset_time",
                "resetTime",
            ],
        ),
        window_seconds: integer(
            value,
            &[
                "limit_window_seconds",
                "limitWindowSeconds",
                "window_seconds",
                "windowSeconds",
                "duration_seconds",
                "durationSeconds",
                "period_seconds",
                "periodSeconds",
            ],
        )
        .unwrap_or(0),
    })
}

fn find_window<'a>(
    rate_limit: &'a Value,
    names: &[&str],
    expected_seconds: u64,
) -> Option<&'a Value> {
    let tolerance = if expected_seconds >= 86_400 { 3600 } else { 120 };
    let duration_is_compatible = |value: &Value| {
        parse_window(Some(value)).is_some_and(|window| {
            window.window_seconds == 0
                || expected_seconds == 0
                || window.window_seconds.abs_diff(expected_seconds) <= tolerance
        })
    };
    let matches_duration = |value: &Value| {
        parse_window(Some(value)).is_some_and(|window| {
            expected_seconds > 0
                && window.window_seconds > 0
                && window.window_seconds.abs_diff(expected_seconds) <= tolerance
        })
    };

    // The API can assign the same quota period to different slots depending on
    // the account. Prefer the declared duration over primary/secondary naming.
    if let Some(fields) = rate_limit.as_object() {
        for value in fields.values() {
            if matches_duration(value) {
                return Some(value);
            }
        }
    }

    for key in [
        "windows",
        "limit_windows",
        "limitWindows",
        "limits",
        "buckets",
    ] {
        let Some(items) = rate_limit.get(key).and_then(Value::as_array) else {
            continue;
        };
        if let Some(item) = items.iter().find(|item| matches_duration(item)) {
            return Some(item);
        }
    }

    for name in names {
        if let Some(value) = rate_limit.get(*name) {
            if duration_is_compatible(value) {
                return Some(value);
            }
        }
    }

    for key in [
        "windows",
        "limit_windows",
        "limitWindows",
        "limits",
        "buckets",
    ] {
        let Some(items) = rate_limit.get(key).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            if parse_window(Some(item)).is_none() {
                continue;
            }
            let matches_name = pick_string(item, &["name", "type", "id", "window", "label"])
                .map(|text| {
                    let lower = text.to_ascii_lowercase();
                    names.iter().any(|name| {
                        lower == name.to_ascii_lowercase()
                            || lower.contains(&name.to_ascii_lowercase())
                    })
                })
                .unwrap_or(false);
            if matches_name && duration_is_compatible(item) {
                return Some(item);
            }
        }
    }

    None
}

fn parse_usage_windows(usage: &Value) -> (Option<UsageWindow>, Option<UsageWindow>) {
    let rate_limit = usage
        .get("rate_limit")
        .or_else(|| usage.get("rateLimit"))
        .unwrap_or(usage);
    let short_window = parse_window(find_window(
        rate_limit,
        &[
            "primary_window",
            "primaryWindow",
            "short_window",
            "shortWindow",
            "five_hour_window",
            "fiveHourWindow",
            "5h",
            "primary",
        ],
        18_000,
    ));
    let weekly_window = parse_window(find_window(
        rate_limit,
        &[
            "secondary_window",
            "secondaryWindow",
            "weekly_window",
            "weeklyWindow",
            "week_window",
            "weekWindow",
            "weekly",
            "secondary",
        ],
        604_800,
    ));
    (short_window, weekly_window)
}

fn safe_http_failure(status: reqwest::StatusCode) -> (&'static str, &'static str) {
    match status.as_u16() {
        401 | 403 => ("signed_out", "Codex login expired. Please sign in again."),
        429 => (
            "unavailable",
            "Quota service is rate limited. It will retry automatically.",
        ),
        _ => ("unavailable", "Quota service is temporarily unavailable."),
    }
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

pub async fn fetch_snapshot(client: &reqwest::Client) -> ProviderSnapshot {
    let auth = match load_auth() {
        Ok(value) => value,
        Err(message) => return ProviderSnapshot::failure("signed_out", message),
    };
    let request_headers = match headers(&auth) {
        Ok(value) => value,
        Err(message) => return ProviderSnapshot::failure("signed_out", message),
    };

    let (usage_result, credits_result) = tokio::join!(
        client
            .get(USAGE_URL)
            .headers(request_headers.clone())
            .send(),
        client.get(CREDITS_URL).headers(request_headers).send(),
    );

    let usage_response = match usage_result {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            let (status, message) = safe_http_failure(response.status());
            return ProviderSnapshot::failure(status, message);
        }
        Err(_) => {
            return ProviderSnapshot::failure(
                "unavailable",
                "Network unavailable. It will retry automatically.",
            )
        }
    };
    let usage: Value = match limited_json(usage_response).await {
        Ok(value) => value,
        Err(_) => {
            return ProviderSnapshot::failure("unavailable", "Quota response format has changed.")
        }
    };
    let (short_window, weekly_window) = parse_usage_windows(&usage);
    if short_window.is_none() && weekly_window.is_none() {
        return ProviderSnapshot::failure(
            "unavailable",
            "Quota response is missing supported usage windows.",
        );
    }

    let credits = match credits_result {
        Ok(response) if response.status().is_success() => limited_json(response).await.ok(),
        _ => None,
    };
    let (reset_credits, reset_credit_expires_at) =
        parse_reset_credit_data(credits.as_ref(), &usage);

    ProviderSnapshot {
        provider: "codex".into(),
        display_name: "CODEX".into(),
        plan: pick_string(&usage, &["plan_type", "planType"]).map(|value| value.to_uppercase()),
        short_window,
        weekly_window,
        extra_pools: Vec::new(),
        reset_credits,
        reset_credit_expires_at,
        updated_at: chrono::Utc::now().to_rfc3339(),
        status: "ok".into(),
        message: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_window_shapes() {
        let snake = serde_json::json!({
            "used_percent": 26,
            "reset_at": 1738300000,
            "limit_window_seconds": 18000
        });
        let window = parse_window(Some(&snake)).unwrap();
        assert_eq!(window.remaining_percent, 74.0);
        assert_eq!(window.window_seconds, 18000);
        let camel = serde_json::json!({
            "utilization": 0.4,
            "resetsAt": "2026-07-07T00:00:00Z",
            "windowSeconds": 604800
        });
        assert_eq!(parse_window(Some(&camel)).unwrap().remaining_percent, 60.0);
    }

    #[test]
    fn prefers_explicit_remaining_percent() {
        let value = serde_json::json!({
            "remainingPercent": 73.4,
            "usedPercent": 99,
            "resetTime": "2026-07-07T00:00:00Z",
            "durationSeconds": 18000
        });
        let window = parse_window(Some(&value)).unwrap();
        assert_eq!(window.remaining_percent, 73.4);
        assert_eq!(window.window_seconds, 18000);
    }

    #[test]
    fn treats_fractional_percent_fields_as_ratios() {
        let explicit_remaining = serde_json::json!({"remaining": 0.25, "periodSeconds": 18000});
        assert_eq!(
            parse_window(Some(&explicit_remaining))
                .unwrap()
                .remaining_percent,
            25.0
        );

        let used_ratio = serde_json::json!({"used": 0.25, "periodSeconds": 18000});
        assert_eq!(
            parse_window(Some(&used_ratio)).unwrap().remaining_percent,
            75.0
        );
    }

    #[test]
    fn does_not_scale_explicit_percent_fields() {
        let explicit_remaining =
            serde_json::json!({"remaining_percent": 0.4, "windowSeconds": 18000});
        assert_eq!(
            parse_window(Some(&explicit_remaining))
                .unwrap()
                .remaining_percent,
            0.4
        );

        let explicit_used = serde_json::json!({"used_percent": 0.4, "windowSeconds": 18000});
        assert_eq!(
            parse_window(Some(&explicit_used))
                .unwrap()
                .remaining_percent,
            99.6
        );
    }

    #[test]
    fn finds_window_by_duration_or_name_in_arrays() {
        let rate_limit = serde_json::json!({
            "windows": [
                {"name": "weekly", "remainingPercent": 88, "windowSeconds": 604800},
                {"name": "primary", "remainingPercent": 51, "windowSeconds": 18000}
            ]
        });
        let short = parse_window(find_window(
            &rate_limit,
            &["primary_window", "primary"],
            18_000,
        ))
        .unwrap();
        let weekly = parse_window(find_window(
            &rate_limit,
            &["secondary_window", "weekly"],
            604_800,
        ))
        .unwrap();
        assert_eq!(short.remaining_percent, 51.0);
        assert_eq!(weekly.remaining_percent, 88.0);
    }

    #[test]
    fn finds_weekly_window_in_primary_slot_by_duration() {
        let rate_limit = serde_json::json!({
            "primary_window": {
                "used_percent": 27,
                "limit_window_seconds": 604800,
                "reset_at": 1784516228
            },
            "secondary_window": null
        });
        let weekly = parse_window(find_window(
            &rate_limit,
            &["secondary_window", "weekly_window", "weekly"],
            604_800,
        ))
        .unwrap();
        assert_eq!(weekly.remaining_percent, 73.0);
        assert_eq!(weekly.window_seconds, 604_800);

        let five_hour_only = serde_json::json!({
            "primary_window": {
                "used_percent": 27,
                "limit_window_seconds": 18000
            },
            "secondary_window": null
        });
        assert!(find_window(
            &five_hour_only,
            &["secondary_window", "weekly_window", "weekly"],
            604_800,
        )
        .is_none());

        let (short, weekly) = parse_usage_windows(&rate_limit);
        assert!(short.is_none());
        assert_eq!(weekly.unwrap().remaining_percent, 73.0);
    }

    #[test]
    fn parses_both_usage_windows_even_when_slots_are_swapped() {
        let usage = serde_json::json!({
            "rate_limit": {
                "primary_window": {
                    "used_percent": 12,
                    "limit_window_seconds": 604800
                },
                "secondary_window": {
                    "used_percent": 35,
                    "limit_window_seconds": 18000
                }
            }
        });
        let (short, weekly) = parse_usage_windows(&usage);
        assert_eq!(short.unwrap().remaining_percent, 65.0);
        assert_eq!(weekly.unwrap().remaining_percent, 88.0);
    }

    #[test]
    fn accepts_either_usage_window_and_rejects_responses_without_one() {
        let short_only = serde_json::json!({
            "rateLimit": {
                "primaryWindow": {
                    "remainingPercent": 77,
                    "windowSeconds": 18000
                }
            }
        });
        let (short, weekly) = parse_usage_windows(&short_only);
        assert_eq!(short.unwrap().remaining_percent, 77.0);
        assert!(weekly.is_none());

        let weekly_only = serde_json::json!({
            "rate_limit": {
                "windows": [{
                    "name": "weekly",
                    "remaining_percent": 44,
                    "window_seconds": 604800
                }]
            }
        });
        let (short, weekly) = parse_usage_windows(&weekly_only);
        assert!(short.is_none());
        assert_eq!(weekly.unwrap().remaining_percent, 44.0);

        let (short, weekly) = parse_usage_windows(&serde_json::json!({
            "rate_limit": {"primary_window": null, "secondary_window": null}
        }));
        assert!(short.is_none());
        assert!(weekly.is_none());
    }

    #[test]
    fn parses_reset_credit_count_and_sorted_unique_expirations() {
        let snake = serde_json::json!({
            "available_count": 2,
            "credits": [
                {"expires_at": "2026-08-03T00:00:00Z"},
                {"expiration_time": 1785628800},
                {"expires_at": "2026-08-03T00:00:00Z"}
            ]
        });
        assert_eq!(reset_credit_count(&snake), Some(2));
        assert_eq!(
            collect_reset_credit_expirations(&snake),
            vec![
                "2026-08-02T00:00:00+00:00".to_string(),
                "2026-08-03T00:00:00Z".to_string(),
            ]
        );

        let camel = serde_json::json!({
            "availableCount": 3,
            "grants": [{"expiresAt": "2026-08-04T00:00:00Z"}]
        });
        assert_eq!(reset_credit_count(&camel), Some(3));
        assert_eq!(
            collect_reset_credit_expirations(&camel),
            vec!["2026-08-04T00:00:00Z".to_string()]
        );
    }

    #[test]
    fn prefers_dedicated_reset_credit_data_and_falls_back_to_usage() {
        let usage = serde_json::json!({
            "rate_limit_reset_credits": {
                "available_count": 1,
                "credits": [{"expires_at": "2026-08-05T00:00:00Z"}]
            }
        });
        let dedicated = serde_json::json!({"availableCount": 4});
        assert_eq!(
            parse_reset_credit_data(Some(&dedicated), &usage),
            (Some(4), vec!["2026-08-05T00:00:00Z".to_string()])
        );
        assert_eq!(
            parse_reset_credit_data(None, &usage),
            (Some(1), vec!["2026-08-05T00:00:00Z".to_string()])
        );
    }

    #[test]
    fn treats_invalid_reset_credit_data_as_unknown() {
        let usage = serde_json::json!({"rate_limit_reset_credits": {"available_count": -1}});
        let dedicated = serde_json::json!({"availableCount": "many", "credits": [{}]});
        assert_eq!(
            parse_reset_credit_data(Some(&dedicated), &usage),
            (None, Vec::new())
        );
    }

    #[test]
    fn parses_gpt_pro_weekly_only_windows() {
        let primary_weekly = serde_json::json!({
            "plan_type": "pro",
            "rate_limit": {
                "primary_window": {
                    "used_percent": 18,
                    "limit_window_seconds": 604800,
                    "reset_at": 1784516228
                },
                "secondary_window": null
            }
        });
        let (short, weekly) = parse_usage_windows(&primary_weekly);
        assert!(short.is_none());
        let weekly = weekly.expect("GPT Pro weekly window should parse");
        assert_eq!(weekly.remaining_percent, 82.0);
        assert_eq!(weekly.window_seconds, 604_800);

        let secondary_weekly = serde_json::json!({
            "plan_type": "pro",
            "rate_limit": {
                "primary_window": null,
                "secondary_window": {
                    "remaining_percent": 64,
                    "limit_window_seconds": 604800,
                    "reset_at": 1784516228
                }
            }
        });
        let (short, weekly) = parse_usage_windows(&secondary_weekly);
        assert!(short.is_none());
        let weekly = weekly.expect("GPT Pro secondary weekly window should parse");
        assert_eq!(weekly.remaining_percent, 64.0);
        assert_eq!(weekly.window_seconds, 604_800);
    }
}
