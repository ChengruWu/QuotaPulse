// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderSnapshot, WidgetPreferences } from "../types";
import {
  EXPANDED_HEIGHT,
  EXPANDED_HEIGHT_ANTIGRAVITY_COMPACT,
  EXPANDED_HEIGHT_ANTIGRAVITY_WITH_POOLS,
  EXPANDED_HEIGHT_CODEX_ONLY,
  cancelAntigravitySignIn,
  expandedHeightFor,
  fetchSnapshots,
  getAntigravitySignInState,
  listenDesktopEvents,
  signInAntigravity,
} from "./bridge";

const mocks = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  unlisteners: [] as ReturnType<typeof vi.fn>[],
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, callback: (event: { payload: unknown }) => void) => {
    mocks.listeners.set(name, callback);
    const unlisten = vi.fn();
    mocks.unlisteners.push(unlisten);
    return unlisten;
  }),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));

const snapshot: ProviderSnapshot = {
  provider: "codex",
  displayName: "CODEX",
  plan: "PRO",
  shortWindow: null,
  weeklyWindow: { remainingPercent: 74, resetsAt: null, windowSeconds: 604_800 },
  resetCredits: 1,
  resetCreditExpiresAt: [],
  updatedAt: "2026-08-09T12:00:00Z",
  status: "ok",
  message: null,
};

beforeEach(() => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  mocks.listeners.clear();
  mocks.unlisteners.length = 0;
  mocks.invoke.mockReset();
});

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});

describe("desktop bridge events", () => {
  it("delivers quota and presentation changes while the webview is hidden", async () => {
    const preferences: WidgetPreferences = {
      locked: false,
      alwaysOnTop: true,
      pinnedProvider: null,
      autoRotateSeconds: 12,
      language: "zh-CN",
      trayProvider: "gemini",
      showCodex: true,
      showAntigravity: true,
    };
    const handlers = {
      onPreferences: vi.fn(),
      onRefresh: vi.fn(),
      onSnapshots: vi.fn(),
      onPresentation: vi.fn(),
      onAntigravitySignInState: vi.fn(),
    };
    const cleanup = await listenDesktopEvents(handlers);

    mocks.listeners.get("snapshots-changed")?.({ payload: [snapshot] });
    mocks.listeners.get("window-presentation-changed")?.({ payload: "popup" });
    mocks.listeners.get("preferences-changed")?.({ payload: preferences });
    mocks.listeners.get("antigravity-signin-state")?.({ payload: true });

    expect(handlers.onSnapshots).toHaveBeenCalledWith([snapshot]);
    expect(handlers.onPresentation).toHaveBeenCalledWith("popup");
    expect(handlers.onPreferences).toHaveBeenCalledWith(preferences);
    expect(handlers.onAntigravitySignInState).toHaveBeenCalledWith(true);

    cleanup();
    expect(mocks.unlisteners).toHaveLength(5);
    expect(mocks.unlisteners.every((unlisten) => unlisten.mock.calls.length === 1)).toBe(true);
  });

  it("invokes direct Antigravity sign-in commands", async () => {
    mocks.invoke.mockResolvedValueOnce([snapshot]);
    mocks.invoke.mockResolvedValueOnce(undefined);
    mocks.invoke.mockResolvedValueOnce(true);

    await expect(signInAntigravity()).resolves.toEqual([snapshot]);
    await expect(cancelAntigravitySignIn()).resolves.toBeUndefined();
    await expect(getAntigravitySignInState()).resolves.toBe(true);

    expect(mocks.invoke).toHaveBeenNthCalledWith(1, "sign_in_antigravity");
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, "cancel_antigravity_sign_in");
    expect(mocks.invoke).toHaveBeenNthCalledWith(3, "get_antigravity_sign_in_state");
  });

  it("uses the forced refresh command only for manual refreshes", async () => {
    mocks.invoke.mockResolvedValue([snapshot]);
    await fetchSnapshots(false);
    await fetchSnapshots(true);
    expect(mocks.invoke).toHaveBeenNthCalledWith(1, "get_snapshots");
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, "refresh_snapshots");
  });

  it("computes adaptive expanded heights for dual and single-service modes", () => {
    expect(expandedHeightFor({ showCodex: true, showAntigravity: true }, [snapshot])).toBe(EXPANDED_HEIGHT);
    expect(expandedHeightFor({ showCodex: true, showAntigravity: false }, [snapshot])).toBe(EXPANDED_HEIGHT_CODEX_ONLY);
    expect(
      expandedHeightFor(
        { showCodex: false, showAntigravity: true },
        [{ ...snapshot, provider: "gemini", extraPools: [] }],
      ),
    ).toBe(EXPANDED_HEIGHT_ANTIGRAVITY_COMPACT);
    expect(
      expandedHeightFor(
        { showCodex: false, showAntigravity: true },
        [
          {
            ...snapshot,
            provider: "gemini",
            extraPools: [
              {
                id: "antigravity_3p",
                displayName: "Claude & GPT",
                shortWindow: null,
                weeklyWindow: { remainingPercent: 55, resetsAt: null, windowSeconds: 604_800 },
              },
            ],
          },
        ],
      ),
    ).toBe(EXPANDED_HEIGHT_ANTIGRAVITY_WITH_POOLS);
  });
});
