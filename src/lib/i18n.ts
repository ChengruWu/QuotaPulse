import type { Language, TrayProviderId } from "../types";

export const DEFAULT_LANGUAGE: Language = "zh-CN";

export const copy = {
  "zh-CN": {
    accountFallback: "ACCOUNT",
    active: "正在消耗额度",
    shortAvailableLabel: (percent: number) => `5 小时额度剩余 ${percent}%`,
    weeklyAvailableLabel: (percent: number) => `周额度剩余 ${percent}%`,
    clickThroughOff: "取消鼠标穿透",
    controls: "面板控制",
    creditExpiresUnknown: "到期时间未知",
    creditItem: (index: number, label: string) => `第 ${index + 1} 次：${label}`,
    dataSynced: "额度数据已同步",
    dataStale: "额度数据已过期",
    dateUnknown: "日期未知",
    errorUnavailable: "额度接口暂时不可用，将自动重试。",
    fullQuota: "满额",
    geminiModels: "Gemini 模型",
    keepAtLeastOneService: "至少保留一个显示服务",
    loadingQuota: "正在读取额度",
    manageServices: "显示服务设置",
    noCreditExpiration: "当前接口只返回数量，未返回到期时间。",
    notSignedIn: "未登录",
    openAntigravity: "打开 Antigravity",
    signInGemini: "登录 Gemini",
    signingInGemini: "等待浏览器授权…",
    cancelSignIn: "取消",
    pinOff: "取消置顶",
    pinOn: "置顶显示",
    refresh: "刷新",
    refreshQuota: "刷新额度数据",
    resetCreditUnknown: "重置机会未知",
    resetCredits: (count: number) => `${count} 次重置机会`,
    resetInDays: (days: number, hours: number) => `${days} 天 ${hours} 小时后重置`,
    resetInHours: (hours: number, minutes: number) => minutes ? `${hours} 小时 ${minutes} 分钟后重置` : `${hours} 小时后重置`,
    resetInMinutes: (minutes: number) => `${minutes} 分钟后重置`,
    resetShortDays: (days: number, hours: number) => `${days}d ${hours}h`,
    resetShortHours: (hours: number, minutes: number) => minutes ? `${hours}h ${minutes}m` : `${hours}h`,
    resetShortMinutes: (minutes: number) => `${minutes}m`,
    resetTimeUnknown: "重置时间未知",
    resetUpdating: "正在更新额度",
    shortRemaining: "5h 剩余",
    shortUnavailable: "5h 额度暂不可用",
    signedInCodex: "请先登录 Codex",
    signedInAntigravity: "请先登录 Antigravity",
    staleExpired: "额度数据已过期",
    switchLanguage: "Switch to English",
    temporarilyUnavailable: "暂时无法读取",
    visibleServicesTitle: "显示服务",
    codexServiceLabel: "Codex (GPT)",
    antigravityServiceLabel: "Antigravity (Gemini)",
    trayActive: "托盘中",
    trayDisplay: "设到托盘",
    trayShortPill: "5h",
    trayWeeklyPill: "周",
    trayDisplayAction: (name: string) => `在任务栏托盘显示 ${name} 的 5 小时额度`,
    trayDisplaySelected: (name: string) => `当前已在任务栏托盘显示 ${name} 的 5 小时额度`,
    trayDisplayWeeklyAction: (name: string) => `在任务栏托盘显示 ${name} 的周额度`,
    trayDisplayWeeklySelected: (name: string) => `当前已在任务栏托盘显示 ${name} 的周额度`,
    unavailableStatus: "暂时无法读取额度",
    view: "查看",
    weeklyLabel: "周额度",
    weeklyRemaining: "本周剩余",
    weeklyUnavailable: "周额度暂不可用",
    weeklyUntil: (date: string) => `本周 · 至 ${date}`,
  },
  en: {
    accountFallback: "ACCOUNT",
    active: "Quota in use",
    shortAvailableLabel: (percent: number) => `5-hour quota remaining ${percent}%`,
    weeklyAvailableLabel: (percent: number) => `Weekly quota remaining ${percent}%`,
    clickThroughOff: "Disable click-through",
    controls: "Panel controls",
    creditExpiresUnknown: "Expiration unknown",
    creditItem: (index: number, label: string) => `Credit ${index + 1}: ${label}`,
    dataSynced: "Quota data synced",
    dataStale: "Quota data is stale",
    dateUnknown: "Date unknown",
    errorUnavailable: "Quota service is temporarily unavailable. It will retry automatically.",
    fullQuota: "Full",
    geminiModels: "Gemini Models",
    loadingQuota: "Reading quota",
    noCreditExpiration: "The current API only returned the count, not the expiration time.",
    notSignedIn: "Signed out",
    openAntigravity: "Open Antigravity",
    signInGemini: "Sign in",
    signingInGemini: "Finish in browser…",
    cancelSignIn: "Cancel",
    pinOff: "Disable always on top",
    pinOn: "Keep always on top",
    refresh: "Refresh",
    refreshQuota: "Refresh quota data",
    resetCreditUnknown: "Reset credits unknown",
    resetCredits: (count: number) => `${count} reset ${count === 1 ? "credit" : "credits"}`,
    resetInDays: (days: number, hours: number) => `resets in ${days}d ${hours}h`,
    resetInHours: (hours: number, minutes: number) => minutes ? `resets in ${hours}h ${minutes}m` : `resets in ${hours}h`,
    resetInMinutes: (minutes: number) => `resets in ${minutes}m`,
    resetShortDays: (days: number, hours: number) => `${days}d ${hours}h`,
    resetShortHours: (hours: number, minutes: number) => minutes ? `${hours}h ${minutes}m` : `${hours}h`,
    resetShortMinutes: (minutes: number) => `${minutes}m`,
    resetTimeUnknown: "Reset time unknown",
    resetUpdating: "Updating quota",
    shortRemaining: "5h left",
    shortUnavailable: "5h quota unavailable",
    signedInCodex: "Sign in to Codex",
    signedInAntigravity: "Sign in to Antigravity",
    staleExpired: "Quota data expired",
    switchLanguage: "Switch to Chinese",
    temporarilyUnavailable: "Unavailable",
    manageServices: "Service visibility",
    visibleServicesTitle: "Visible services",
    codexServiceLabel: "Codex (GPT)",
    antigravityServiceLabel: "Antigravity (Gemini)",
    keepAtLeastOneService: "Keep at least one service enabled",
    trayActive: "In Tray",
    trayDisplay: "Tray",
    trayShortPill: "5h",
    trayWeeklyPill: "7d",
    trayDisplayAction: (name: string) => `Show ${name} 5-hour quota in taskbar tray`,
    trayDisplaySelected: (name: string) => `Showing ${name} 5-hour quota in taskbar tray`,
    trayDisplayWeeklyAction: (name: string) => `Show ${name} weekly quota in taskbar tray`,
    trayDisplayWeeklySelected: (name: string) => `Showing ${name} weekly quota in taskbar tray`,
    unavailableStatus: "Quota temporarily unavailable",
    view: "View",
    weeklyLabel: "Weekly",
    weeklyRemaining: "Weekly left",
    weeklyUnavailable: "Weekly quota unavailable",
    weeklyUntil: (date: string) => `Weekly · until ${date}`,
  },
} as const;

export function normalizeLanguage(value: unknown): Language {
  return value === "en" ? "en" : DEFAULT_LANGUAGE;
}

export function isCodexTrayProvider(value: TrayProviderId): boolean {
  return value === "codex" || value === "codex_weekly";
}

export function normalizeProviderVisibility(
  showCodex: unknown,
  showAntigravity: unknown,
): { showCodex: boolean; showAntigravity: boolean } {
  const codex = showCodex !== false;
  const antigravity = showAntigravity !== false;
  if (!codex && !antigravity) {
    return { showCodex: true, showAntigravity: true };
  }
  return { showCodex: codex, showAntigravity: antigravity };
}

export function normalizeTrayProvider(value: unknown): TrayProviderId {
  if (
    value === "codex"
    || value === "codex_weekly"
    || value === "gemini"
    || value === "gemini_weekly"
    || value === "antigravity_3p"
    || value === "antigravity_3p_weekly"
    || value === "gemini_argon_weekly"
  ) {
    return value;
  }
  if (value === "gemini_argon") return "gemini_argon_weekly";
  return "codex";
}

export function normalizeTrayProviderForVisibility(
  value: unknown,
  showCodex: boolean,
  showAntigravity: boolean,
): TrayProviderId {
  const normalized = normalizeTrayProvider(value);
  const visibility = normalizeProviderVisibility(showCodex, showAntigravity);
  if (!visibility.showCodex && isCodexTrayProvider(normalized)) {
    return "gemini";
  }
  if (!visibility.showAntigravity && !isCodexTrayProvider(normalized)) {
    return "codex";
  }
  return normalized;
}

export function nextLanguage(language: Language): Language {
  return language === "en" ? "zh-CN" : "en";
}
