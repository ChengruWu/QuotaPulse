export type ProviderId = "codex" | "gemini";
export type TrayProviderId =
  | "codex"
  | "codex_weekly"
  | "gemini"
  | "gemini_weekly"
  | "antigravity_3p"
  | "antigravity_3p_weekly"
  | "gemini_argon_weekly";
export type SnapshotStatus = "ok" | "stale" | "loading" | "unavailable" | "signed_out";
export type Language = "zh-CN" | "en";

export interface UsageWindow {
  remainingPercent: number;
  resetsAt: string | null;
  windowSeconds: number;
}

export interface QuotaPool {
  id: string;
  displayName: string;
  shortWindow: UsageWindow | null;
  weeklyWindow: UsageWindow | null;
}

export interface ProviderSnapshot {
  provider: ProviderId;
  displayName: string;
  plan: string | null;
  shortWindow: UsageWindow | null;
  weeklyWindow: UsageWindow | null;
  extraPools?: QuotaPool[];
  resetCredits: number | null;
  resetCreditExpiresAt?: string[];
  updatedAt: string;
  status: SnapshotStatus;
  message: string | null;
}

export interface WidgetPreferences {
  locked: boolean;
  alwaysOnTop: boolean;
  pinnedProvider: ProviderId | null;
  autoRotateSeconds: number;
  language: Language;
  trayProvider: TrayProviderId;
  showCodex: boolean;
  showAntigravity: boolean;
}
