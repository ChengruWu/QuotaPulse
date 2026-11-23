import {
  ArrowClockwise,
  ArrowSquareOut,
  Check,
  ClockCounterClockwise,
  CloudSlash,
  Desktop,
  PushPin,
  PushPinSlash,
  SignIn,
  SlidersHorizontal,
  WarningCircle,
} from "@phosphor-icons/react";
import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  clampPercent,
  formatDateTime,
  formatResetDate,
  formatResetTime,
  formatShortReset,
  quotaTier,
} from "../lib/format";
import {
  copy,
  normalizeLanguage,
  normalizeProviderVisibility,
  normalizeTrayProvider,
  normalizeTrayProviderForVisibility,
} from "../lib/i18n";
import type {
  Language,
  ProviderId,
  ProviderSnapshot,
  QuotaPool,
  TrayProviderId,
  UsageWindow,
  WidgetPreferences,
} from "../types";
import { ProviderMark } from "./ProviderMark";

interface CardProps {
  snapshots: ProviderSnapshot[];
  preferences: WidgetPreferences;
  onSelectTrayProvider: (target: TrayProviderId) => void;
  onToggleService?: (provider: ProviderId) => void;
  onLock: () => void;
  onLanguage: () => void;
  onDrag: () => void;
  onHover: (hovered: boolean) => void;
  onRefresh?: () => void;
  onOpenAntigravity?: () => void;
  refreshing?: boolean;
  notice?: string | null;
  initialShowCreditTip?: boolean;
}

interface OrbProps {
  snapshots: ProviderSnapshot[];
  trayProvider?: TrayProviderId;
  showCodex?: boolean;
  showAntigravity?: boolean;
  language?: Language;
  onDrag: () => void;
  onHover: (hovered: boolean) => void;
}

function StatusIcon({ status, expired = false }: { status: ProviderSnapshot["status"]; expired?: boolean }) {
  if (status === "signed_out") return <SignIn weight="duotone" />;
  if (status === "stale" || expired) return <ClockCounterClockwise weight="duotone" />;
  if (status === "unavailable") return <CloudSlash weight="duotone" />;
  return <WarningCircle weight="duotone" />;
}

function localizedBackendMessage(message: string | null, provider: ProviderSnapshot["provider"], language: Language): string | null {
  if (!message) return null;
  if (language === "en") return message;
  const normalized = message.toLowerCase();
  if (normalized.includes("no local antigravity login file")) return "未检测到本机 Antigravity 登录文件，请打开 Antigravity 并登录。";
  if (normalized.includes("in use")) return "本机 Antigravity 状态文件正在占用，请稍后刷新。";
  if (normalized.includes("no active antigravity session token")) return "未从本机状态中找到有效的 Antigravity 会话，请打开 Antigravity 登录。";
  if (normalized.includes("not signed in")) return "Antigravity 尚未登录，请打开 Antigravity 并登录。";
  if (normalized.includes("timed out")) return "读取 Antigravity 额度超时，将自动重试。";
  if (normalized.includes("could not be started")) return "无法启动本机 Antigravity CLI，将自动重试。";
  if (normalized.includes("(401)")) return "Antigravity 登录态已过期 (401)，请打开 Antigravity 刷新登录。";
  if (normalized.includes("(403)")) return "Antigravity 额度接口拒绝访问 (403)。";
  if (normalized.includes("sign in") || normalized.includes("login")) {
    return provider === "gemini"
      ? "Antigravity 登录已失效，请打开 Antigravity 或重新登录。"
      : "Codex 登录已失效，请重新登录。";
  }
  if (normalized.includes("rate limited")) return "请求过于频繁，将稍后自动重试。";
  if (normalized.includes("network")) return "网络不可用，将自动重试。";
  if (normalized.includes("format")) return "额度响应格式已变化。";
  if (normalized.includes("missing supported usage windows")) return "额度响应缺少可识别的额度窗口。";
  if (normalized.includes("refresh is already running")) return "额度正在刷新，请稍候。";
  return message;
}

function isSnapshotAvailable(snapshot: ProviderSnapshot): { available: boolean; staleExpired: boolean } {
  const staleAge = Date.now() - new Date(snapshot.updatedAt).getTime();
  const staleExpired = snapshot.status === "stale" && staleAge > 30 * 60_000;
  const hasAnyWindow = Boolean(
    snapshot.shortWindow
      || snapshot.weeklyWindow
      || snapshot.extraPools?.some((pool) => pool.shortWindow || pool.weeklyWindow),
  );
  const available = (snapshot.status === "ok" || (snapshot.status === "stale" && !staleExpired)) && hasAnyWindow;
  return { available, staleExpired };
}

function resolveEffectiveTrayProvider(
  snapshots: ProviderSnapshot[],
  trayProvider: TrayProviderId,
  showCodex = true,
  showAntigravity = true,
): TrayProviderId {
  const normalized = normalizeTrayProviderForVisibility(trayProvider, showCodex, showAntigravity);
  const codex = snapshots.find((item) => item.provider === "codex");
  const gemini = snapshots.find((item) => item.provider === "gemini");
  const codexUsable = codex ? isSnapshotAvailable(codex).available : false;
  const geminiUsable = gemini ? isSnapshotAvailable(gemini).available : false;

  const geminiDefault = (snapshot: ProviderSnapshot): TrayProviderId => {
    if (snapshot.shortWindow) return "gemini";
    if (snapshot.weeklyWindow) return "gemini_weekly";
    const pool3p = snapshot.extraPools?.find((pool) => pool.id === "antigravity_3p");
    if (pool3p?.shortWindow) return "antigravity_3p";
    if (pool3p?.weeklyWindow) return "antigravity_3p_weekly";
    if (snapshot.extraPools?.some((pool) => pool.id === "gemini_argon" && pool.weeklyWindow)) {
      return "gemini_argon_weekly";
    }
    return "gemini";
  };

  if (normalized === "codex" && codexUsable && !codex?.shortWindow && codex?.weeklyWindow) {
    return "codex_weekly";
  }
  if (normalized === "codex_weekly" && codexUsable && !codex?.weeklyWindow && codex?.shortWindow) {
    return "codex";
  }
  if (normalized === "gemini" && geminiUsable && !gemini?.shortWindow && gemini?.weeklyWindow) {
    return "gemini_weekly";
  }
  if (normalized === "gemini_weekly" && geminiUsable && !gemini?.weeklyWindow && gemini?.shortWindow) {
    return "gemini";
  }
  if ((normalized === "antigravity_3p" || normalized === "antigravity_3p_weekly") && geminiUsable && gemini) {
    const pool = gemini.extraPools?.find((item) => item.id === "antigravity_3p");
    if (pool) {
      if (normalized === "antigravity_3p") {
        if (!pool.shortWindow && pool.weeklyWindow) return "antigravity_3p_weekly";
        if (pool.shortWindow) return "antigravity_3p";
      } else {
        if (!pool.weeklyWindow && pool.shortWindow) return "antigravity_3p";
        if (pool.weeklyWindow) return "antigravity_3p_weekly";
      }
    }
    return geminiDefault(gemini);
  }
  if (normalized === "gemini_argon_weekly" && geminiUsable && gemini) {
    if (gemini.extraPools?.some((item) => item.id === "gemini_argon" && item.weeklyWindow)) {
      return "gemini_argon_weekly";
    }
    return geminiDefault(gemini);
  }
  return normalized;
}

function resolveTraySelection(
  snapshots: ProviderSnapshot[],
  trayProvider: TrayProviderId,
  showCodex = true,
  showAntigravity = true,
): {
  effectiveTray: TrayProviderId;
  snapshot: ProviderSnapshot | null;
  primaryWindow: UsageWindow | null;
  secondaryWindow: UsageWindow | null;
  windowKind: "short" | "weekly";
  themeKey: "codex" | "gemini" | "antigravity_3p" | "gemini_argon";
  tag: string;
  resetCredits: number | null;
} {
  const effectiveTray = resolveEffectiveTrayProvider(snapshots, trayProvider, showCodex, showAntigravity);
  const codex = snapshots.find((item) => item.provider === "codex") ?? null;
  const gemini = snapshots.find((item) => item.provider === "gemini") ?? null;
  if (effectiveTray === "codex_weekly") {
    const source = codex ?? snapshots[0] ?? null;
    return {
      effectiveTray,
      snapshot: source,
      primaryWindow: source?.weeklyWindow ?? null,
      secondaryWindow: source?.shortWindow ?? null,
      windowKind: "weekly",
      themeKey: "codex",
      tag: "GPT·W",
      resetCredits: source?.resetCredits ?? null,
    };
  }
  if (effectiveTray === "gemini") {
    return {
      effectiveTray,
      snapshot: gemini,
      primaryWindow: gemini?.shortWindow ?? null,
      secondaryWindow: gemini?.weeklyWindow ?? null,
      windowKind: "short",
      themeKey: "gemini",
      tag: "GEM",
      resetCredits: null,
    };
  }
  if (effectiveTray === "gemini_weekly") {
    return {
      effectiveTray,
      snapshot: gemini,
      primaryWindow: gemini?.weeklyWindow ?? null,
      secondaryWindow: gemini?.shortWindow ?? null,
      windowKind: "weekly",
      themeKey: "gemini",
      tag: "GEM·W",
      resetCredits: null,
    };
  }
  if (effectiveTray === "antigravity_3p") {
    const pool = gemini?.extraPools?.find((item) => item.id === "antigravity_3p") ?? null;
    return {
      effectiveTray,
      snapshot: gemini,
      primaryWindow: pool?.shortWindow ?? null,
      secondaryWindow: pool?.weeklyWindow ?? null,
      windowKind: "short",
      themeKey: "antigravity_3p",
      tag: "3P",
      resetCredits: null,
    };
  }
  if (effectiveTray === "antigravity_3p_weekly") {
    const pool = gemini?.extraPools?.find((item) => item.id === "antigravity_3p") ?? null;
    return {
      effectiveTray,
      snapshot: gemini,
      primaryWindow: pool?.weeklyWindow ?? null,
      secondaryWindow: pool?.shortWindow ?? null,
      windowKind: "weekly",
      themeKey: "antigravity_3p",
      tag: "3P·W",
      resetCredits: null,
    };
  }
  if (effectiveTray === "gemini_argon_weekly") {
    const pool = gemini?.extraPools?.find((item) => item.id === "gemini_argon") ?? null;
    return {
      effectiveTray,
      snapshot: gemini,
      primaryWindow: pool?.weeklyWindow ?? null,
      secondaryWindow: null,
      windowKind: "weekly",
      themeKey: "gemini_argon",
      tag: "ARGON",
      resetCredits: null,
    };
  }
  const source = codex ?? snapshots[0] ?? null;
  return {
    effectiveTray,
    snapshot: source,
    primaryWindow: source?.shortWindow ?? null,
    secondaryWindow: source?.weeklyWindow ?? null,
    windowKind: "short",
    themeKey: "codex",
    tag: "GPT",
    resetCredits: source?.resetCredits ?? null,
  };
}

function TrayWindowSwitch({
  shortTarget,
  weeklyTarget,
  activeTray,
  targetName,
  language,
  compact = false,
  onSelect,
}: {
  shortTarget?: TrayProviderId;
  weeklyTarget?: TrayProviderId;
  activeTray: TrayProviderId;
  targetName: string;
  language: Language;
  compact?: boolean;
  onSelect: (target: TrayProviderId) => void;
}) {
  const t = copy[language];
  const shortActive = Boolean(shortTarget && activeTray === shortTarget);
  const weeklyActive = Boolean(weeklyTarget && activeTray === weeklyTarget);
  const anyActive = shortActive || weeklyActive;

  return (
    <div
      className={`tray-switch${anyActive ? " is-active" : ""}${compact ? " tray-switch--compact" : ""}`}
      onMouseDown={(event) => event.stopPropagation()}
    >
      {!compact ? (
        <span className="tray-switch-icon" aria-hidden="true">
          <Desktop weight={anyActive ? "fill" : "regular"} />
        </span>
      ) : null}
      {shortTarget ? (
        <button
          type="button"
          className={`tray-switch-option${shortActive ? " is-active" : ""}`}
          onClick={() => onSelect(shortTarget)}
          aria-pressed={shortActive}
          aria-label={shortActive ? t.trayDisplaySelected(targetName) : t.trayDisplayAction(targetName)}
          title={shortActive ? t.trayDisplaySelected(targetName) : t.trayDisplayAction(targetName)}
        >
          {t.trayShortPill}
        </button>
      ) : null}
      {weeklyTarget ? (
        <button
          type="button"
          className={`tray-switch-option${weeklyActive ? " is-active" : ""}`}
          onClick={() => onSelect(weeklyTarget)}
          aria-pressed={weeklyActive}
          aria-label={weeklyActive ? t.trayDisplayWeeklySelected(targetName) : t.trayDisplayWeeklyAction(targetName)}
          title={weeklyActive ? t.trayDisplayWeeklySelected(targetName) : t.trayDisplayWeeklyAction(targetName)}
        >
          {compact && !shortTarget ? <Desktop weight={weeklyActive ? "fill" : "regular"} /> : null}
          <span>{t.trayWeeklyPill}</span>
        </button>
      ) : null}
    </div>
  );
}

function SubPoolTile({
  pool,
  language,
  trayProvider,
  onSelectTrayProvider,
}: {
  pool: QuotaPool;
  language: Language;
  trayProvider: TrayProviderId;
  onSelectTrayProvider: (target: TrayProviderId) => void;
}) {
  const t = copy[language];
  const shortPct = pool.shortWindow ? clampPercent(pool.shortWindow.remainingPercent) : null;
  const weeklyPct = pool.weeklyWindow ? clampPercent(pool.weeklyWindow.remainingPercent) : null;
  const shortTarget: TrayProviderId | undefined =
    pool.id === "antigravity_3p" && pool.shortWindow !== null ? "antigravity_3p" : undefined;
  const weeklyTarget: TrayProviderId | undefined =
    pool.id === "antigravity_3p" && pool.weeklyWindow !== null
      ? "antigravity_3p_weekly"
      : pool.id === "gemini_argon" && pool.weeklyWindow !== null
        ? "gemini_argon_weekly"
        : undefined;
  const canPinTray = Boolean(shortTarget || weeklyTarget);
  const isTrayActive = Boolean(
    (shortTarget && trayProvider === shortTarget)
    || (weeklyTarget && trayProvider === weeklyTarget),
  );
  const primaryPct = shortPct ?? weeklyPct;
  const tier = quotaTier(primaryPct);
  const shortReset = pool.shortWindow ? formatShortReset(pool.shortWindow.resetsAt, new Date(), language) : null;
  const weeklyReset = pool.weeklyWindow ? formatShortReset(pool.weeklyWindow.resetsAt, new Date(), language) : null;

  return (
    <article className={`subpool-tile subpool-tile--${tier}${isTrayActive ? " subpool-tile--tray" : ""}`}>
      <div className="subpool-header">
        <span className="subpool-name" title={pool.displayName}>{pool.displayName}</span>
        {canPinTray ? (
          <TrayWindowSwitch
            shortTarget={shortTarget}
            weeklyTarget={weeklyTarget}
            activeTray={trayProvider}
            targetName={pool.displayName}
            language={language}
            compact
            onSelect={onSelectTrayProvider}
          />
        ) : null}
      </div>

      {shortPct !== null ? (
        <div className="subpool-metric-row">
          <div className="subpool-metric-main">
            <span className="subpool-tag">5h</span>
            <strong aria-label={`${pool.displayName} ${t.shortAvailableLabel(shortPct)}`}>{shortPct}%</strong>
          </div>
          {shortReset ? <span className="subpool-reset">{shortReset}</span> : null}
        </div>
      ) : null}

      {primaryPct !== null ? (
        <div className="mini-progress" aria-hidden="true">
          <span style={{ width: `${primaryPct}%` }} />
        </div>
      ) : null}

      {weeklyPct !== null ? (
        <div className="subpool-metric-row subpool-metric-row--weekly">
          <div className="subpool-metric-main">
            <span className="subpool-tag">{t.weeklyLabel}</span>
            <strong aria-label={`${pool.displayName} ${t.weeklyAvailableLabel(weeklyPct)}`}>{weeklyPct}%</strong>
          </div>
          <span className="subpool-reset">
            {weeklyReset ?? (weeklyPct === 100 ? t.fullQuota : "")}
          </span>
        </div>
      ) : null}
    </article>
  );
}

function InlineError({
  snapshot,
  staleExpired,
  language,
  onRefresh,
  onOpenAntigravity,
}: {
  snapshot: ProviderSnapshot;
  staleExpired: boolean;
  language: Language;
  onRefresh?: () => void;
  onOpenAntigravity?: () => void;
}) {
  const t = copy[language];
  const message = localizedBackendMessage(snapshot.message, snapshot.provider, language);
  const title = snapshot.status === "signed_out"
    ? (snapshot.provider === "gemini" ? t.signedInAntigravity : t.signedInCodex)
    : staleExpired
      ? t.staleExpired
      : t.temporarilyUnavailable;
  const showOpenAntigravity = snapshot.provider === "gemini" && Boolean(onOpenAntigravity);

  return (
    <div className={`inline-error inline-error--${snapshot.status}`} aria-live="polite">
      <div className="inline-error-main">
        <div className="inline-error-icon" aria-hidden="true">
          <StatusIcon status={snapshot.status} expired={staleExpired} />
        </div>
        <div className="inline-error-copy">
          <strong>{title}</strong>
          <p>{message ?? t.errorUnavailable}</p>
        </div>
        {onRefresh ? (
          <button
            type="button"
            className="inline-refresh-button"
            onMouseDown={(event) => event.stopPropagation()}
            onClick={onRefresh}
            aria-label={t.refreshQuota}
            title={t.refreshQuota}
          >
            <ArrowClockwise />
          </button>
        ) : null}
      </div>
      {showOpenAntigravity ? (
        <div className="inline-error-actions" onMouseDown={(event) => event.stopPropagation()}>
          <button
            type="button"
            className="inline-action-button"
            onClick={onOpenAntigravity}
          >
            <ArrowSquareOut />
            <span>{t.openAntigravity}</span>
          </button>
        </div>
      ) : null}
    </div>
  );
}

export const QuotaCard = memo(function QuotaCard({
  snapshots,
  preferences,
  onSelectTrayProvider,
  onToggleService,
  onLock,
  onLanguage,
  onDrag,
  onHover,
  onRefresh,
  onOpenAntigravity,
  refreshing = false,
  notice = null,
  initialShowCreditTip = false,
}: CardProps) {
  const [showCreditTip, setShowCreditTip] = useState(initialShowCreditTip);
  const [showServiceMenu, setShowServiceMenu] = useState(false);
  const language = normalizeLanguage(preferences.language);
  const visibility = normalizeProviderVisibility(preferences.showCodex, preferences.showAntigravity);
  const singleService = visibility.showCodex !== visibility.showAntigravity;
  const trayProvider = normalizeTrayProvider(preferences.trayProvider);
  const t = copy[language];

  const codex = snapshots.find((item) => item.provider === "codex") ?? {
    provider: "codex" as const,
    displayName: "CODEX",
    plan: null,
    shortWindow: null,
    weeklyWindow: null,
    extraPools: [],
    resetCredits: null,
    resetCreditExpiresAt: [],
    updatedAt: new Date().toISOString(),
    status: "unavailable" as const,
    message: null,
  };
  const gemini = snapshots.find((item) => item.provider === "gemini") ?? {
    provider: "gemini" as const,
    displayName: "GEMINI",
    plan: null,
    shortWindow: null,
    weeklyWindow: null,
    extraPools: [],
    resetCredits: null,
    resetCreditExpiresAt: [],
    updatedAt: new Date().toISOString(),
    status: "unavailable" as const,
    message: null,
  };

  const codexState = isSnapshotAvailable(codex);
  const geminiState = isSnapshotAvailable(gemini);
  const codexPrimary = codex.shortWindow ? clampPercent(codex.shortWindow.remainingPercent) : null;
  const codexWeekly = codex.weeklyWindow ? clampPercent(codex.weeklyWindow.remainingPercent) : null;
  const codexWeeklyOnly = codexState.available && codex.shortWindow === null && codex.weeklyWindow !== null;
  const codexHero = codexWeeklyOnly ? codexWeekly : codexPrimary;
  const codexHeroLabel = codexWeeklyOnly
    ? (codexHero === null ? t.weeklyUnavailable : t.weeklyAvailableLabel(codexHero))
    : (codexPrimary === null ? t.shortUnavailable : t.shortAvailableLabel(codexPrimary));
  const codexShortTarget: TrayProviderId | undefined =
    !codexState.available || codex.shortWindow !== null ? "codex" : undefined;
  const codexWeeklyTarget: TrayProviderId | undefined =
    !codexState.available || codex.weeklyWindow !== null ? "codex_weekly" : undefined;

  const geminiPrimary = gemini.shortWindow ? clampPercent(gemini.shortWindow.remainingPercent) : null;
  const geminiWeekly = gemini.weeklyWindow ? clampPercent(gemini.weeklyWindow.remainingPercent) : null;
  const geminiWeeklyOnly = geminiState.available && gemini.shortWindow === null && gemini.weeklyWindow !== null;
  const geminiHero = geminiWeeklyOnly ? geminiWeekly : geminiPrimary;
  const geminiHeroLabel = geminiWeeklyOnly
    ? (geminiHero === null ? t.weeklyUnavailable : t.weeklyAvailableLabel(geminiHero))
    : (geminiPrimary === null ? t.shortUnavailable : t.shortAvailableLabel(geminiPrimary));
  const geminiShortTarget: TrayProviderId | undefined =
    !geminiState.available || gemini.shortWindow !== null ? "gemini" : undefined;
  const geminiWeeklyTarget: TrayProviderId | undefined =
    !geminiState.available || gemini.weeklyWindow !== null ? "gemini_weekly" : undefined;

  const activeSelection = resolveTraySelection(
    snapshots,
    trayProvider,
    visibility.showCodex,
    visibility.showAntigravity,
  );
  const effectiveTray = activeSelection.effectiveTray;
  const fallbackHero = visibility.showCodex && visibility.showAntigravity
    ? (codexHero ?? geminiHero)
    : visibility.showCodex
      ? codexHero
      : geminiHero;
  const cardTier = quotaTier(
    activeSelection.primaryWindow ? clampPercent(activeSelection.primaryWindow.remainingPercent) : fallbackHero,
  );

  const creditExpirations = useMemo(
    () => (codex.resetCreditExpiresAt ?? []).map((value, index) => t.creditItem(index, formatDateTime(value, language))),
    [codex.resetCreditExpiresAt, language, t],
  );
  const canViewCreditDetails = codex.resetCredits !== null && codex.resetCredits > 0;

  const statusMeta = (snapshot: ProviderSnapshot) => {
    const state = snapshot.status === "ok" ? "ok" : snapshot.status === "stale" ? "stale" : "error";
    const label = snapshot.status === "ok"
      ? t.dataSynced
      : snapshot.status === "stale"
        ? t.dataStale
        : snapshot.status === "signed_out"
          ? t.notSignedIn
          : t.unavailableStatus;
    return { state, label };
  };

  const codexMeta = statusMeta(codex);
  const geminiMeta = statusMeta(gemini);
  const geminiResetShort = gemini.weeklyWindow ? formatShortReset(gemini.weeklyWindow.resetsAt, new Date(), language) : null;
  const extraPools = gemini.extraPools ?? [];

  return (
    <main
      className={`quota-card quota-overview quota-card--${cardTier}${singleService ? " quota-card--single" : ""}`}
      onMouseEnter={() => onHover(true)}
      onMouseLeave={() => onHover(false)}
      onMouseDown={(event) => { if (event.button === 0) void onDrag(); }}
    >
      <div className="aurora" aria-hidden="true" />
      {notice ? <p className="operation-notice" role="status">{notice}</p> : null}

      <header className="overview-topbar">
        <div className="brand-title">
          <span className="brand-dot" aria-hidden="true" />
          <strong>QUOTAPULSE</strong>
        </div>
        {!preferences.locked ? (
          <nav className="card-actions" aria-label={t.controls} onMouseDown={(event) => event.stopPropagation()}>
            {onToggleService ? (
              <button
                type="button"
                className={showServiceMenu ? "is-active" : ""}
                onClick={() => setShowServiceMenu((value) => !value)}
                aria-expanded={showServiceMenu}
                aria-label={t.manageServices}
                title={t.manageServices}
              >
                <SlidersHorizontal />
              </button>
            ) : null}
            {onRefresh ? (
              <button
                type="button"
                className={refreshing ? "is-spinning" : ""}
                onClick={onRefresh}
                aria-label={t.refreshQuota}
                title={t.refreshQuota}
              >
                <ArrowClockwise />
              </button>
            ) : null}
            <button
              type="button"
              className="language-button"
              onClick={onLanguage}
              aria-label={t.switchLanguage}
              title={t.switchLanguage}
            >
              {language === "en" ? "中" : "EN"}
            </button>
            <button
              type="button"
              onClick={onLock}
              aria-label={preferences.alwaysOnTop ? t.pinOff : t.pinOn}
              title={preferences.alwaysOnTop ? t.pinOff : t.pinOn}
            >
              {preferences.alwaysOnTop ? <PushPin /> : <PushPinSlash />}
            </button>
            {showServiceMenu && onToggleService ? (
              <div className="service-popover" role="group" aria-label={t.visibleServicesTitle}>
                <p className="service-popover-title">{t.visibleServicesTitle}</p>
                <button
                  type="button"
                  className={`service-option${visibility.showCodex ? " is-active" : ""}`}
                  aria-pressed={visibility.showCodex}
                  disabled={visibility.showCodex && !visibility.showAntigravity}
                  title={visibility.showCodex && !visibility.showAntigravity ? t.keepAtLeastOneService : undefined}
                  onClick={() => onToggleService("codex")}
                >
                  <span>{t.codexServiceLabel}</span>
                  <span className="service-option-check" aria-hidden="true">
                    {visibility.showCodex ? <Check weight="bold" /> : null}
                  </span>
                </button>
                <button
                  type="button"
                  className={`service-option${visibility.showAntigravity ? " is-active" : ""}`}
                  aria-pressed={visibility.showAntigravity}
                  disabled={visibility.showAntigravity && !visibility.showCodex}
                  title={visibility.showAntigravity && !visibility.showCodex ? t.keepAtLeastOneService : undefined}
                  onClick={() => onToggleService("gemini")}
                >
                  <span>{t.antigravityServiceLabel}</span>
                  <span className="service-option-check" aria-hidden="true">
                    {visibility.showAntigravity ? <Check weight="bold" /> : null}
                  </span>
                </button>
              </div>
            ) : null}
          </nav>
        ) : null}
      </header>

      <div className="overview-sections">
        {visibility.showCodex ? (
          <section
            className={`provider-panel provider-panel--codex provider-panel--${quotaTier(codexHero)}${effectiveTray === "codex" || effectiveTray === "codex_weekly" ? " is-tray-source" : ""}`}
            aria-label="Codex"
          >
            <div className="provider-panel-head">
              <div className="provider-identity">
                <ProviderMark provider="codex" />
                <div className="provider-title-group">
                  <span className="provider-name">{codex.displayName}</span>
                  <span className="provider-plan">{codex.plan ?? t.accountFallback}</span>
                </div>
                <span
                  className={`usage-dot usage-dot--${codexMeta.state}`}
                  role="status"
                  aria-label={`Codex ${codexMeta.label}`}
                  title={codexMeta.label}
                />
              </div>
              {codexShortTarget || codexWeeklyTarget ? (
                <TrayWindowSwitch
                  shortTarget={codexShortTarget}
                  weeklyTarget={codexWeeklyTarget}
                  activeTray={effectiveTray}
                  targetName="Codex (GPT)"
                  language={language}
                  onSelect={onSelectTrayProvider}
                />
              ) : null}
            </div>

            {codexState.available ? (
              <>
                <div className="panel-metrics-row">
                  <div className="panel-primary" aria-label={codexHeroLabel}>
                    <div className="panel-primary-value">
                      <span>{codexHero ?? "--"}</span>
                      <small>{codexHero === null ? "" : "%"}</small>
                    </div>
                    <p className="panel-primary-caption">
                      {codexWeeklyOnly
                        ? `${t.weeklyRemaining} · ${formatResetTime(codex.weeklyWindow?.resetsAt ?? null, new Date(), language)}`
                        : `${t.shortRemaining} · ${codex.shortWindow ? formatResetTime(codex.shortWindow.resetsAt, new Date(), language) : t.shortUnavailable}`}
                    </p>
                  </div>
                  {!codexWeeklyOnly ? (
                    <div className="panel-weekly">
                      <span className="panel-weekly-label">
                        {codex.weeklyWindow ? t.weeklyUntil(formatResetDate(codex.weeklyWindow.resetsAt, language)) : t.weeklyUnavailable}
                      </span>
                      <strong aria-label={codexWeekly === null ? t.weeklyUnavailable : t.weeklyAvailableLabel(codexWeekly)}>
                        {codexWeekly ?? "--"}<small>{codexWeekly === null ? "" : "%"}</small>
                      </strong>
                    </div>
                  ) : null}
                </div>

                {codexHero === null ? (
                  <div className="progress progress--unavailable" aria-hidden="true" />
                ) : (
                  <div
                    className="progress"
                    role="progressbar"
                    aria-label={`Codex ${codexHeroLabel}`}
                    aria-valuemin={0}
                    aria-valuemax={100}
                    aria-valuenow={codexHero}
                  >
                    <span style={{ width: `${codexHero}%` }} />
                  </div>
                )}

                <div className="panel-meta-row" onMouseDown={(event) => event.stopPropagation()}>
                  <div className={`reset-credit-row${codex.status === "stale" ? " reset-credit-row--stale" : ""}`}>
                    <span>{codex.resetCredits === null ? t.resetCreditUnknown : t.resetCredits(codex.resetCredits)}</span>
                    {canViewCreditDetails ? (
                      <button
                        type="button"
                        className="reset-credit-button"
                        onClick={() => setShowCreditTip((value) => !value)}
                        aria-expanded={showCreditTip}
                        aria-label={t.view}
                      >
                        {t.view}
                      </button>
                    ) : null}
                  </div>
                  {showCreditTip && canViewCreditDetails ? (
                    <div className="reset-credit-tip" role="status">
                      {creditExpirations.length > 0
                        ? creditExpirations.map((item) => <p key={item}>{item}</p>)
                        : <p>{t.noCreditExpiration}</p>}
                    </div>
                  ) : null}
                </div>
              </>
            ) : (
              <InlineError snapshot={codex} staleExpired={codexState.staleExpired} language={language} onRefresh={onRefresh} />
            )}
          </section>
        ) : null}

        {visibility.showAntigravity ? (
          <section
            className={`provider-panel provider-panel--gemini provider-panel--${quotaTier(geminiHero)}${effectiveTray === "gemini" || effectiveTray === "gemini_weekly" ? " is-tray-source" : ""}`}
            aria-label="Gemini"
          >
            <div className="provider-panel-head">
              <div className="provider-identity">
                <ProviderMark provider="gemini" />
                <div className="provider-title-group">
                  <span className="provider-name">{t.geminiModels}</span>
                  {gemini.plan ? <span className="provider-plan">{gemini.plan}</span> : null}
                </div>
                <span
                  className={`usage-dot usage-dot--${geminiMeta.state}`}
                  role="status"
                  aria-label={`Gemini ${geminiMeta.label}`}
                  title={geminiMeta.label}
                />
              </div>
              {geminiShortTarget || geminiWeeklyTarget ? (
                <TrayWindowSwitch
                  shortTarget={geminiShortTarget}
                  weeklyTarget={geminiWeeklyTarget}
                  activeTray={effectiveTray}
                  targetName="Gemini"
                  language={language}
                  onSelect={onSelectTrayProvider}
                />
              ) : null}
            </div>

            {geminiState.available ? (
              <>
                <div className="panel-metrics-row">
                  <div className="panel-primary" aria-label={geminiHeroLabel}>
                    <div className="panel-primary-value">
                      <span>{geminiHero ?? "--"}</span>
                      <small>{geminiHero === null ? "" : "%"}</small>
                    </div>
                    <p className="panel-primary-caption">
                      {geminiWeeklyOnly
                        ? `${t.weeklyRemaining} · ${formatResetTime(gemini.weeklyWindow?.resetsAt ?? null, new Date(), language)}`
                        : `${t.shortRemaining} · ${gemini.shortWindow ? formatResetTime(gemini.shortWindow.resetsAt, new Date(), language) : t.shortUnavailable}`}
                    </p>
                  </div>
                  {!geminiWeeklyOnly ? (
                    <div className="panel-weekly">
                      <span className="panel-weekly-label">
                        {gemini.weeklyWindow
                          ? (geminiResetShort ? `${t.weeklyLabel} · ${geminiResetShort}` : t.weeklyLabel)
                          : t.weeklyUnavailable}
                      </span>
                      <strong aria-label={geminiWeekly === null ? t.weeklyUnavailable : t.weeklyAvailableLabel(geminiWeekly)}>
                        {geminiWeekly ?? "--"}<small>{geminiWeekly === null ? "" : "%"}</small>
                      </strong>
                    </div>
                  ) : null}
                </div>

                {geminiHero === null ? (
                  <div className="progress progress--unavailable" aria-hidden="true" />
                ) : (
                  <div
                    className="progress progress--gemini"
                    role="progressbar"
                    aria-label={`Gemini ${geminiHeroLabel}`}
                    aria-valuemin={0}
                    aria-valuemax={100}
                    aria-valuenow={geminiHero}
                  >
                    <span style={{ width: `${geminiHero}%` }} />
                  </div>
                )}

                {extraPools.length > 0 ? (
                  <div className={`subpools-grid${extraPools.length === 1 ? " subpools-grid--single" : ""}`}>
                    {extraPools.map((pool) => (
                      <SubPoolTile
                        key={pool.id}
                        pool={pool}
                        language={language}
                        trayProvider={effectiveTray}
                        onSelectTrayProvider={onSelectTrayProvider}
                      />
                    ))}
                  </div>
                ) : null}
              </>
            ) : (
              <InlineError
                snapshot={gemini}
                staleExpired={geminiState.staleExpired}
                language={language}
                onRefresh={onRefresh}
                onOpenAntigravity={onOpenAntigravity}
              />
            )}
          </section>
        ) : null}
      </div>
    </main>
  );
});

export const QuotaOrb = memo(function QuotaOrb({
  snapshots,
  trayProvider = "codex",
  showCodex = true,
  showAntigravity = true,
  onDrag,
  onHover,
  language = "zh-CN",
}: OrbProps) {
  const [idle, setIdle] = useState(false);
  const idleTimer = useRef<number | null>(null);
  const activeLanguage = normalizeLanguage(language);
  const activeTray = normalizeTrayProvider(trayProvider);
  const t = copy[activeLanguage];

  const selection = resolveTraySelection(snapshots, activeTray, showCodex, showAntigravity);
  const snapshot = selection.snapshot ?? {
    provider: "codex" as const,
    displayName: "CODEX",
    plan: null,
    shortWindow: null,
    weeklyWindow: null,
    extraPools: [],
    resetCredits: null,
    resetCreditExpiresAt: [],
    updatedAt: new Date().toISOString(),
    status: "unavailable" as const,
    message: null,
  };

  const primary = selection.primaryWindow ? clampPercent(selection.primaryWindow.remainingPercent) : null;
  const secondary = selection.secondaryWindow ? clampPercent(selection.secondaryWindow.remainingPercent) : null;
  const tier = quotaTier(primary);
  const available = snapshot.status === "ok" && primary !== null;
  const isCodex = selection.themeKey === "codex";

  const badgeText = isCodex
    ? (selection.resetCredits === null ? "?" : selection.resetCredits > 99 ? "99+" : String(selection.resetCredits))
    : selection.themeKey === "gemini_argon" || (selection.windowKind === "weekly" && secondary === null)
      ? t.trayWeeklyPill
      : (secondary === null ? "?" : `${secondary}`);
  const badgeAria = isCodex
    ? (selection.resetCredits === null ? t.resetCreditUnknown : t.resetCredits(selection.resetCredits))
    : selection.themeKey === "gemini_argon" || (selection.windowKind === "weekly" && secondary === null)
      ? t.weeklyLabel
      : selection.windowKind === "short"
        ? (secondary === null ? t.weeklyUnavailable : t.weeklyAvailableLabel(secondary))
        : (secondary === null ? t.shortUnavailable : t.shortAvailableLabel(secondary));
  const quotaLabel = available
    ? (selection.windowKind === "short" ? t.shortAvailableLabel(primary) : t.weeklyAvailableLabel(primary))
    : snapshot.status === "ok"
      ? (selection.windowKind === "short" ? t.shortUnavailable : t.weeklyUnavailable)
      : localizedBackendMessage(snapshot.message, snapshot.provider, activeLanguage) ?? t.unavailableStatus;

  useEffect(() => {
    idleTimer.current = window.setTimeout(() => setIdle(true), 2000);
    return () => {
      if (idleTimer.current !== null) window.clearTimeout(idleTimer.current);
    };
  }, []);

  const handleMouseEnter = () => {
    if (idleTimer.current !== null) window.clearTimeout(idleTimer.current);
    setIdle(false);
    onHover(true);
  };

  return (
    <main
      className={`quota-orb quota-orb--${selection.themeKey} quota-card--${snapshot.status} quota-card--${tier}${idle ? " quota-orb--idle" : ""}`}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={() => onHover(false)}
      onMouseDown={(event) => { if (event.button === 0) void onDrag(); }}
      aria-label={`${selection.tag} ${quotaLabel}. ${badgeAria}${snapshot.status === "stale" ? `, ${t.dataStale}` : ""}`}
    >
      <div className="aurora" aria-hidden="true" />
      <span
        className={`orb-credit-badge${snapshot.status === "stale" ? " orb-credit-badge--stale" : ""}${badgeText === "?" ? " orb-credit-badge--unknown" : ""}`}
        aria-hidden="true"
      >
        {badgeText}
      </span>
      {available ? (
        <section className="orb-metric">
          <span>{primary}</span>
          <small>%</small>
        </section>
      ) : (
        <section className="orb-unavailable">
          <StatusIcon status={snapshot.status} />
        </section>
      )}
      <span className="orb-provider-tag" aria-hidden="true">{selection.tag}</span>
    </main>
  );
});
