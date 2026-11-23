import { useCallback, useEffect, useRef, useState } from "react";
import { QuotaCard, QuotaOrb } from "./components/QuotaCard";
import {
  expandedHeightFor,
  fetchSnapshots,
  getPreferences,
  listenDesktopEvents,
  openAntigravity,
  setAlwaysOnTop,
  setWidgetExpanded,
  startDragging,
  updatePreferences,
  type WindowPresentation,
} from "./lib/bridge";
import {
  copy,
  nextLanguage,
  normalizeLanguage,
  normalizeProviderVisibility,
  normalizeTrayProviderForVisibility,
} from "./lib/i18n";
import type { ProviderId, ProviderSnapshot, TrayProviderId, WidgetPreferences } from "./types";

const DEFAULT_PREFS: WidgetPreferences = {
  locked: false,
  alwaysOnTop: true,
  pinnedProvider: null,
  autoRotateSeconds: 12,
  language: "zh-CN",
  trayProvider: "codex",
  showCodex: true,
  showAntigravity: true,
};

function normalizePrefs(value: Partial<WidgetPreferences>): WidgetPreferences {
  const visibility = normalizeProviderVisibility(value.showCodex, value.showAntigravity);
  return {
    ...DEFAULT_PREFS,
    ...value,
    ...visibility,
    language: normalizeLanguage(value.language),
    trayProvider: normalizeTrayProviderForVisibility(
      value.trayProvider,
      visibility.showCodex,
      visibility.showAntigravity,
    ),
  };
}

export default function App() {
  const [snapshots, setSnapshots] = useState<ProviderSnapshot[]>([]);
  const [preferences, setPreferences] = useState(DEFAULT_PREFS);
  const [compact, setCompact] = useState(() => window.innerWidth <= 120 || window.innerHeight <= 120);
  const [presentation, setPresentation] = useState<WindowPresentation>("floating");
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [operationError, setOperationError] = useState<string | null>(null);
  const refreshing = useRef(false);
  const preferencesRef = useRef(preferences);
  preferencesRef.current = preferences;
  const language = normalizeLanguage(preferences.language);
  const t = copy[language];

  const refresh = useCallback(async (force = false) => {
    if (refreshing.current) return;
    refreshing.current = true;
    setIsRefreshing(true);
    try {
      const values = await fetchSnapshots(force);
      setSnapshots(values);
    } catch {
      const now = new Date().toISOString();
      const visibility = normalizeProviderVisibility(
        preferencesRef.current.showCodex,
        preferencesRef.current.showAntigravity,
      );
      setSnapshots((current) => {
        if (current.length > 0) {
          return current.map((item) => ({ ...item, status: "stale", message: "Refresh failed. Please try again later." }));
        }
        const fallback: ProviderSnapshot[] = [];
        if (visibility.showCodex) {
          fallback.push({
            provider: "codex",
            displayName: "CODEX",
            plan: null,
            shortWindow: null,
            weeklyWindow: null,
            extraPools: [],
            resetCredits: null,
            resetCreditExpiresAt: [],
            updatedAt: now,
            status: "unavailable",
            message: "Quota is temporarily unavailable. It will retry automatically.",
          });
        }
        if (visibility.showAntigravity) {
          fallback.push({
            provider: "gemini",
            displayName: "GEMINI",
            plan: null,
            shortWindow: null,
            weeklyWindow: null,
            extraPools: [],
            resetCredits: null,
            resetCreditExpiresAt: [],
            updatedAt: now,
            status: "unavailable",
            message: "Antigravity quota is temporarily unavailable. It will retry automatically.",
          });
        }
        return fallback;
      });
    } finally {
      refreshing.current = false;
      setIsRefreshing(false);
    }
  }, []);

  useEffect(() => {
    void refresh(false);
    void getPreferences()
      .then((value) => setPreferences(normalizePrefs(value)))
      .catch(() => setOperationError("Unable to read settings. Defaults are in use."));
  }, [refresh]);

  useEffect(() => {
    const updateCompact = () => setCompact(window.innerWidth <= 120 || window.innerHeight <= 120);
    updateCompact();
    window.addEventListener("resize", updateCompact);
    return () => window.removeEventListener("resize", updateCompact);
  }, []);

  useEffect(() => {
    let cancelled = false;
    let cleanup: () => void = () => {};
    void listenDesktopEvents({
      onPreferences: (value) => setPreferences(normalizePrefs(value)),
      onRefresh: () => void refresh(true),
      onSnapshots: (value) => setSnapshots(value),
      onPresentation: (value) => {
        setPresentation(value);
        setCompact(value !== "popup");
      },
    }).then((value) => {
      if (cancelled) value();
      else cleanup = value;
    }).catch(() => setOperationError("Desktop event listener failed to start."));
    return () => { cancelled = true; cleanup(); };
  }, [refresh]);

  const savePreferences = useCallback((next: WidgetPreferences) => {
    const normalized = normalizePrefs(next);
    const previous = preferences;
    setPreferences(normalized);
    setOperationError(null);
    void updatePreferences(normalized).catch(() => {
      setPreferences(previous);
      setOperationError("Settings could not be saved. Previous state restored.");
    });
  }, [preferences]);

  const handleSelectTrayProvider = useCallback((trayProvider: TrayProviderId) => {
    savePreferences({ ...preferences, trayProvider });
  }, [preferences, savePreferences]);

  const handleToggleService = useCallback((provider: ProviderId) => {
    const nextShowCodex = provider === "codex" ? !preferences.showCodex : preferences.showCodex;
    const nextShowAntigravity = provider === "gemini" ? !preferences.showAntigravity : preferences.showAntigravity;
    if (!nextShowCodex && !nextShowAntigravity) return;
    savePreferences({
      ...preferences,
      showCodex: nextShowCodex,
      showAntigravity: nextShowAntigravity,
    });
  }, [preferences, savePreferences]);

  const expandedHeight = expandedHeightFor(preferences, snapshots);

  useEffect(() => {
    if (presentation === "floating" && !compact) {
      void setWidgetExpanded(true, expandedHeight).catch(() => undefined);
    }
  }, [compact, expandedHeight, presentation]);

  const handleHover = useCallback((value: boolean) => {
    if (presentation === "popup") return;
    setCompact(!value);
    void setWidgetExpanded(value, expandedHeight).catch(() =>
      setOperationError(value ? "Widget expand failed." : "Widget collapse failed."),
    );
  }, [expandedHeight, presentation]);

  const handleDrag = useCallback(() => {
    if (presentation === "floating") void startDragging();
  }, [presentation]);

  if (snapshots.length === 0) {
    return <div className="loading-card" aria-label={t.loadingQuota}><span /><span /><span /></div>;
  }

  if (compact) {
    return (
      <QuotaOrb
        snapshots={snapshots}
        trayProvider={preferences.trayProvider}
        showCodex={preferences.showCodex}
        showAntigravity={preferences.showAntigravity}
        language={language}
        onDrag={handleDrag}
        onHover={handleHover}
      />
    );
  }

  return (
    <QuotaCard
      snapshots={snapshots}
      preferences={preferences}
      onSelectTrayProvider={handleSelectTrayProvider}
      onToggleService={handleToggleService}
      onLanguage={() => savePreferences({ ...preferences, language: nextLanguage(language) })}
      onLock={() => {
        setOperationError(null);
        void setAlwaysOnTop(!preferences.alwaysOnTop)
          .then((value) => setPreferences(normalizePrefs(value)))
          .catch(() => setOperationError("Always-on-top toggle failed."));
      }}
      onDrag={handleDrag}
      onHover={handleHover}
      onRefresh={() => void refresh(true)}
      onOpenAntigravity={() => {
        setOperationError(null);
        void openAntigravity().catch(() => setOperationError("Unable to open Antigravity."));
      }}
      refreshing={isRefreshing}
      notice={operationError}
    />
  );
}
