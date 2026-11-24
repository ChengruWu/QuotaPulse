mod antigravity;
mod codex;
mod models;
mod tray_status;
mod updates;
mod window_position;

use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

use models::{ProviderSnapshot, WidgetPreferences};
use tauri::{
    image::Image,
    menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Rect, State, WindowEvent, Wry,
};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_window_state::{Builder as WindowStateBuilder, StateFlags};
use tray_status::{merge_snapshots, render_icon, resolve_tray_provider, visual_for_snapshots};
use window_position::{popup_position, Bounds, Point};

const POPUP_WIDTH: u32 = 360;
const POPUP_HEIGHT_DUAL: u32 = 460;
const POPUP_HEIGHT_CODEX_ONLY: u32 = 240;
const POPUP_HEIGHT_ANTIGRAVITY_COMPACT: u32 = 250;
const POPUP_HEIGHT_ANTIGRAVITY_WITH_POOLS: u32 = 316;
const ORB_SIZE: u32 = 100;
const ANTIGRAVITY_POLL_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowPresentation {
    Tray,
    Popup,
    Floating,
}

impl WindowPresentation {
    fn event_name(self) -> &'static str {
        match self {
            Self::Tray => "tray",
            Self::Popup => "popup",
            Self::Floating => "floating",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrayMenuSignature {
    show_codex: bool,
    show_antigravity: bool,
    active_tray: String,
    autostart: bool,
    gemini_signed_out: bool,
    gemini_signing_in: bool,
    codex_has_short: bool,
    codex_has_weekly: bool,
    gemini_has_short: bool,
    gemini_has_weekly: bool,
    has_3p_short: bool,
    has_3p_weekly: bool,
    has_argon_weekly: bool,
}

fn popup_height_for(preferences: &WidgetPreferences, snapshots: &[ProviderSnapshot]) -> u32 {
    let prefs = preferences.clone().normalized();
    if prefs.show_codex && !prefs.show_antigravity {
        return POPUP_HEIGHT_CODEX_ONLY;
    }
    if !prefs.show_codex && prefs.show_antigravity {
        let gemini = snapshots.iter().find(|item| item.provider == "gemini");
        let has_subpools =
            gemini.is_some_and(|item| item.status != "signed_out" && !item.extra_pools.is_empty());
        return if has_subpools {
            POPUP_HEIGHT_ANTIGRAVITY_WITH_POOLS
        } else {
            POPUP_HEIGHT_ANTIGRAVITY_COMPACT
        };
    }
    POPUP_HEIGHT_DUAL
}

fn menu_signature_for(
    preferences: &WidgetPreferences,
    snapshots: &[ProviderSnapshot],
    autostart: bool,
    gemini_signing_in: bool,
) -> TrayMenuSignature {
    let prefs = preferences.clone().normalized();
    let active_tray = resolve_tray_provider(snapshots, &prefs, chrono::Utc::now(), false);
    let gemini_signed_out = prefs.show_antigravity
        && snapshots
            .iter()
            .any(|item| item.provider == "gemini" && item.status == "signed_out");
    let codex = snapshots
        .iter()
        .find(|item| item.provider == "codex" && item.status == "ok" && item.has_any_window());
    let gemini = snapshots
        .iter()
        .find(|item| item.provider == "gemini" && item.status == "ok" && item.has_any_window());

    let (codex_has_short, codex_has_weekly) = match codex {
        Some(snap) => (snap.short_window.is_some(), snap.weekly_window.is_some()),
        None => (true, true),
    };
    let (gemini_has_short, gemini_has_weekly, has_3p_short, has_3p_weekly, has_argon_weekly) =
        match gemini {
            Some(snap) => {
                let pool_3p = snap
                    .extra_pools
                    .iter()
                    .find(|pool| pool.id == "antigravity_3p");
                let has_argon = snap
                    .extra_pools
                    .iter()
                    .any(|pool| pool.id == "gemini_argon" && pool.weekly_window.is_some());
                (
                    snap.short_window.is_some(),
                    snap.weekly_window.is_some(),
                    pool_3p.is_some_and(|pool| pool.short_window.is_some()),
                    pool_3p.is_some_and(|pool| pool.weekly_window.is_some()),
                    has_argon,
                )
            }
            None => (true, true, true, true, true),
        };

    TrayMenuSignature {
        show_codex: prefs.show_codex,
        show_antigravity: prefs.show_antigravity,
        active_tray,
        autostart,
        gemini_signed_out,
        gemini_signing_in,
        codex_has_short,
        codex_has_weekly,
        gemini_has_short,
        gemini_has_weekly,
        has_3p_short,
        has_3p_weekly,
        has_argon_weekly,
    }
}

struct AppState {
    client: Mutex<Option<reqwest::Client>>,
    antigravity_session: Mutex<antigravity::SessionCache>,
    antigravity_login_cancel: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    last_antigravity_fetch: Mutex<Option<Instant>>,
    preferences: Mutex<WidgetPreferences>,
    preferences_path: PathBuf,
    fetch_lock: tokio::sync::Mutex<()>,
    snapshot_cache: Mutex<Option<(Instant, Vec<ProviderSnapshot>)>>,
    presentation: Mutex<WindowPresentation>,
    last_tray_bounds: Mutex<Option<Bounds>>,
    last_floating_position: Mutex<Option<Point>>,
    last_popup_height: Mutex<Option<u32>>,
    tray_menu_signature: Mutex<Option<TrayMenuSignature>>,
}

fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("QuotaPulse/0.2")
        .build()
        .expect("static HTTP client configuration must be valid")
}

fn cache_matches_visibility(
    values: &[ProviderSnapshot],
    show_codex: bool,
    show_antigravity: bool,
) -> bool {
    let has_codex = values.iter().any(|item| item.provider == "codex");
    let has_gemini = values.iter().any(|item| item.provider == "gemini");
    has_codex == show_codex && has_gemini == show_antigravity
}

async fn fetch_and_publish(app: &AppHandle, force: bool, include_antigravity: bool) -> Vec<ProviderSnapshot> {
    const CACHE_TTL: Duration = Duration::from_secs(9);
    let state = app.state::<AppState>();
    let (show_codex, show_antigravity) = state
        .preferences
        .lock()
        .map(|prefs| {
            let normalized = prefs.clone().normalized();
            (normalized.show_codex, normalized.show_antigravity)
        })
        .unwrap_or((true, true));

    if !force {
        if let Ok(cache) = state.snapshot_cache.lock() {
            if let Some((time, values)) = &*cache {
                if time.elapsed() < CACHE_TTL
                    && cache_matches_visibility(values, show_codex, show_antigravity)
                {
                    return values.clone();
                }
            }
        }
    }

    let _guard = state.fetch_lock.lock().await;
    let (show_codex, show_antigravity) = state
        .preferences
        .lock()
        .map(|prefs| {
            let normalized = prefs.clone().normalized();
            (normalized.show_codex, normalized.show_antigravity)
        })
        .unwrap_or((true, true));

    if !force {
        if let Ok(cache) = state.snapshot_cache.lock() {
            if let Some((time, values)) = &*cache {
                if time.elapsed() < CACHE_TTL
                    && cache_matches_visibility(values, show_codex, show_antigravity)
                {
                    return values.clone();
                }
            }
        }
    }

    let client = {
        let mut slot = state.client.lock().unwrap_or_else(|err| err.into_inner());
        slot.get_or_insert_with(build_http_client).clone()
    };

    let cached_gemini = if show_antigravity {
        state
            .snapshot_cache
            .lock()
            .ok()
            .and_then(|cache| {
                cache
                    .as_ref()
                    .and_then(|(_, values)| values.iter().find(|item| item.provider == "gemini").cloned())
            })
    } else {
        None
    };

    let should_fetch_antigravity = show_antigravity
        && (include_antigravity
            || cached_gemini.is_none()
            || state
                .last_antigravity_fetch
                .lock()
                .ok()
                .and_then(|value| *value)
                .is_none_or(|last| last.elapsed() >= ANTIGRAVITY_POLL_INTERVAL));

    let incoming = match (show_codex, show_antigravity) {
        (true, true) => {
            if should_fetch_antigravity {
                let (codex_res, gemini_res) = tokio::join!(
                    codex::fetch_snapshot(&client),
                    antigravity::fetch_snapshot(&client, &state.antigravity_session),
                );
                if let Ok(mut slot) = state.last_antigravity_fetch.lock() {
                    *slot = Some(Instant::now());
                }
                vec![codex_res, gemini_res]
            } else {
                let codex_res = codex::fetch_snapshot(&client).await;
                vec![codex_res, cached_gemini.unwrap()]
            }
        }
        (true, false) => vec![codex::fetch_snapshot(&client).await],
        (false, true) => {
            if should_fetch_antigravity {
                let gemini_res =
                    antigravity::fetch_snapshot(&client, &state.antigravity_session).await;
                if let Ok(mut slot) = state.last_antigravity_fetch.lock() {
                    *slot = Some(Instant::now());
                }
                vec![gemini_res]
            } else {
                vec![cached_gemini.unwrap()]
            }
        }
        (false, false) => Vec::new(),
    };

    if !incoming.is_empty()
        && incoming
            .iter()
            .all(|snapshot| snapshot.status == "unavailable")
    {
        *state.client.lock().unwrap_or_else(|err| err.into_inner()) = None;
    }

    let values = if let Ok(mut cache) = state.snapshot_cache.lock() {
        let current: Vec<ProviderSnapshot> = cache
            .as_ref()
            .map(|(_, values)| {
                values
                    .iter()
                    .filter(|item| {
                        (item.provider == "codex" && show_codex)
                            || (item.provider == "gemini" && show_antigravity)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let merged = merge_snapshots(&current, incoming);
        *cache = Some((Instant::now(), merged.clone()));
        merged
    } else {
        incoming
    };

    let resolved_prefs = {
        let mut prefs_guard = state.preferences.lock().unwrap_or_else(|err| err.into_inner());
        let resolved =
            resolve_tray_provider(&values, &prefs_guard, chrono::Utc::now(), true);
        if resolved != prefs_guard.tray_provider {
            prefs_guard.tray_provider = resolved;
            let normalized = prefs_guard.clone().normalized();
            *prefs_guard = normalized.clone();
            let _ = persist_preferences(&state.preferences_path, &normalized);
            Some(normalized)
        } else {
            None
        }
    };
    if let Some(updated_prefs) = resolved_prefs {
        let _ = app.emit_to("widget", "preferences-changed", updated_prefs);
    }

    publish_snapshots(app, &values);
    values
}

fn publish_snapshots(app: &AppHandle, values: &[ProviderSnapshot]) {
    let _ = app.emit_to("widget", "snapshots-changed", values.to_vec());
    let preferences = app
        .state::<AppState>()
        .preferences
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default();
    update_windows_tray(app, values);
    sync_tray_menu(app, &preferences, values);
    resize_open_popup_if_needed(app, &preferences, values);
}

fn update_windows_tray(app: &AppHandle, snapshots: &[ProviderSnapshot]) {
    let preferences = app
        .state::<AppState>()
        .preferences
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default();
    let visual = visual_for_snapshots(snapshots, &preferences, chrono::Utc::now());
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_icon(Some(Image::new_owned(render_icon(&visual), 32, 32)));
        let _ = tray.set_tooltip(Some(&visual.tooltip));
    }
}

fn update_tray_from_cache(app: &AppHandle) {
    let values = app
        .state::<AppState>()
        .snapshot_cache
        .lock()
        .ok()
        .and_then(|cache| cache.as_ref().map(|(_, values)| values.clone()))
        .unwrap_or_default();
    let preferences = app
        .state::<AppState>()
        .preferences
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default();
    update_windows_tray(app, &values);
    sync_tray_menu(app, &preferences, &values);
}

fn load_preferences(path: &PathBuf) -> WidgetPreferences {
    let parse = |candidate: &PathBuf| {
        fs::read_to_string(candidate)
            .ok()
            .and_then(|raw| serde_json::from_str::<WidgetPreferences>(&raw).ok())
    };
    if let Some(value) = parse(path) {
        return value.normalized();
    }
    let backup = path.with_extension("json.bak");
    if let Some(value) = parse(&backup) {
        eprintln!("preferences recovered from backup");
        return value.normalized();
    }
    WidgetPreferences::default()
}

fn persist_preferences(path: &PathBuf, value: &WidgetPreferences) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| "failed to create settings directory".to_string())?;
    }
    let serialized =
        serde_json::to_vec_pretty(value).map_err(|_| "failed to serialize settings".to_string())?;
    let temporary = path.with_extension("json.tmp");
    let backup = path.with_extension("json.bak");
    let mut file = fs::File::create(&temporary)
        .map_err(|_| "failed to create temporary settings file".to_string())?;
    file.write_all(&serialized)
        .and_then(|_| file.sync_all())
        .map_err(|_| "failed to write settings".to_string())?;
    if path.exists() {
        let _ = fs::remove_file(&backup);
        fs::rename(path, &backup).map_err(|_| "failed to back up settings".to_string())?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::rename(&backup, path);
        return Err(format!("failed to commit settings: {error}"));
    }
    Ok(())
}

#[tauri::command]
async fn get_snapshots(app: AppHandle) -> Result<Vec<ProviderSnapshot>, String> {
    Ok(fetch_and_publish(&app, false, false).await)
}

#[tauri::command]
async fn refresh_snapshots(app: AppHandle) -> Result<Vec<ProviderSnapshot>, String> {
    Ok(fetch_and_publish(&app, true, true).await)
}

#[tauri::command]
fn open_antigravity() -> Result<(), String> {
    antigravity::launch_antigravity()
}

fn is_gemini_signing_in(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .and_then(|state| {
            state
                .antigravity_login_cancel
                .lock()
                .ok()
                .map(|guard| guard.is_some())
        })
        .unwrap_or(false)
}

fn cancel_antigravity_sign_in_inner(app: &AppHandle) {
    let cancel_tx = app.try_state::<AppState>().and_then(|state| {
        state
            .antigravity_login_cancel
            .lock()
            .ok()
            .and_then(|mut guard| guard.take())
    });
    if let Some(tx) = cancel_tx {
        let _ = tx.send(());
    }
    let _ = app.emit_to("widget", "antigravity-signin-state", false);
    update_tray_from_cache(app);
}

async fn run_antigravity_sign_in(app: AppHandle) -> Result<Vec<ProviderSnapshot>, String> {
    let rx = {
        let state = app.state::<AppState>();
        let mut guard = state
            .antigravity_login_cancel
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if guard.is_some() {
            return Err("Sign-in is already in progress. Complete it in your browser.".into());
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        *guard = Some(tx);
        rx
    };

    let _ = app.emit_to("widget", "antigravity-signin-state", true);
    update_tray_from_cache(&app);

    let outcome = antigravity::sign_in_via_cli_or_launch(rx).await;

    {
        let state = app.state::<AppState>();
        let mut guard = state
            .antigravity_login_cancel
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        *guard = None;
    }
    let _ = app.emit_to("widget", "antigravity-signin-state", false);
    update_tray_from_cache(&app);

    match outcome {
        Ok(antigravity::SignInOutcome::Authenticated) => {
            if let Ok(mut guard) = app.state::<AppState>().antigravity_session.lock() {
                *guard = antigravity::SessionCache::default();
            }
            Ok(fetch_and_publish(&app, true, true).await)
        }
        Ok(antigravity::SignInOutcome::LaunchedExternalApp | antigravity::SignInOutcome::Cancelled) => {
            Ok(fetch_and_publish(&app, false, false).await)
        }
        Err(err) => Err(err),
    }
}

#[tauri::command]
async fn sign_in_antigravity(app: AppHandle) -> Result<Vec<ProviderSnapshot>, String> {
    run_antigravity_sign_in(app).await
}

#[tauri::command]
fn cancel_antigravity_sign_in(app: AppHandle) {
    cancel_antigravity_sign_in_inner(&app);
}

#[tauri::command]
fn get_antigravity_sign_in_state(app: AppHandle) -> bool {
    is_gemini_signing_in(&app)
}

#[tauri::command]
fn get_preferences(state: State<'_, AppState>) -> Result<WidgetPreferences, String> {
    state
        .preferences
        .lock()
        .map(|value| value.clone())
        .map_err(|_| "settings unavailable".into())
}

fn apply_preferences_update(
    app: &AppHandle,
    state: &State<'_, AppState>,
    previous: &WidgetPreferences,
    next: WidgetPreferences,
) -> Result<(), String> {
    let next = next.normalized();
    persist_preferences(&state.preferences_path, &next)?;
    *state
        .preferences
        .lock()
        .map_err(|_| "settings unavailable".to_string())? = next.clone();

    let newly_enabled_codex = !previous.show_codex && next.show_codex;
    let newly_enabled_antigravity = !previous.show_antigravity && next.show_antigravity;

    if !next.show_antigravity {
        if let Ok(mut slot) = state.last_antigravity_fetch.lock() {
            *slot = None;
        }
    }

    let values = if let Ok(mut cache) = state.snapshot_cache.lock() {
        let filtered: Vec<ProviderSnapshot> = cache
            .as_ref()
            .map(|(_, values)| {
                values
                    .iter()
                    .filter(|item| {
                        (item.provider == "codex" && next.show_codex)
                            || (item.provider == "gemini" && next.show_antigravity)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if let Some((time, _)) = cache.as_ref() {
            *cache = Some((*time, filtered.clone()));
        }
        filtered
    } else {
        Vec::new()
    };

    let _ = app.emit_to("widget", "preferences-changed", next.clone());
    publish_snapshots(app, &values);

    if newly_enabled_codex || newly_enabled_antigravity {
        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = fetch_and_publish(&app_handle, true, newly_enabled_antigravity).await;
        });
    }
    Ok(())
}

#[tauri::command]
fn set_preferences(
    preferences: WidgetPreferences,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let previous = state
        .preferences
        .lock()
        .map_err(|_| "settings unavailable".to_string())?
        .clone();
    apply_preferences_update(&app, &state, &previous, preferences)
}

fn apply_lock(app: &AppHandle, locked: bool) -> Result<(), String> {
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    window
        .set_ignore_cursor_events(locked)
        .map_err(|_| "failed to toggle click-through".to_string())
}

#[tauri::command]
fn set_widget_locked(
    locked: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WidgetPreferences, String> {
    let previous = state
        .preferences
        .lock()
        .map_err(|_| "settings unavailable".to_string())?
        .clone();
    let mut next = previous.clone();
    next.locked = locked;
    persist_preferences(&state.preferences_path, &next)?;
    if let Err(error) = apply_lock(&app, locked) {
        let _ = persist_preferences(&state.preferences_path, &previous);
        return Err(error);
    }
    *state
        .preferences
        .lock()
        .map_err(|_| "settings unavailable".to_string())? = next.clone();
    Ok(next)
}

#[tauri::command]
fn set_widget_always_on_top(
    always_on_top: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WidgetPreferences, String> {
    let previous = state
        .preferences
        .lock()
        .map_err(|_| "settings unavailable".to_string())?
        .clone();
    let mut next = previous.clone();
    next.always_on_top = always_on_top;
    persist_preferences(&state.preferences_path, &next)?;
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    if let Err(error) = window.set_always_on_top(always_on_top) {
        let _ = persist_preferences(&state.preferences_path, &previous);
        return Err(format!("failed to toggle always-on-top: {error}"));
    }
    *state
        .preferences
        .lock()
        .map_err(|_| "settings unavailable".to_string())? = next.clone();
    let _ = app.emit_to("widget", "preferences-changed", next.clone());
    Ok(next)
}

fn select_tray_provider(app: &AppHandle, provider: &str) {
    let normalized_provider = models::normalize_tray_provider(provider);
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut prefs) = state.preferences.lock() {
            prefs.tray_provider = normalized_provider;
            let normalized = prefs.clone().normalized();
            *prefs = normalized.clone();
            let _ = persist_preferences(&state.preferences_path, &normalized);
            let _ = app.emit_to("widget", "preferences-changed", normalized);
        }
    }
    update_tray_from_cache(app);
}

fn set_presentation(app: &AppHandle, presentation: WindowPresentation) {
    if let Ok(mut value) = app.state::<AppState>().presentation.lock() {
        *value = presentation;
    }
    let _ = app.emit_to(
        "widget",
        "window-presentation-changed",
        presentation.event_name(),
    );
}

fn current_presentation(app: &AppHandle) -> WindowPresentation {
    app.state::<AppState>()
        .presentation
        .lock()
        .map(|value| *value)
        .unwrap_or(WindowPresentation::Tray)
}

fn restore_window_preferences(app: &AppHandle) {
    let preferences = app
        .state::<AppState>()
        .preferences
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default();
    if let Some(window) = app.get_webview_window("widget") {
        let _ = window.set_ignore_cursor_events(preferences.locked);
        let _ = window.set_always_on_top(preferences.always_on_top);
    }
}

fn rect_to_bounds(rect: Rect, scale_factor: f64) -> Bounds {
    let position = rect.position.to_physical::<f64>(scale_factor);
    let size = rect.size.to_physical::<f64>(scale_factor);
    Bounds {
        x: position.x.round() as i32,
        y: position.y.round() as i32,
        width: size.width.round().max(1.0) as u32,
        height: size.height.round().max(1.0) as u32,
    }
}

fn record_tray_bounds(app: &AppHandle, position: PhysicalPosition<f64>, rect: Rect) -> Bounds {
    let scale_factor = app
        .monitor_from_point(position.x, position.y)
        .ok()
        .flatten()
        .map(|monitor| monitor.scale_factor())
        .unwrap_or(1.0);
    let bounds = rect_to_bounds(rect, scale_factor);
    if let Ok(mut value) = app.state::<AppState>().last_tray_bounds.lock() {
        *value = Some(bounds);
    }
    bounds
}

fn place_window_near_tray(
    app: &AppHandle,
    bounds: Option<Bounds>,
    logical_width: u32,
    logical_height: u32,
) {
    let Some(window) = app.get_webview_window("widget") else {
        return;
    };
    let remembered = app
        .state::<AppState>()
        .last_tray_bounds
        .lock()
        .ok()
        .and_then(|value| *value);
    let requested = bounds.or(remembered);
    let monitor = requested
        .and_then(|tray| {
            app.monitor_from_point(
                tray.x as f64 + tray.width as f64 / 2.0,
                tray.y as f64 + tray.height as f64 / 2.0,
            )
            .ok()
            .flatten()
        })
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    let scale = monitor.scale_factor();
    let _ = window.set_size(LogicalSize::new(logical_width, logical_height));
    let physical_width = (logical_width as f64 * scale).round().max(1.0) as u32;
    let physical_height = (logical_height as f64 * scale).round().max(1.0) as u32;
    let work = monitor.work_area();
    let monitor_bounds = Bounds {
        x: monitor.position().x,
        y: monitor.position().y,
        width: monitor.size().width,
        height: monitor.size().height,
    };
    let work_bounds = Bounds {
        x: work.position.x,
        y: work.position.y,
        width: work.size.width,
        height: work.size.height,
    };
    let tray = requested.unwrap_or(Bounds {
        x: work.position.x + work.size.width as i32 - 24,
        y: monitor.position().y + monitor.size().height as i32 - 24,
        width: 16,
        height: 16,
    });
    let point = popup_position(
        tray,
        monitor_bounds,
        work_bounds,
        physical_width,
        physical_height,
        (8.0 * scale).round() as i32,
    );
    let _ = window.set_position(PhysicalPosition::new(point.x, point.y));
}

fn resize_open_popup_if_needed(
    app: &AppHandle,
    preferences: &WidgetPreferences,
    snapshots: &[ProviderSnapshot],
) {
    if current_presentation(app) != WindowPresentation::Popup {
        return;
    }
    let visible = app
        .get_webview_window("widget")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);
    if !visible {
        return;
    }
    let height = popup_height_for(preferences, snapshots);
    let changed = app
        .try_state::<AppState>()
        .and_then(|state| {
            state.last_popup_height.lock().ok().map(|mut last| {
                if *last == Some(height) {
                    false
                } else {
                    *last = Some(height);
                    true
                }
            })
        })
        .unwrap_or(true);
    if changed {
        place_window_near_tray(app, None, POPUP_WIDTH, height);
    }
}

fn hide_windows_widget(app: &AppHandle) {
    set_presentation(app, WindowPresentation::Tray);
    if let Ok(mut last) = app.state::<AppState>().last_popup_height.lock() {
        *last = None;
    }
    if let Some(window) = app.get_webview_window("widget") {
        let _ = window.hide();
    }
    restore_window_preferences(app);
}

fn toggle_tray_popup(app: &AppHandle, bounds: Bounds) {
    if current_presentation(app) == WindowPresentation::Popup
        && app
            .get_webview_window("widget")
            .and_then(|window| window.is_visible().ok())
            .unwrap_or(false)
    {
        hide_windows_widget(app);
        return;
    }
    let (preferences, snapshots) = {
        let state = app.state::<AppState>();
        let prefs = state
            .preferences
            .lock()
            .map(|value| value.clone())
            .unwrap_or_default();
        let snaps = state
            .snapshot_cache
            .lock()
            .ok()
            .and_then(|cache| cache.as_ref().map(|(_, values)| values.clone()))
            .unwrap_or_default();
        (prefs, snaps)
    };
    let height = popup_height_for(&preferences, &snapshots);
    if let Ok(mut last) = app.state::<AppState>().last_popup_height.lock() {
        *last = Some(height);
    }
    set_presentation(app, WindowPresentation::Popup);
    if let Some(window) = app.get_webview_window("widget") {
        let _ = window.set_ignore_cursor_events(false);
        let _ = window.set_always_on_top(true);
        place_window_near_tray(app, Some(bounds), POPUP_WIDTH, height);
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn toggle_floating_widget(app: &AppHandle) {
    let visible = app
        .get_webview_window("widget")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);
    if current_presentation(app) == WindowPresentation::Floating && visible {
        hide_windows_widget(app);
        return;
    }
    set_presentation(app, WindowPresentation::Floating);
    restore_window_preferences(app);
    if let Some(window) = app.get_webview_window("widget") {
        let _ = window.set_size(LogicalSize::new(ORB_SIZE, ORB_SIZE));
        let remembered = app
            .state::<AppState>()
            .last_floating_position
            .lock()
            .ok()
            .and_then(|value| *value);
        if let Some(point) = remembered {
            let _ = window.set_position(PhysicalPosition::new(point.x, point.y));
        } else {
            place_window_near_tray(app, None, ORB_SIZE, ORB_SIZE);
        }
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn build_tray_menu(app: &AppHandle, sig: &TrayMenuSignature) -> tauri::Result<Menu<Wry>> {
    let show = MenuItem::with_id(
        app,
        "show",
        "Floating widget / 桌面悬浮窗",
        true,
        None::<&str>,
    )?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh now / 立即刷新", true, None::<&str>)?;
    let gemini_signin = (sig.show_antigravity && (sig.gemini_signed_out || sig.gemini_signing_in))
        .then(|| {
            if sig.gemini_signing_in {
                MenuItem::with_id(
                    app,
                    "cancel_gemini_signin",
                    "Cancel Gemini sign-in / 取消 Gemini 登录",
                    true,
                    None::<&str>,
                )
            } else {
                MenuItem::with_id(
                    app,
                    "signin_gemini",
                    "Sign in to Gemini / 登录 Gemini",
                    true,
                    None::<&str>,
                )
            }
        })
        .transpose()?;

    let toggle_codex = CheckMenuItem::with_id(
        app,
        "toggle_codex",
        "Codex (GPT)",
        sig.show_antigravity,
        sig.show_codex,
        None::<&str>,
    )?;
    let toggle_antigravity = CheckMenuItem::with_id(
        app,
        "toggle_antigravity",
        "Antigravity (Gemini)",
        sig.show_codex,
        sig.show_antigravity,
        None::<&str>,
    )?;
    let services_menu = Submenu::with_id_and_items(
        app,
        "services",
        "Services / 显示服务",
        true,
        &[&toggle_codex, &toggle_antigravity],
    )?;

    let tray_codex = (sig.show_codex && sig.codex_has_short)
        .then(|| {
            CheckMenuItem::with_id(
                app,
                "tray_codex",
                "Codex (GPT) · 5h",
                true,
                sig.active_tray == "codex",
                None::<&str>,
            )
        })
        .transpose()?;
    let tray_codex_weekly = (sig.show_codex && sig.codex_has_weekly)
        .then(|| {
            CheckMenuItem::with_id(
                app,
                "tray_codex_weekly",
                "Codex (GPT) · Weekly / 周额度",
                true,
                sig.active_tray == "codex_weekly",
                None::<&str>,
            )
        })
        .transpose()?;
    let has_codex_items = tray_codex.is_some() || tray_codex_weekly.is_some();

    let tray_gemini = (sig.show_antigravity && sig.gemini_has_short)
        .then(|| {
            CheckMenuItem::with_id(
                app,
                "tray_gemini",
                "Gemini · 5h",
                true,
                sig.active_tray == "gemini",
                None::<&str>,
            )
        })
        .transpose()?;
    let tray_gemini_weekly = (sig.show_antigravity && sig.gemini_has_weekly)
        .then(|| {
            CheckMenuItem::with_id(
                app,
                "tray_gemini_weekly",
                "Gemini · Weekly / 周额度",
                true,
                sig.active_tray == "gemini_weekly",
                None::<&str>,
            )
        })
        .transpose()?;
    let tray_3p = (sig.show_antigravity && sig.has_3p_short)
        .then(|| {
            CheckMenuItem::with_id(
                app,
                "tray_3p",
                "Claude & GPT · 5h",
                true,
                sig.active_tray == "antigravity_3p",
                None::<&str>,
            )
        })
        .transpose()?;
    let tray_3p_weekly = (sig.show_antigravity && sig.has_3p_weekly)
        .then(|| {
            CheckMenuItem::with_id(
                app,
                "tray_3p_weekly",
                "Claude & GPT · Weekly / 周额度",
                true,
                sig.active_tray == "antigravity_3p_weekly",
                None::<&str>,
            )
        })
        .transpose()?;
    let tray_argon_weekly = (sig.show_antigravity && sig.has_argon_weekly)
        .then(|| {
            CheckMenuItem::with_id(
                app,
                "tray_argon_weekly",
                "Gemini 4 Argon · Weekly / 周额度",
                true,
                sig.active_tray == "gemini_argon_weekly",
                None::<&str>,
            )
        })
        .transpose()?;
    let has_antigravity_items = tray_gemini.is_some()
        || tray_gemini_weekly.is_some()
        || tray_3p.is_some()
        || tray_3p_weekly.is_some()
        || tray_argon_weekly.is_some();

    let tray_sep = (has_codex_items && has_antigravity_items)
        .then(|| PredefinedMenuItem::separator(app))
        .transpose()?;

    let mut source_items: Vec<&dyn IsMenuItem<Wry>> = Vec::new();
    if let Some(item) = &tray_codex {
        source_items.push(item);
    }
    if let Some(item) = &tray_codex_weekly {
        source_items.push(item);
    }
    if let Some(item) = &tray_sep {
        source_items.push(item);
    }
    if let Some(item) = &tray_gemini {
        source_items.push(item);
    }
    if let Some(item) = &tray_gemini_weekly {
        source_items.push(item);
    }
    if let Some(item) = &tray_3p {
        source_items.push(item);
    }
    if let Some(item) = &tray_3p_weekly {
        source_items.push(item);
    }
    if let Some(item) = &tray_argon_weekly {
        source_items.push(item);
    }

    let tray_source_menu = Submenu::with_id_and_items(
        app,
        "tray_source",
        "Tray display / 托盘显示额度",
        true,
        &source_items,
    )?;

    let separator = PredefinedMenuItem::separator(app)?;
    let check_updates = MenuItem::with_id(
        app,
        "check_updates",
        "Check for updates / 检查更新",
        true,
        None::<&str>,
    )?;
    let unlock = MenuItem::with_id(app, "unlock", "Unlock widget / 解锁穿透", true, None::<&str>)?;
    let language = MenuItem::with_id(
        app,
        "language",
        "Switch Language / 切换语言",
        true,
        None::<&str>,
    )?;
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "Start at login / 开机自启",
        true,
        sig.autostart,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit / 退出", true, None::<&str>)?;

    let mut top_items: Vec<&dyn IsMenuItem<Wry>> = vec![&show, &refresh];
    if let Some(item) = &gemini_signin {
        top_items.push(item);
    }
    top_items.extend([
        &services_menu as &dyn IsMenuItem<Wry>,
        &tray_source_menu,
        &separator,
        &check_updates,
        &unlock,
        &language,
        &autostart,
        &quit,
    ]);
    Menu::with_items(app, &top_items)
}

fn sync_tray_menu(
    app: &AppHandle,
    preferences: &WidgetPreferences,
    snapshots: &[ProviderSnapshot],
) {
    let autostart = app.autolaunch().is_enabled().unwrap_or(false);
    let signing_in = is_gemini_signing_in(app);
    let sig = menu_signature_for(preferences, snapshots, autostart, signing_in);
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let should_rebuild = state
        .tray_menu_signature
        .lock()
        .map(|guard| guard.as_ref() != Some(&sig))
        .unwrap_or(true);
    if !should_rebuild {
        return;
    }
    if let Some(tray) = app.tray_by_id("main") {
        if let Ok(menu) = build_tray_menu(app, &sig) {
            if tray.set_menu(Some(menu)).is_ok() {
                if let Ok(mut guard) = state.tray_menu_signature.lock() {
                    *guard = Some(sig);
                }
            }
        }
    }
}

fn toggle_provider_visibility(app: &AppHandle, provider: &str) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(previous) = state.preferences.lock().map(|value| value.clone()) else {
        return;
    };
    let mut next = previous.clone();
    match provider {
        "codex" => {
            if next.show_codex && !next.show_antigravity {
                return;
            }
            next.show_codex = !next.show_codex;
        }
        "gemini" => {
            if next.show_antigravity && !next.show_codex {
                return;
            }
            next.show_antigravity = !next.show_antigravity;
        }
        _ => return,
    }
    let _ = apply_preferences_update(app, &state, &previous, next);
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let preferences = app
        .state::<AppState>()
        .preferences
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default();

    let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
    let sig = menu_signature_for(&preferences, &[], autostart_enabled, false);
    let menu = build_tray_menu(app.handle(), &sig)?;
    if let Ok(mut slot) = app.state::<AppState>().tray_menu_signature.lock() {
        *slot = Some(sig);
    }

    let visual = visual_for_snapshots(&[], &preferences, chrono::Utc::now());

    TrayIconBuilder::with_id("main")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .icon(Image::new_owned(render_icon(&visual), 32, 32))
        .tooltip(&visual.tooltip)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "show" => toggle_floating_widget(app),
            "refresh" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = fetch_and_publish(&app, true, true).await;
                });
            }
            "signin_gemini" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = run_antigravity_sign_in(app).await;
                });
            }
            "cancel_gemini_signin" => cancel_antigravity_sign_in_inner(app),
            "toggle_codex" => toggle_provider_visibility(app, "codex"),
            "toggle_antigravity" => toggle_provider_visibility(app, "gemini"),
            "tray_codex" => select_tray_provider(app, "codex"),
            "tray_codex_weekly" => select_tray_provider(app, "codex_weekly"),
            "tray_gemini" => select_tray_provider(app, "gemini"),
            "tray_gemini_weekly" => select_tray_provider(app, "gemini_weekly"),
            "tray_3p" => select_tray_provider(app, "antigravity_3p"),
            "tray_3p_weekly" => select_tray_provider(app, "antigravity_3p_weekly"),
            "tray_argon_weekly" => select_tray_provider(app, "gemini_argon_weekly"),
            "check_updates" => updates::start_check(app.clone(), true),
            "unlock" => {
                let _ = apply_lock(app, false);
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut prefs) = state.preferences.lock() {
                        prefs.locked = false;
                        let _ = persist_preferences(&state.preferences_path, &prefs);
                        let _ = app.emit_to("widget", "preferences-changed", prefs.clone());
                    }
                }
            }
            "language" => {
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut prefs) = state.preferences.lock() {
                        prefs.language = if prefs.language == "en" {
                            "zh-CN".into()
                        } else {
                            "en".into()
                        };
                        let normalized = prefs.clone().normalized();
                        *prefs = normalized.clone();
                        let _ = persist_preferences(&state.preferences_path, &normalized);
                        let _ = app.emit_to("widget", "preferences-changed", normalized);
                    }
                }
                update_tray_from_cache(app);
            }
            "autostart" => {
                let manager = app.autolaunch();
                let enabled = manager.is_enabled().unwrap_or(false);
                let result = if enabled {
                    manager.disable()
                } else {
                    manager.enable()
                };
                match result {
                    Ok(()) => update_tray_from_cache(app),
                    Err(_) => eprintln!("autostart update failed"),
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_updater::Builder::new()
                .pubkey(option_env!("QUOTAFLOAT_UPDATER_PUBLIC_KEY").unwrap_or(""))
                .build(),
        )
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            toggle_floating_widget(app);
        }))
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(
            WindowStateBuilder::default()
                .with_state_flags(StateFlags::all() & !StateFlags::VISIBLE)
                .build(),
        )
        .setup(|app| {
            app.manage(updates::UpdateCheckLock::default());
            let data_dir = app.path().app_config_dir()?;
            let preferences_path = data_dir.join("preferences.json");
            let preferences = load_preferences(&preferences_path);
            app.manage(AppState {
                client: Mutex::new(None),
                antigravity_session: Mutex::new(antigravity::SessionCache::default()),
                antigravity_login_cancel: Mutex::new(None),
                last_antigravity_fetch: Mutex::new(None),
                preferences: Mutex::new(preferences.clone()),
                preferences_path,
                fetch_lock: tokio::sync::Mutex::new(()),
                snapshot_cache: Mutex::new(None),
                presentation: Mutex::new(WindowPresentation::Tray),
                last_tray_bounds: Mutex::new(None),
                last_floating_position: Mutex::new(None),
                last_popup_height: Mutex::new(None),
                tray_menu_signature: Mutex::new(None),
            });
            let tray_ready = setup_tray(app).is_ok();
            if !tray_ready {
                eprintln!("tray setup failed; enabling taskbar fallback");
                if let Some(window) = app.get_webview_window("widget") {
                    let _ = window.set_skip_taskbar(false);
                    let _ = window.show();
                }
                set_presentation(app.handle(), WindowPresentation::Floating);
            }
            if preferences.locked {
                let _ = apply_lock(app.handle(), true);
            }
            if let Some(window) = app.get_webview_window("widget") {
                let _ = window.set_always_on_top(preferences.always_on_top);
                if tray_ready {
                    let _ = window.hide();
                }
            }
            let app_handle = app.handle().clone();
            updates::start_periodic_checks(app.handle().clone());
            tauri::async_runtime::spawn(async move {
                let _ = fetch_and_publish(&app_handle, true, true).await;
                loop {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    let _ = fetch_and_publish(&app_handle, true, false).await;
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshots,
            refresh_snapshots,
            open_antigravity,
            sign_in_antigravity,
            cancel_antigravity_sign_in,
            get_antigravity_sign_in_state,
            get_preferences,
            set_preferences,
            set_widget_locked,
            set_widget_always_on_top
        ])
        .on_tray_icon_event(|app, event| {
            if let TrayIconEvent::Click {
                position,
                rect,
                button,
                button_state,
                ..
            } = event
            {
                let bounds = record_tray_bounds(app, position, rect);
                if button == MouseButton::Left && button_state == MouseButtonState::Up {
                    toggle_tray_popup(app, bounds);
                }
            }
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                hide_windows_widget(window.app_handle());
            }
            WindowEvent::Focused(false)
                if current_presentation(window.app_handle()) == WindowPresentation::Popup =>
            {
                hide_windows_widget(window.app_handle());
            }
            WindowEvent::Moved(position)
                if current_presentation(window.app_handle()) == WindowPresentation::Floating =>
            {
                if let Ok(mut value) = window
                    .app_handle()
                    .state::<AppState>()
                    .last_floating_position
                    .lock()
                {
                    *value = Some(Point {
                        x: position.x,
                        y: position.y,
                    });
                }
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("failed to build QuotaPulse");
    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::Resumed) {
            let app_handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                let _ = fetch_and_publish(&app_handle, true, true).await;
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::{QuotaPool, UsageWindow};

    fn weekly_window(remaining_percent: f64) -> UsageWindow {
        UsageWindow {
            remaining_percent,
            resets_at: Some("2026-11-29T00:00:00Z".into()),
            window_seconds: 604_800,
        }
    }

    fn gemini_ultra_snapshot(with_3p: bool) -> ProviderSnapshot {
        ProviderSnapshot {
            provider: "gemini".into(),
            display_name: "GEMINI".into(),
            plan: Some("ULTRA".into()),
            short_window: None,
            weekly_window: Some(weekly_window(90.0)),
            extra_pools: if with_3p {
                vec![QuotaPool {
                    id: "antigravity_3p".into(),
                    display_name: "Claude & GPT".into(),
                    short_window: None,
                    weekly_window: Some(weekly_window(55.0)),
                }]
            } else {
                Vec::new()
            },
            reset_credits: None,
            reset_credit_expires_at: Vec::new(),
            updated_at: "2026-11-22T00:00:00Z".into(),
            status: "ok".into(),
            message: None,
        }
    }

    #[test]
    fn computes_adaptive_popup_height_for_single_and_dual_services() {
        let mut prefs = WidgetPreferences::default();
        let snaps = vec![gemini_ultra_snapshot(true)];
        assert_eq!(popup_height_for(&prefs, &snaps), POPUP_HEIGHT_DUAL);

        prefs.show_codex = true;
        prefs.show_antigravity = false;
        assert_eq!(popup_height_for(&prefs, &snaps), POPUP_HEIGHT_CODEX_ONLY);

        prefs.show_codex = false;
        prefs.show_antigravity = true;
        assert_eq!(
            popup_height_for(&prefs, &snaps),
            POPUP_HEIGHT_ANTIGRAVITY_WITH_POOLS
        );
        assert_eq!(
            popup_height_for(&prefs, &[gemini_ultra_snapshot(false)]),
            POPUP_HEIGHT_ANTIGRAVITY_COMPACT
        );
    }

    #[test]
    fn builds_tray_menu_signature_respecting_visibility_and_available_windows() {
        let codex_pro = ProviderSnapshot {
            provider: "codex".into(),
            display_name: "CODEX".into(),
            plan: Some("PRO".into()),
            short_window: None,
            weekly_window: Some(weekly_window(80.0)),
            extra_pools: Vec::new(),
            reset_credits: None,
            reset_credit_expires_at: Vec::new(),
            updated_at: "2026-11-22T00:00:00Z".into(),
            status: "ok".into(),
            message: None,
        };
        let ultra = gemini_ultra_snapshot(true);
        let prefs = WidgetPreferences {
            show_codex: false,
            show_antigravity: true,
            tray_provider: "gemini_argon_weekly".into(),
            ..WidgetPreferences::default()
        };
        let sig = menu_signature_for(&prefs, &[codex_pro, ultra], true, false);
        assert!(!sig.show_codex);
        assert!(sig.show_antigravity);
        assert!(!sig.gemini_signed_out);
        assert!(!sig.gemini_signing_in);
        assert_eq!(sig.active_tray, "gemini_weekly");
        assert!(!sig.gemini_has_short);
        assert!(sig.gemini_has_weekly);
        assert!(!sig.has_3p_short);
        assert!(sig.has_3p_weekly);
        assert!(!sig.has_argon_weekly);

        let signed_out_gemini = ProviderSnapshot::failure_for(
            "gemini",
            "GEMINI",
            "signed_out",
            "Antigravity is not signed in. Sign in directly from QuotaPulse.",
        );
        let signed_out_sig = menu_signature_for(&prefs, &[signed_out_gemini], true, true);
        assert!(signed_out_sig.gemini_signed_out);
        assert!(signed_out_sig.gemini_signing_in);
    }
}
