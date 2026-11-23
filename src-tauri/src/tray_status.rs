use chrono::{DateTime, Utc};

use crate::models::{ProviderSnapshot, UsageWindow, WidgetPreferences};

pub const HEALTHY_COLOR: [u8; 4] = [0x39, 0x7A, 0xE0, 0xFF];
pub const CAUTION_COLOR: [u8; 4] = [0xD9, 0x9A, 0x2B, 0xFF];
pub const CRITICAL_COLOR: [u8; 4] = [0xE8, 0x5B, 0x4A, 0xFF];
pub const UNKNOWN_COLOR: [u8; 4] = [0x6B, 0x72, 0x80, 0xFF];

const CODEX_START: [u8; 3] = [0x8E, 0xA5, 0xD5];
const CODEX_END: [u8; 3] = [0xFF, 0x7D, 0x62];
const GEMINI_START: [u8; 3] = [0x4B, 0x7B, 0xF5];
const GEMINI_END: [u8; 3] = [0x8B, 0x5C, 0xF6];
const THIRD_PARTY_START: [u8; 3] = [0x6E, 0x67, 0xE8];
const THIRD_PARTY_END: [u8; 3] = [0xE8, 0x8D, 0x5A];
const ARGON_START: [u8; 3] = [0x26, 0xB5, 0xC8];
const ARGON_END: [u8; 3] = [0x7C, 0x5C, 0xF6];

const STALE_MAX_AGE_SECONDS: i64 = 30 * 60;
const MAX_TOOLTIP_CHARS: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayBrand {
    Codex,
    Gemini,
    Antigravity3p,
    GeminiArgon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayWindowKind {
    Short,
    Weekly,
}

impl TrayWindowKind {
    fn label(self, language: &str) -> &'static str {
        match (self, language) {
            (Self::Short, _) => "5h",
            (Self::Weekly, "en") => "Weekly",
            (Self::Weekly, _) => "周额度",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrayVisual {
    pub label: String,
    pub background: [u8; 4],
    pub brand: TrayBrand,
    pub tooltip: String,
}

pub fn clamp_percent(value: f64) -> u8 {
    if !value.is_finite() {
        return 0;
    }
    value.round().clamp(0.0, 100.0) as u8
}

pub fn merge_snapshots(
    current: &[ProviderSnapshot],
    incoming: Vec<ProviderSnapshot>,
) -> Vec<ProviderSnapshot> {
    incoming
        .into_iter()
        .map(|next| {
            if next.status == "ok" || next.status == "signed_out" {
                return next;
            }
            current
                .iter()
                .find(|item| item.provider == next.provider && item.has_any_window())
                .map(|previous| {
                    let mut stale = previous.clone();
                    stale.status = "stale".into();
                    stale.message = next.message.clone();
                    stale
                })
                .unwrap_or(next)
        })
        .collect()
}

fn snapshot_can_show_percent(snapshot: &ProviderSnapshot, now: DateTime<Utc>) -> bool {
    if snapshot.status == "ok" {
        return true;
    }
    if snapshot.status != "stale" {
        return false;
    }
    DateTime::parse_from_rfc3339(&snapshot.updated_at)
        .map(|updated| {
            now.signed_duration_since(updated.with_timezone(&Utc))
                .num_seconds()
        })
        .map(|age| (0..=STALE_MAX_AGE_SECONDS).contains(&age))
        .unwrap_or(false)
}

fn window_percent_for(
    snapshots: &[ProviderSnapshot],
    provider: &str,
    pool_id: Option<&str>,
    kind: TrayWindowKind,
    now: DateTime<Utc>,
) -> Option<u8> {
    let snapshot = snapshots.iter().find(|item| item.provider == provider)?;
    if !snapshot_can_show_percent(snapshot, now) {
        return None;
    }
    let window = match (pool_id, kind) {
        (Some(id), TrayWindowKind::Short) => snapshot
            .extra_pools
            .iter()
            .find(|pool| pool.id == id)
            .and_then(|pool| pool.short_window.as_ref()),
        (Some(id), TrayWindowKind::Weekly) => snapshot
            .extra_pools
            .iter()
            .find(|pool| pool.id == id)
            .and_then(|pool| pool.weekly_window.as_ref()),
        (None, TrayWindowKind::Short) => snapshot.short_window.as_ref(),
        (None, TrayWindowKind::Weekly) => snapshot.weekly_window.as_ref(),
    }?;
    Some(clamp_percent(window.remaining_percent))
}

fn snapshot_usable(snapshot: &ProviderSnapshot, now: DateTime<Utc>, only_on_ok: bool) -> bool {
    if only_on_ok {
        snapshot.status == "ok" && snapshot.has_any_window()
    } else {
        snapshot_can_show_percent(snapshot, now)
    }
}

pub fn resolve_tray_provider(
    snapshots: &[ProviderSnapshot],
    preferences: &WidgetPreferences,
    now: DateTime<Utc>,
    only_on_ok: bool,
) -> String {
    let prefs = preferences.clone().normalized();
    let codex = snapshots
        .iter()
        .find(|item| item.provider == "codex")
        .filter(|item| snapshot_usable(item, now, only_on_ok));
    let gemini = snapshots
        .iter()
        .find(|item| item.provider == "gemini")
        .filter(|item| snapshot_usable(item, now, only_on_ok));

    let gemini_default = |snapshot: &ProviderSnapshot| -> String {
        if snapshot.short_window.is_some() {
            "gemini".into()
        } else if snapshot.weekly_window.is_some() {
            "gemini_weekly".into()
        } else {
            let pool_3p = snapshot
                .extra_pools
                .iter()
                .find(|pool| pool.id == "antigravity_3p");
            if pool_3p.and_then(|pool| pool.short_window.as_ref()).is_some() {
                "antigravity_3p".into()
            } else if pool_3p.and_then(|pool| pool.weekly_window.as_ref()).is_some() {
                "antigravity_3p_weekly".into()
            } else if snapshot
                .extra_pools
                .iter()
                .any(|pool| pool.id == "gemini_argon" && pool.weekly_window.is_some())
            {
                "gemini_argon_weekly".into()
            } else {
                "gemini".into()
            }
        }
    };

    match prefs.tray_provider.as_str() {
        "codex" => {
            if let Some(snapshot) = codex {
                if snapshot.short_window.is_none() && snapshot.weekly_window.is_some() {
                    return "codex_weekly".into();
                }
            }
            "codex".into()
        }
        "codex_weekly" => {
            if let Some(snapshot) = codex {
                if snapshot.weekly_window.is_none() && snapshot.short_window.is_some() {
                    return "codex".into();
                }
            }
            "codex_weekly".into()
        }
        "gemini" => {
            if let Some(snapshot) = gemini {
                if snapshot.short_window.is_none() && snapshot.weekly_window.is_some() {
                    return "gemini_weekly".into();
                }
            }
            "gemini".into()
        }
        "gemini_weekly" => {
            if let Some(snapshot) = gemini {
                if snapshot.weekly_window.is_none() && snapshot.short_window.is_some() {
                    return "gemini".into();
                }
            }
            "gemini_weekly".into()
        }
        "antigravity_3p" => {
            if let Some(snapshot) = gemini {
                if let Some(pool) = snapshot
                    .extra_pools
                    .iter()
                    .find(|pool| pool.id == "antigravity_3p")
                {
                    if pool.short_window.is_none() && pool.weekly_window.is_some() {
                        return "antigravity_3p_weekly".into();
                    }
                    if pool.short_window.is_some() {
                        return "antigravity_3p".into();
                    }
                }
                return gemini_default(snapshot);
            }
            "antigravity_3p".into()
        }
        "antigravity_3p_weekly" => {
            if let Some(snapshot) = gemini {
                if let Some(pool) = snapshot
                    .extra_pools
                    .iter()
                    .find(|pool| pool.id == "antigravity_3p")
                {
                    if pool.weekly_window.is_none() && pool.short_window.is_some() {
                        return "antigravity_3p".into();
                    }
                    if pool.weekly_window.is_some() {
                        return "antigravity_3p_weekly".into();
                    }
                }
                return gemini_default(snapshot);
            }
            "antigravity_3p_weekly".into()
        }
        "gemini_argon_weekly" | "gemini_argon" => {
            if let Some(snapshot) = gemini {
                if snapshot
                    .extra_pools
                    .iter()
                    .any(|pool| pool.id == "gemini_argon" && pool.weekly_window.is_some())
                {
                    return "gemini_argon_weekly".into();
                }
                return gemini_default(snapshot);
            }
            "gemini_argon_weekly".into()
        }
        other => other.into(),
    }
}

fn summary_line(
    snapshots: &[ProviderSnapshot],
    preferences: &WidgetPreferences,
    now: DateTime<Utc>,
) -> String {
    let mut parts = Vec::new();
    if preferences.show_codex {
        if let Some(pct) = window_percent_for(snapshots, "codex", None, TrayWindowKind::Short, now)
        {
            parts.push(format!("GPT {pct}%"));
        } else if let Some(pct) =
            window_percent_for(snapshots, "codex", None, TrayWindowKind::Weekly, now)
        {
            parts.push(format!("GPT·W {pct}%"));
        }
    }
    if preferences.show_antigravity {
        if let Some(pct) = window_percent_for(snapshots, "gemini", None, TrayWindowKind::Short, now)
        {
            parts.push(format!("Gemini {pct}%"));
        } else if let Some(pct) =
            window_percent_for(snapshots, "gemini", None, TrayWindowKind::Weekly, now)
        {
            parts.push(format!("Gemini·W {pct}%"));
        }
        if let Some(pct) = window_percent_for(
            snapshots,
            "gemini",
            Some("antigravity_3p"),
            TrayWindowKind::Short,
            now,
        ) {
            parts.push(format!("3P {pct}%"));
        } else if let Some(pct) = window_percent_for(
            snapshots,
            "gemini",
            Some("antigravity_3p"),
            TrayWindowKind::Weekly,
            now,
        ) {
            parts.push(format!("3P·W {pct}%"));
        }
        if let Some(pct) = window_percent_for(
            snapshots,
            "gemini",
            Some("gemini_argon"),
            TrayWindowKind::Weekly,
            now,
        ) {
            parts.push(format!("Argon {pct}%"));
        }
    }
    parts.join(" | ")
}

fn truncate_tooltip(text: String) -> String {
    if text.chars().count() <= MAX_TOOLTIP_CHARS {
        return text;
    }
    let mut out: String = text.chars().take(MAX_TOOLTIP_CHARS.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn append_summary(
    primary: String,
    snapshots: &[ProviderSnapshot],
    preferences: &WidgetPreferences,
    now: DateTime<Utc>,
) -> String {
    let summary = summary_line(snapshots, preferences, now);
    if summary.is_empty() {
        return truncate_tooltip(primary);
    }
    let combined = format!("{primary}\n{summary}");
    if combined.chars().count() <= MAX_TOOLTIP_CHARS {
        combined
    } else {
        truncate_tooltip(primary)
    }
}

struct TrayTarget<'a> {
    brand: TrayBrand,
    label: &'static str,
    login_app: &'static str,
    window_kind: TrayWindowKind,
    snapshot: Option<&'a ProviderSnapshot>,
    window: Option<&'a UsageWindow>,
}

fn select_target<'a>(snapshots: &'a [ProviderSnapshot], tray_provider: &str) -> TrayTarget<'a> {
    let codex = snapshots.iter().find(|item| item.provider == "codex");
    let gemini = snapshots.iter().find(|item| item.provider == "gemini");
    match tray_provider {
        "codex_weekly" => TrayTarget {
            brand: TrayBrand::Codex,
            label: "Codex",
            login_app: "Codex",
            window_kind: TrayWindowKind::Weekly,
            snapshot: codex,
            window: codex.and_then(|item| item.weekly_window.as_ref()),
        },
        "gemini" => TrayTarget {
            brand: TrayBrand::Gemini,
            label: "Gemini",
            login_app: "Antigravity",
            window_kind: TrayWindowKind::Short,
            snapshot: gemini,
            window: gemini.and_then(|item| item.short_window.as_ref()),
        },
        "gemini_weekly" => TrayTarget {
            brand: TrayBrand::Gemini,
            label: "Gemini",
            login_app: "Antigravity",
            window_kind: TrayWindowKind::Weekly,
            snapshot: gemini,
            window: gemini.and_then(|item| item.weekly_window.as_ref()),
        },
        "antigravity_3p" => TrayTarget {
            brand: TrayBrand::Antigravity3p,
            label: "Claude & GPT",
            login_app: "Antigravity",
            window_kind: TrayWindowKind::Short,
            snapshot: gemini,
            window: gemini.and_then(|item| {
                item.extra_pools
                    .iter()
                    .find(|pool| pool.id == "antigravity_3p")
                    .and_then(|pool| pool.short_window.as_ref())
            }),
        },
        "antigravity_3p_weekly" => TrayTarget {
            brand: TrayBrand::Antigravity3p,
            label: "Claude & GPT",
            login_app: "Antigravity",
            window_kind: TrayWindowKind::Weekly,
            snapshot: gemini,
            window: gemini.and_then(|item| {
                item.extra_pools
                    .iter()
                    .find(|pool| pool.id == "antigravity_3p")
                    .and_then(|pool| pool.weekly_window.as_ref())
            }),
        },
        "gemini_argon_weekly" | "gemini_argon" => TrayTarget {
            brand: TrayBrand::GeminiArgon,
            label: "Gemini 4 Argon",
            login_app: "Antigravity",
            window_kind: TrayWindowKind::Weekly,
            snapshot: gemini,
            window: gemini.and_then(|item| {
                item.extra_pools
                    .iter()
                    .find(|pool| pool.id == "gemini_argon")
                    .and_then(|pool| pool.weekly_window.as_ref())
            }),
        },
        _ => TrayTarget {
            brand: TrayBrand::Codex,
            label: "Codex",
            login_app: "Codex",
            window_kind: TrayWindowKind::Short,
            snapshot: codex,
            window: codex.and_then(|item| item.short_window.as_ref()),
        },
    }
}

pub fn visual_for_snapshots(
    snapshots: &[ProviderSnapshot],
    preferences: &WidgetPreferences,
    now: DateTime<Utc>,
) -> TrayVisual {
    let normalized = preferences.clone().normalized();
    let language = normalized.language.as_str();
    let effective_provider = resolve_tray_provider(snapshots, &normalized, now, false);
    let target = select_target(snapshots, &effective_provider);
    let brand = target.brand;
    let target_label = target.label;
    let login_app = target.login_app;
    let window_label = target.window_kind.label(language);

    let Some(snapshot) = target.snapshot else {
        return unknown_visual(language, brand, false, snapshots, &normalized, now);
    };

    if snapshot.status == "signed_out" {
        let primary = if language == "en" {
            format!("QuotaPulse · Sign in to {login_app}")
        } else {
            format!("QuotaPulse · 请登录 {login_app}")
        };
        return TrayVisual {
            label: "!".into(),
            background: UNKNOWN_COLOR,
            brand,
            tooltip: append_summary(primary, snapshots, &normalized, now),
        };
    }

    let can_show = snapshot_can_show_percent(snapshot, now);
    let Some(window) = target.window.filter(|_| can_show) else {
        return missing_window_visual(
            language,
            brand,
            target_label,
            window_label,
            snapshot.status == "stale",
            snapshots,
            &normalized,
            now,
        );
    };

    let percent = clamp_percent(window.remaining_percent);
    let background = if snapshot.status == "stale" {
        UNKNOWN_COLOR
    } else if percent >= 50 {
        HEALTHY_COLOR
    } else if percent >= 10 {
        CAUTION_COLOR
    } else {
        CRITICAL_COLOR
    };
    let reset = format_reset(window.resets_at.as_deref(), now, language);
    let primary = if language == "en" {
        format!(
            "{target_label} {window_label}: {percent}% left{reset}{}",
            if snapshot.status == "stale" { " · stale" } else { "" }
        )
    } else {
        format!(
            "{target_label} {window_label} 剩余 {percent}%{reset}{}",
            if snapshot.status == "stale" { " · 已过期" } else { "" }
        )
    };
    TrayVisual {
        label: percent.to_string(),
        background,
        brand,
        tooltip: append_summary(primary, snapshots, &normalized, now),
    }
}

fn missing_window_visual(
    language: &str,
    brand: TrayBrand,
    target_label: &str,
    window_label: &str,
    stale: bool,
    snapshots: &[ProviderSnapshot],
    preferences: &WidgetPreferences,
    now: DateTime<Utc>,
) -> TrayVisual {
    let primary = if language == "en" {
        if stale {
            format!("QuotaPulse · {target_label} {window_label} data expired")
        } else {
            format!("QuotaPulse · {target_label} {window_label} unavailable")
        }
    } else if stale {
        format!("QuotaPulse · {target_label} {window_label} 数据已过期")
    } else {
        format!("QuotaPulse · {target_label} {window_label} 暂不可用")
    };
    TrayVisual {
        label: String::new(),
        background: UNKNOWN_COLOR,
        brand,
        tooltip: append_summary(primary, snapshots, preferences, now),
    }
}

fn unknown_visual(
    language: &str,
    brand: TrayBrand,
    stale: bool,
    snapshots: &[ProviderSnapshot],
    preferences: &WidgetPreferences,
    now: DateTime<Utc>,
) -> TrayVisual {
    let primary = if language == "en" {
        if stale {
            "QuotaPulse · quota data expired".into()
        } else {
            "QuotaPulse · quota unavailable".into()
        }
    } else if stale {
        "QuotaPulse · 额度数据已过期".into()
    } else {
        "QuotaPulse · 额度暂不可用".into()
    };
    TrayVisual {
        label: String::new(),
        background: UNKNOWN_COLOR,
        brand,
        tooltip: append_summary(primary, snapshots, preferences, now),
    }
}

fn format_reset(value: Option<&str>, now: DateTime<Utc>, language: &str) -> String {
    let Some(target) = value.and_then(|value| DateTime::parse_from_rfc3339(value).ok()) else {
        return String::new();
    };
    let minutes = target
        .with_timezone(&Utc)
        .signed_duration_since(now)
        .num_minutes();
    if minutes <= 0 {
        return if language == "en" {
            " · updating".into()
        } else {
            " · 重置更新中".into()
        };
    }
    let text = if minutes < 60 {
        if language == "en" {
            format!("{minutes}m")
        } else {
            format!("{minutes} 分钟")
        }
    } else if minutes < 24 * 60 {
        let hours = minutes / 60;
        let rest = minutes % 60;
        if language == "en" {
            format!("{hours}h {rest}m")
        } else {
            format!("{hours} 小时 {rest} 分钟")
        }
    } else {
        let days = minutes / (24 * 60);
        let hours = (minutes / 60) % 24;
        if language == "en" {
            format!("{days}d {hours}h")
        } else {
            format!("{days} 天 {hours} 小时")
        }
    };
    if language == "en" {
        format!(" · {text}")
    } else {
        format!(" · {text}后重置")
    }
}

pub fn render_icon(visual: &TrayVisual) -> Vec<u8> {
    const WIDTH: usize = 32;
    const HEIGHT: usize = 32;
    let mut rgba = vec![0; WIDTH * HEIGHT * 4];

    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            if !inside_icon(x as i32, y as i32) {
                continue;
            }
            let border = !inside_icon(x as i32 - 2, y as i32)
                || !inside_icon(x as i32 + 2, y as i32)
                || !inside_icon(x as i32, y as i32 - 2)
                || !inside_icon(x as i32, y as i32 + 2);
            set_pixel(
                &mut rgba,
                x,
                y,
                if border {
                    visual.background
                } else {
                    brand_gradient(visual.brand, x, y)
                },
            );
        }
    }

    if visual.label.is_empty() {
        draw_brand_mark(&mut rgba);
        return rgba;
    }

    let glyphs: Vec<[u8; 5]> = visual.label.chars().map(glyph).collect();
    let scale = match glyphs.len() {
        0 | 1 => 5,
        2 => 4,
        _ => 3,
    };
    let glyph_width = 3 * scale;
    let gap = if glyphs.len() >= 3 { 2 } else { 3 };
    let total_width = glyphs.len() * glyph_width + glyphs.len().saturating_sub(1) * gap;
    let start_x = (WIDTH.saturating_sub(total_width)) / 2;
    let start_y = (HEIGHT - 5 * scale) / 2;
    for (index, rows) in glyphs.iter().enumerate() {
        let x = start_x + index * (glyph_width + gap);
        draw_glyph(
            &mut rgba,
            x + 1,
            start_y + 1,
            scale,
            rows,
            [0x2A, 0x2C, 0x42, 0x72],
        );
        draw_glyph(&mut rgba, x, start_y, scale, rows, [0xFF; 4]);
    }
    rgba
}

fn inside_icon(x: i32, y: i32) -> bool {
    if !(1..=30).contains(&x) || !(1..=30).contains(&y) {
        return false;
    }
    let nearest_x = x.clamp(7, 24);
    let nearest_y = y.clamp(7, 24);
    let dx = x - nearest_x;
    let dy = y - nearest_y;
    dx * dx + dy * dy <= 36
}

fn brand_gradient(brand: TrayBrand, x: usize, y: usize) -> [u8; 4] {
    let (start, end) = match brand {
        TrayBrand::Codex => (CODEX_START, CODEX_END),
        TrayBrand::Gemini => (GEMINI_START, GEMINI_END),
        TrayBrand::Antigravity3p => (THIRD_PARTY_START, THIRD_PARTY_END),
        TrayBrand::GeminiArgon => (ARGON_START, ARGON_END),
    };
    let diagonal = ((x + y).saturating_sub(2) as f32 / 58.0).clamp(0.0, 1.0);
    let center_mix = (1.0 - (diagonal - 0.5).abs() * 2.0) * 0.18;
    let mut color = [0u8; 4];
    for channel in 0..3 {
        let base = start[channel] as f32 + (end[channel] as f32 - start[channel] as f32) * diagonal;
        color[channel] = (base + (0xE7 as f32 - base) * center_mix).round() as u8;
    }
    color[3] = 0xFF;
    color
}

fn dist_sq_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let abx = bx - ax;
    let aby = by - ay;
    let len_sq = abx * abx + aby * aby;
    let t = if len_sq <= f32::EPSILON {
        0.0
    } else {
        (((px - ax) * abx + (py - ay) * aby) / len_sq).clamp(0.0, 1.0)
    };
    let cx = ax + t * abx;
    let cy = ay + t * aby;
    let dx = px - cx;
    let dy = py - cy;
    dx * dx + dy * dy
}

fn draw_brand_mark(rgba: &mut [u8]) {
    const POINTS: [(f32, f32); 6] = [
        (6.0, 16.0),
        (11.0, 16.0),
        (14.0, 9.0),
        (18.0, 23.0),
        (21.0, 16.0),
        (26.0, 16.0),
    ];
    const MAX_DIST_SQ: f32 = 1.15 * 1.15;
    let mut mask = [[false; 32]; 32];
    for (y, row) in mask.iter_mut().enumerate() {
        for (x, cell) in row.iter_mut().enumerate() {
            let px = x as f32;
            let py = y as f32;
            *cell = POINTS
                .windows(2)
                .any(|seg| dist_sq_to_segment(px, py, seg[0].0, seg[0].1, seg[1].0, seg[1].1) <= MAX_DIST_SQ);
        }
    }

    let shadow = [0x2A, 0x2C, 0x42, 0x60];
    for (y, row) in mask.iter().enumerate() {
        for (x, &on) in row.iter().enumerate() {
            if on {
                blend_pixel(rgba, x + 1, y + 1, shadow);
            }
        }
    }
    for (y, row) in mask.iter().enumerate() {
        for (x, &on) in row.iter().enumerate() {
            if on {
                blend_pixel(rgba, x, y, [0xFF; 4]);
            }
        }
    }
}

fn glyph(value: char) -> [u8; 5] {
    match value {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '!' => [0b010, 0b010, 0b010, 0b000, 0b010],
        _ => [0b111, 0b001, 0b011, 0b000, 0b010],
    }
}

fn draw_glyph(rgba: &mut [u8], x: usize, y: usize, scale: usize, rows: &[u8; 5], color: [u8; 4]) {
    for (row, bits) in rows.iter().enumerate() {
        for column in 0..3 {
            if bits & (1 << (2 - column)) == 0 {
                continue;
            }
            for dy in 0..scale {
                for dx in 0..scale {
                    blend_pixel(rgba, x + column * scale + dx, y + row * scale + dy, color);
                }
            }
        }
    }
}

fn blend_pixel(rgba: &mut [u8], x: usize, y: usize, color: [u8; 4]) {
    if x >= 32 || y >= 32 || !inside_icon(x as i32, y as i32) {
        return;
    }
    let index = (y * 32 + x) * 4;
    let alpha = color[3] as f32 / 255.0;
    for channel in 0..3 {
        rgba[index + channel] = (rgba[index + channel] as f32 * (1.0 - alpha)
            + color[channel] as f32 * alpha)
            .round() as u8;
    }
    rgba[index + 3] = 0xFF;
}

fn set_pixel(rgba: &mut [u8], x: usize, y: usize, color: [u8; 4]) {
    if x >= 32 || y >= 32 {
        return;
    }
    let index = (y * 32 + x) * 4;
    rgba[index..index + 4].copy_from_slice(&color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{QuotaPool, UsageWindow};

    fn codex_snapshot(status: &str, percent: f64, updated_at: &str) -> ProviderSnapshot {
        ProviderSnapshot {
            provider: "codex".into(),
            display_name: "CODEX".into(),
            plan: Some("PRO".into()),
            short_window: Some(UsageWindow {
                remaining_percent: percent,
                resets_at: Some("2026-08-09T14:00:00Z".into()),
                window_seconds: 18_000,
            }),
            weekly_window: Some(UsageWindow {
                remaining_percent: 73.0,
                resets_at: Some("2026-08-10T12:00:00Z".into()),
                window_seconds: 604_800,
            }),
            extra_pools: Vec::new(),
            reset_credits: Some(1),
            reset_credit_expires_at: Vec::new(),
            updated_at: updated_at.into(),
            status: status.into(),
            message: None,
        }
    }

    fn gemini_snapshot(status: &str, gemini_pct: f64, tp_pct: f64, updated_at: &str) -> ProviderSnapshot {
        ProviderSnapshot {
            provider: "gemini".into(),
            display_name: "GEMINI".into(),
            plan: Some("PRO".into()),
            short_window: Some(UsageWindow {
                remaining_percent: gemini_pct,
                resets_at: Some("2026-08-09T15:03:00Z".into()),
                window_seconds: 18_000,
            }),
            weekly_window: Some(UsageWindow {
                remaining_percent: 97.0,
                resets_at: Some("2026-08-16T10:00:00Z".into()),
                window_seconds: 604_800,
            }),
            extra_pools: vec![
                QuotaPool {
                    id: "antigravity_3p".into(),
                    display_name: "Claude & GPT".into(),
                    short_window: Some(UsageWindow {
                        remaining_percent: tp_pct,
                        resets_at: Some("2026-08-09T15:52:00Z".into()),
                        window_seconds: 18_000,
                    }),
                    weekly_window: Some(UsageWindow {
                        remaining_percent: 44.0,
                        resets_at: Some("2026-08-16T10:00:00Z".into()),
                        window_seconds: 604_800,
                    }),
                },
                QuotaPool {
                    id: "gemini_argon".into(),
                    display_name: "Gemini 4 Argon".into(),
                    short_window: None,
                    weekly_window: Some(UsageWindow {
                        remaining_percent: 100.0,
                        resets_at: None,
                        window_seconds: 604_800,
                    }),
                },
            ],
            reset_credits: None,
            reset_credit_expires_at: Vec::new(),
            updated_at: updated_at.into(),
            status: status.into(),
            message: None,
        }
    }

    #[test]
    fn clamps_and_rounds_percentages() {
        assert_eq!(clamp_percent(-1.0), 0);
        assert_eq!(clamp_percent(49.6), 50);
        assert_eq!(clamp_percent(101.0), 100);
    }

    #[test]
    fn switches_tray_between_codex_gemini_and_third_party_pools() {
        let now = DateTime::parse_from_rfc3339("2026-08-09T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let snapshots = vec![
            codex_snapshot("ok", 74.0, "2026-08-09T12:00:00Z"),
            gemini_snapshot("ok", 93.0, 0.0, "2026-08-09T12:00:00Z"),
        ];

        let mut prefs = WidgetPreferences::default();
        let codex_visual = visual_for_snapshots(&snapshots, &prefs, now);
        assert_eq!(codex_visual.label, "74");
        assert_eq!(codex_visual.brand, TrayBrand::Codex);
        assert_eq!(codex_visual.background, HEALTHY_COLOR);
        assert!(codex_visual.tooltip.contains("GPT 74% | Gemini 93% | 3P 0% | Argon 100%"));

        prefs.tray_provider = "codex_weekly".into();
        let codex_weekly = visual_for_snapshots(&snapshots, &prefs, now);
        assert_eq!(codex_weekly.label, "73");
        assert_eq!(codex_weekly.brand, TrayBrand::Codex);
        assert!(codex_weekly.tooltip.contains("Codex 周额度 剩余 73%"));

        prefs.tray_provider = "gemini".into();
        let gemini_visual = visual_for_snapshots(&snapshots, &prefs, now);
        assert_eq!(gemini_visual.label, "93");
        assert_eq!(gemini_visual.brand, TrayBrand::Gemini);
        assert_eq!(gemini_visual.background, HEALTHY_COLOR);

        prefs.tray_provider = "gemini_weekly".into();
        let gemini_weekly = visual_for_snapshots(&snapshots, &prefs, now);
        assert_eq!(gemini_weekly.label, "97");
        assert_eq!(gemini_weekly.brand, TrayBrand::Gemini);
        assert!(gemini_weekly.tooltip.contains("Gemini 周额度 剩余 97%"));

        prefs.tray_provider = "antigravity_3p".into();
        let tp_visual = visual_for_snapshots(&snapshots, &prefs, now);
        assert_eq!(tp_visual.label, "0");
        assert_eq!(tp_visual.brand, TrayBrand::Antigravity3p);
        assert_eq!(tp_visual.background, CRITICAL_COLOR);

        prefs.tray_provider = "antigravity_3p_weekly".into();
        let tp_weekly = visual_for_snapshots(&snapshots, &prefs, now);
        assert_eq!(tp_weekly.label, "44");
        assert_eq!(tp_weekly.brand, TrayBrand::Antigravity3p);
        assert_eq!(tp_weekly.background, CAUTION_COLOR);
        assert!(tp_weekly.tooltip.contains("Claude & GPT 周额度 剩余 44%"));

        prefs.tray_provider = "gemini_argon_weekly".into();
        let argon_weekly = visual_for_snapshots(&snapshots, &prefs, now);
        assert_eq!(argon_weekly.label, "100");
        assert_eq!(argon_weekly.brand, TrayBrand::GeminiArgon);
        assert_eq!(argon_weekly.background, HEALTHY_COLOR);
        assert!(argon_weekly.tooltip.contains("Gemini 4 Argon 周额度 剩余 100%"));
    }

    #[test]
    fn uses_inclusive_color_boundaries() {
        let now = DateTime::parse_from_rfc3339("2026-08-09T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let prefs = WidgetPreferences::default();
        for (pct, expected) in [
            (50.0, HEALTHY_COLOR),
            (49.0, CAUTION_COLOR),
            (10.0, CAUTION_COLOR),
            (9.0, CRITICAL_COLOR),
        ] {
            let visual = visual_for_snapshots(
                &[codex_snapshot("ok", pct, "2026-08-09T12:00:00Z")],
                &prefs,
                now,
            );
            assert_eq!(visual.background, expected);
        }
    }

    #[test]
    fn keeps_recent_failures_stale_per_provider_but_not_signed_out() {
        let codex_ok = codex_snapshot("ok", 42.0, "2026-08-09T12:00:00Z");
        let gemini_ok = gemini_snapshot("ok", 90.0, 20.0, "2026-08-09T12:00:00Z");
        let codex_fail = ProviderSnapshot::failure_for("codex", "CODEX", "unavailable", "Net err");
        let gemini_signed_out =
            ProviderSnapshot::failure_for("gemini", "GEMINI", "signed_out", "Sign in");

        let merged = merge_snapshots(&[codex_ok, gemini_ok], vec![codex_fail, gemini_signed_out]);
        assert_eq!(merged[0].status, "stale");
        assert_eq!(merged[0].short_window.as_ref().unwrap().remaining_percent, 42.0);
        assert_eq!(merged[1].status, "signed_out");
    }

    #[test]
    fn falls_back_to_weekly_and_available_pools_for_pro_and_ultra() {
        let now = DateTime::parse_from_rfc3339("2026-08-09T13:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut codex_pro = codex_snapshot("ok", 0.0, "2026-08-09T13:00:00Z");
        codex_pro.plan = Some("PRO".into());
        codex_pro.short_window = None;
        codex_pro.weekly_window = Some(UsageWindow {
            remaining_percent: 61.0,
            resets_at: Some("2026-08-14T12:00:00Z".into()),
            window_seconds: 604_800,
        });

        let mut gemini_ultra = gemini_snapshot("ok", 0.0, 0.0, "2026-08-09T13:00:00Z");
        gemini_ultra.plan = Some("ULTRA".into());
        gemini_ultra.short_window = None;
        gemini_ultra.weekly_window = Some(UsageWindow {
            remaining_percent: 88.0,
            resets_at: Some("2026-08-16T10:00:00Z".into()),
            window_seconds: 604_800,
        });
        gemini_ultra.extra_pools = vec![QuotaPool {
            id: "antigravity_3p".into(),
            display_name: "Claude & GPT".into(),
            short_window: None,
            weekly_window: Some(UsageWindow {
                remaining_percent: 54.0,
                resets_at: Some("2026-08-16T10:00:00Z".into()),
                window_seconds: 604_800,
            }),
        }];

        let snapshots = vec![codex_pro, gemini_ultra];
        let mut prefs = WidgetPreferences::default();

        let codex_visual = visual_for_snapshots(&snapshots, &prefs, now);
        assert_eq!(codex_visual.label, "61");
        assert_eq!(codex_visual.brand, TrayBrand::Codex);
        assert!(codex_visual.tooltip.contains("Codex 周额度 剩余 61%"));
        assert!(codex_visual
            .tooltip
            .contains("GPT·W 61% | Gemini·W 88% | 3P·W 54%"));

        prefs.tray_provider = "gemini".into();
        let gemini_visual = visual_for_snapshots(&snapshots, &prefs, now);
        assert_eq!(gemini_visual.label, "88");
        assert_eq!(gemini_visual.brand, TrayBrand::Gemini);
        assert!(gemini_visual.tooltip.contains("Gemini 周额度 剩余 88%"));

        prefs.tray_provider = "antigravity_3p".into();
        let tp_visual = visual_for_snapshots(&snapshots, &prefs, now);
        assert_eq!(tp_visual.label, "54");
        assert_eq!(tp_visual.brand, TrayBrand::Antigravity3p);
        assert!(tp_visual.tooltip.contains("Claude & GPT 周额度 剩余 54%"));

        prefs.tray_provider = "gemini_argon_weekly".into();
        let argon_fallback = visual_for_snapshots(&snapshots, &prefs, now);
        assert_eq!(argon_fallback.label, "88");
        assert_eq!(argon_fallback.brand, TrayBrand::Gemini);
        assert_eq!(
            resolve_tray_provider(&snapshots, &prefs, now, true),
            "gemini_weekly"
        );
    }

    #[test]
    fn hides_disabled_provider_from_tray_and_tooltip_summary() {
        let now = DateTime::parse_from_rfc3339("2026-08-09T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let snapshots = vec![
            codex_snapshot("ok", 74.0, "2026-08-09T12:00:00Z"),
            gemini_snapshot("ok", 93.0, 20.0, "2026-08-09T12:00:00Z"),
        ];

        let codex_only = WidgetPreferences {
            show_codex: true,
            show_antigravity: false,
            tray_provider: "gemini".into(),
            ..WidgetPreferences::default()
        };
        let visual = visual_for_snapshots(&snapshots, &codex_only, now);
        assert_eq!(visual.label, "74");
        assert_eq!(visual.brand, TrayBrand::Codex);
        assert!(visual.tooltip.contains("GPT 74%"));
        assert!(!visual.tooltip.contains("Gemini"));
        assert!(!visual.tooltip.contains("3P"));

        let antigravity_only = WidgetPreferences {
            show_codex: false,
            show_antigravity: true,
            tray_provider: "codex".into(),
            ..WidgetPreferences::default()
        };
        let visual = visual_for_snapshots(&snapshots, &antigravity_only, now);
        assert_eq!(visual.label, "93");
        assert_eq!(visual.brand, TrayBrand::Gemini);
        assert!(visual.tooltip.contains("Gemini 93%"));
        assert!(!visual.tooltip.contains("GPT 74%"));
    }

    #[test]
    fn renders_distinct_brand_icons_and_respects_tooltip_limit() {
        let codex_rgba = render_icon(&TrayVisual {
            label: "100".into(),
            background: HEALTHY_COLOR,
            brand: TrayBrand::Codex,
            tooltip: String::new(),
        });
        let gemini_rgba = render_icon(&TrayVisual {
            label: "100".into(),
            background: HEALTHY_COLOR,
            brand: TrayBrand::Gemini,
            tooltip: String::new(),
        });
        assert_eq!(codex_rgba.len(), 32 * 32 * 4);
        assert_ne!(codex_rgba, gemini_rgba);
        assert_eq!(&codex_rgba[0..4], &[0, 0, 0, 0]);

        let fallback_rgba = render_icon(&TrayVisual {
            label: String::new(),
            background: UNKNOWN_COLOR,
            brand: TrayBrand::Gemini,
            tooltip: String::new(),
        });
        assert_eq!(&fallback_rgba[0..4], &[0, 0, 0, 0]);
        let pixel = |x: usize, y: usize| -> &[u8] {
            let idx = (y * 32 + x) * 4;
            &fallback_rgba[idx..idx + 4]
        };
        assert_eq!(pixel(6, 16), &[0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(pixel(16, 16), &[0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(pixel(26, 16), &[0xFF, 0xFF, 0xFF, 0xFF]);
        assert_ne!(pixel(16, 6), &[0xFF, 0xFF, 0xFF, 0xFF]);

        assert!(truncate_tooltip("a".repeat(200)).chars().count() <= MAX_TOOLTIP_CHARS);
    }
}
