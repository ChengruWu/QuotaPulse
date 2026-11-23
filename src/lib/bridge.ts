import type { ProviderSnapshot, WidgetPreferences } from "../types";
import { positionAfterAnchoredResize } from "./windowGeometry";

export type WindowPresentation = "tray" | "popup" | "floating";

export const EXPANDED_WIDTH = 360;
export const EXPANDED_HEIGHT = 460;
export const EXPANDED_HEIGHT_CODEX_ONLY = 240;
export const EXPANDED_HEIGHT_ANTIGRAVITY_COMPACT = 250;
export const EXPANDED_HEIGHT_ANTIGRAVITY_WITH_POOLS = 316;
export const ORB_SIZE = 100;

export function expandedHeightFor(
  preferences: Pick<WidgetPreferences, "showCodex" | "showAntigravity">,
  snapshots: ProviderSnapshot[],
): number {
  const showCodex = preferences.showCodex !== false;
  const showAntigravity = preferences.showAntigravity !== false;
  if (showCodex && !showAntigravity) {
    return EXPANDED_HEIGHT_CODEX_ONLY;
  }
  if (!showCodex && showAntigravity) {
    const gemini = snapshots.find((item) => item.provider === "gemini");
    const hasSubpools = Boolean(
      gemini && gemini.status !== "signed_out" && (gemini.extraPools?.length ?? 0) > 0,
    );
    return hasSubpools ? EXPANDED_HEIGHT_ANTIGRAVITY_WITH_POOLS : EXPANDED_HEIGHT_ANTIGRAVITY_COMPACT;
  }
  return EXPANDED_HEIGHT;
}

const defaultPreferences: WidgetPreferences = {
  locked: false,
  alwaysOnTop: true,
  pinnedProvider: null,
  autoRotateSeconds: 12,
  language: "zh-CN",
  trayProvider: "codex",
  showCodex: true,
  showAntigravity: true,
};

const mockSnapshots: ProviderSnapshot[] = [
  {
    provider: "codex",
    displayName: "CODEX",
    plan: "PLUS",
    shortWindow: { remainingPercent: 74, resetsAt: new Date(Date.now() + 78 * 60_000).toISOString(), windowSeconds: 18_000 },
    weeklyWindow: { remainingPercent: 42, resetsAt: new Date(Date.now() + 3.2 * 86_400_000).toISOString(), windowSeconds: 604_800 },
    extraPools: [],
    resetCredits: 1,
    resetCreditExpiresAt: [new Date(Date.now() + 9 * 86_400_000).toISOString()],
    updatedAt: new Date().toISOString(),
    status: "ok",
    message: null,
  },
  {
    provider: "gemini",
    displayName: "GEMINI",
    plan: "PRO",
    shortWindow: { remainingPercent: 93, resetsAt: new Date(Date.now() + 183 * 60_000).toISOString(), windowSeconds: 18_000 },
    weeklyWindow: { remainingPercent: 97, resetsAt: new Date(Date.now() + 6.9 * 86_400_000).toISOString(), windowSeconds: 604_800 },
    extraPools: [
      {
        id: "antigravity_3p",
        displayName: "Claude & GPT",
        shortWindow: { remainingPercent: 0, resetsAt: new Date(Date.now() + 232 * 60_000).toISOString(), windowSeconds: 18_000 },
        weeklyWindow: { remainingPercent: 44, resetsAt: new Date(Date.now() + 6.9 * 86_400_000).toISOString(), windowSeconds: 604_800 },
      },
      {
        id: "gemini_argon",
        displayName: "Gemini 4 Argon",
        shortWindow: null,
        weeklyWindow: { remainingPercent: 100, resetsAt: null, windowSeconds: 604_800 },
      },
    ],
    resetCredits: null,
    resetCreditExpiresAt: [],
    updatedAt: new Date().toISOString(),
    status: "ok",
    message: null,
  },
];

export const isTauri = () => "__TAURI_INTERNALS__" in window;

export async function fetchSnapshots(force = false): Promise<ProviderSnapshot[]> {
  if (!isTauri()) return mockSnapshots;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<ProviderSnapshot[]>(force ? "refresh_snapshots" : "get_snapshots");
}

export async function getPreferences(): Promise<WidgetPreferences> {
  if (!isTauri()) return defaultPreferences;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WidgetPreferences>("get_preferences");
}

export async function updatePreferences(value: WidgetPreferences): Promise<void> {
  if (!isTauri()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("set_preferences", { preferences: value });
}

export async function setClickThrough(locked: boolean): Promise<WidgetPreferences> {
  if (!isTauri()) return { ...defaultPreferences, locked };
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WidgetPreferences>("set_widget_locked", { locked });
}

export async function setAlwaysOnTop(alwaysOnTop: boolean): Promise<WidgetPreferences> {
  if (!isTauri()) return { ...defaultPreferences, alwaysOnTop };
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WidgetPreferences>("set_widget_always_on_top", { alwaysOnTop });
}

export async function startDragging(): Promise<void> {
  if (!isTauri()) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().startDragging();
}

export async function openAntigravity(): Promise<void> {
  if (!isTauri()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("open_antigravity");
}

let widgetResize = Promise.resolve();

async function resizeWidget(expanded: boolean, expandedHeight: number): Promise<void> {
  const { currentMonitor, getCurrentWindow, LogicalSize, PhysicalPosition } = await import("@tauri-apps/api/window");
  const window = getCurrentWindow();
  const [position, currentSize, monitor] = await Promise.all([
    window.outerPosition(),
    window.outerSize(),
    currentMonitor(),
  ]);
  const size = expanded
    ? new LogicalSize(EXPANDED_WIDTH, expandedHeight)
    : new LogicalSize(ORB_SIZE, ORB_SIZE);

  await window.setSize(size);
  if (!monitor) return;

  const resizedWindow = await window.outerSize();
  const nextPosition = positionAfterAnchoredResize(
    position,
    currentSize,
    { x: monitor.position.x, y: monitor.position.y, width: monitor.size.width, height: monitor.size.height },
    resizedWindow,
    Math.max(1, Math.round(16 * monitor.scaleFactor)),
  );

  if (nextPosition.x !== position.x || nextPosition.y !== position.y) {
    await window.setPosition(new PhysicalPosition(nextPosition.x, nextPosition.y));
  }
}

export function setWidgetExpanded(expanded: boolean, expandedHeight = EXPANDED_HEIGHT): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  const operation = widgetResize.catch(() => undefined).then(() => resizeWidget(expanded, expandedHeight));
  widgetResize = operation;
  return operation;
}

export async function listenDesktopEvents(handlers: {
  onPreferences: (value: WidgetPreferences) => void;
  onRefresh: () => void;
  onSnapshots: (value: ProviderSnapshot[]) => void;
  onPresentation: (value: WindowPresentation) => void;
}): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  const unlistenPreferences = await listen<WidgetPreferences>("preferences-changed", (event) => handlers.onPreferences(event.payload));
  const unlistenRefresh = await listen("refresh-requested", handlers.onRefresh);
  const unlistenSnapshots = await listen<ProviderSnapshot[]>("snapshots-changed", (event) => handlers.onSnapshots(event.payload));
  const unlistenPresentation = await listen<WindowPresentation>("window-presentation-changed", (event) => handlers.onPresentation(event.payload));
  return () => { unlistenPreferences(); unlistenRefresh(); unlistenSnapshots(); unlistenPresentation(); };
}
