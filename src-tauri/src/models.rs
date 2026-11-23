use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub remaining_percent: f64,
    pub resets_at: Option<String>,
    pub window_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaPool {
    pub id: String,
    pub display_name: String,
    pub short_window: Option<UsageWindow>,
    pub weekly_window: Option<UsageWindow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSnapshot {
    pub provider: String,
    pub display_name: String,
    pub plan: Option<String>,
    pub short_window: Option<UsageWindow>,
    pub weekly_window: Option<UsageWindow>,
    #[serde(default)]
    pub extra_pools: Vec<QuotaPool>,
    pub reset_credits: Option<u64>,
    pub reset_credit_expires_at: Vec<String>,
    pub updated_at: String,
    pub status: String,
    pub message: Option<String>,
}

impl ProviderSnapshot {
    pub fn failure_for(provider: &str, display_name: &str, status: &str, message: &str) -> Self {
        Self {
            provider: provider.into(),
            display_name: display_name.into(),
            plan: None,
            short_window: None,
            weekly_window: None,
            extra_pools: Vec::new(),
            reset_credits: None,
            reset_credit_expires_at: Vec::new(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            status: status.into(),
            message: Some(message.into()),
        }
    }

    pub fn failure(status: &str, message: &str) -> Self {
        Self::failure_for("codex", "CODEX", status, message)
    }

    pub fn has_any_window(&self) -> bool {
        self.short_window.is_some()
            || self.weekly_window.is_some()
            || self
                .extra_pools
                .iter()
                .any(|pool| pool.short_window.is_some() || pool.weekly_window.is_some())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WidgetPreferences {
    #[serde(default)]
    pub locked: bool,
    #[serde(default = "default_always_on_top")]
    pub always_on_top: bool,
    #[serde(default)]
    pub pinned_provider: Option<String>,
    #[serde(default = "default_auto_rotate_seconds")]
    pub auto_rotate_seconds: u64,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_tray_provider")]
    pub tray_provider: String,
    #[serde(default = "default_show_provider")]
    pub show_codex: bool,
    #[serde(default = "default_show_provider")]
    pub show_antigravity: bool,
}

fn default_always_on_top() -> bool {
    true
}

fn default_auto_rotate_seconds() -> u64 {
    12
}

fn default_language() -> String {
    "zh-CN".into()
}

fn default_tray_provider() -> String {
    "codex".into()
}

fn default_show_provider() -> bool {
    true
}

pub fn is_codex_tray_provider(value: &str) -> bool {
    matches!(value, "codex" | "codex_weekly")
}

pub fn is_antigravity_tray_provider(value: &str) -> bool {
    matches!(
        value,
        "gemini"
            | "gemini_weekly"
            | "antigravity_3p"
            | "antigravity_3p_weekly"
            | "gemini_argon_weekly"
            | "gemini_argon"
    )
}

pub fn normalize_tray_provider(value: &str) -> String {
    match value {
        "codex"
        | "codex_weekly"
        | "gemini"
        | "gemini_weekly"
        | "antigravity_3p"
        | "antigravity_3p_weekly"
        | "gemini_argon_weekly" => value.into(),
        "gemini_argon" => "gemini_argon_weekly".into(),
        _ => default_tray_provider(),
    }
}

impl Default for WidgetPreferences {
    fn default() -> Self {
        Self {
            locked: false,
            always_on_top: true,
            pinned_provider: None,
            auto_rotate_seconds: default_auto_rotate_seconds(),
            language: default_language(),
            tray_provider: default_tray_provider(),
            show_codex: default_show_provider(),
            show_antigravity: default_show_provider(),
        }
    }
}

impl WidgetPreferences {
    pub fn normalized(mut self) -> Self {
        self.auto_rotate_seconds = self.auto_rotate_seconds.clamp(5, 300);
        if !self.show_codex && !self.show_antigravity {
            self.show_codex = true;
            self.show_antigravity = true;
        }
        if !matches!(self.pinned_provider.as_deref(), Some("codex" | "gemini")) {
            self.pinned_provider = None;
        }
        if (!self.show_codex && self.pinned_provider.as_deref() == Some("codex"))
            || (!self.show_antigravity && self.pinned_provider.as_deref() == Some("gemini"))
        {
            self.pinned_provider = None;
        }
        if self.language != "en" && self.language != "zh-CN" {
            self.language = default_language();
        }
        self.tray_provider = normalize_tray_provider(&self.tray_provider);
        if !self.show_codex && is_codex_tray_provider(&self.tray_provider) {
            self.tray_provider = "gemini".into();
        } else if !self.show_antigravity && is_antigravity_tray_provider(&self.tray_provider) {
            self.tray_provider = "codex".into();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_legacy_preferences_with_both_providers_enabled() {
        let legacy = r#"{"locked":false,"alwaysOnTop":true,"language":"en","trayProvider":"gemini"}"#;
        let prefs: WidgetPreferences = serde_json::from_str(legacy).unwrap();
        let normalized = prefs.normalized();
        assert!(normalized.show_codex);
        assert!(normalized.show_antigravity);
        assert_eq!(normalized.tray_provider, "gemini");
    }

    #[test]
    fn guards_against_disabling_all_providers_and_redirects_tray_source() {
        let both_off = WidgetPreferences {
            show_codex: false,
            show_antigravity: false,
            ..WidgetPreferences::default()
        }
        .normalized();
        assert!(both_off.show_codex);
        assert!(both_off.show_antigravity);

        let codex_only = WidgetPreferences {
            show_codex: true,
            show_antigravity: false,
            tray_provider: "gemini_argon_weekly".into(),
            pinned_provider: Some("gemini".into()),
            ..WidgetPreferences::default()
        }
        .normalized();
        assert_eq!(codex_only.tray_provider, "codex");
        assert_eq!(codex_only.pinned_provider, None);

        let antigravity_only = WidgetPreferences {
            show_codex: false,
            show_antigravity: true,
            tray_provider: "codex_weekly".into(),
            pinned_provider: Some("codex".into()),
            ..WidgetPreferences::default()
        }
        .normalized();
        assert_eq!(antigravity_only.tray_provider, "gemini");
        assert_eq!(antigravity_only.pinned_provider, None);
    }
}
