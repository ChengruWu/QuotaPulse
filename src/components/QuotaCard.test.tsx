// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ProviderId, ProviderSnapshot, TrayProviderId, WidgetPreferences } from "../types";
import { QuotaCard, QuotaOrb } from "./QuotaCard";

const preferences: WidgetPreferences = {
  locked: false,
  alwaysOnTop: true,
  pinnedProvider: null,
  autoRotateSeconds: 12,
  language: "en",
  trayProvider: "codex",
  showCodex: true,
  showAntigravity: true,
};

const codexSnapshot: ProviderSnapshot = {
  provider: "codex",
  displayName: "CODEX",
  plan: "PRO",
  shortWindow: {
    remainingPercent: 74,
    resetsAt: "2026-08-07T02:00:00Z",
    windowSeconds: 18_000,
  },
  weeklyWindow: {
    remainingPercent: 42,
    resetsAt: "2026-08-08T00:00:00Z",
    windowSeconds: 604_800,
  },
  extraPools: [],
  resetCredits: 1,
  resetCreditExpiresAt: ["2026-08-03T00:00:00Z"],
  updatedAt: "2026-07-14T00:00:00Z",
  status: "ok",
  message: null,
};

const geminiSnapshot: ProviderSnapshot = {
  provider: "gemini",
  displayName: "GEMINI",
  plan: "PRO",
  shortWindow: {
    remainingPercent: 93,
    resetsAt: "2026-08-07T03:03:00Z",
    windowSeconds: 18_000,
  },
  weeklyWindow: {
    remainingPercent: 97,
    resetsAt: "2026-08-14T00:00:00Z",
    windowSeconds: 604_800,
  },
  extraPools: [
    {
      id: "antigravity_3p",
      displayName: "Claude & GPT",
      shortWindow: {
        remainingPercent: 0,
        resetsAt: "2026-08-07T03:52:00Z",
        windowSeconds: 18_000,
      },
      weeklyWindow: {
        remainingPercent: 44,
        resetsAt: "2026-08-14T00:00:00Z",
        windowSeconds: 604_800,
      },
    },
    {
      id: "gemini_argon",
      displayName: "Gemini 4 Argon",
      shortWindow: null,
      weeklyWindow: {
        remainingPercent: 100,
        resetsAt: null,
        windowSeconds: 604_800,
      },
    },
  ],
  resetCredits: null,
  resetCreditExpiresAt: [],
  updatedAt: "2026-07-14T00:00:00Z",
  status: "ok",
  message: null,
};

const noop = () => {};

afterEach(cleanup);

function renderCard(
  snapshots: ProviderSnapshot[] = [codexSnapshot, geminiSnapshot],
  prefs: WidgetPreferences = preferences,
  onSelectTrayProvider: (target: TrayProviderId) => void = noop,
  onOpenAntigravity: () => void = noop,
  onToggleService: (provider: ProviderId) => void = noop,
  onSignInAntigravity?: () => void,
  onCancelSignInAntigravity?: () => void,
  signingInAntigravity = false,
) {
  return render(
    <QuotaCard
      snapshots={snapshots}
      preferences={prefs}
      onSelectTrayProvider={onSelectTrayProvider}
      onToggleService={onToggleService}
      onLock={noop}
      onLanguage={noop}
      onDrag={noop}
      onHover={noop}
      onRefresh={noop}
      onOpenAntigravity={onOpenAntigravity}
      onSignInAntigravity={onSignInAntigravity}
      onCancelSignInAntigravity={onCancelSignInAntigravity}
      signingInAntigravity={signingInAntigravity}
    />,
  );
}

describe("dual-service overview card", () => {
  it("displays Codex and Antigravity Gemini, Claude & GPT, and Gemini 4 Argon together", () => {
    renderCard();

    expect(screen.getByText("QUOTAPULSE")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Codex 5-hour quota remaining 74%" })).toHaveAttribute("aria-valuenow", "74");
    expect(screen.getByRole("progressbar", { name: "Gemini 5-hour quota remaining 93%" })).toHaveAttribute("aria-valuenow", "93");
    expect(screen.getByText("Claude & GPT")).toBeInTheDocument();
    expect(screen.getByLabelText("Claude & GPT 5-hour quota remaining 0%")).toHaveTextContent("0%");
    expect(screen.getByLabelText("Claude & GPT Weekly quota remaining 44%")).toHaveTextContent("44%");
    expect(screen.getByText("Gemini 4 Argon")).toBeInTheDocument();
    expect(screen.getByLabelText("Gemini 4 Argon Weekly quota remaining 100%")).toHaveTextContent("100%");
  });

  it("lets the user switch the taskbar tray source across both 5h and weekly windows including Gemini 4 Argon", () => {
    const onSelect = vi.fn();
    renderCard([codexSnapshot, geminiSnapshot], preferences, onSelect);

    const codexPin = screen.getByRole("button", { name: /Showing Codex \(GPT\) 5-hour quota/ });
    expect(codexPin).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(screen.getByRole("button", { name: /Show Codex \(GPT\) weekly quota/ }));
    expect(onSelect).toHaveBeenCalledWith("codex_weekly");

    fireEvent.click(screen.getByRole("button", { name: /Show Gemini 5-hour quota/ }));
    expect(onSelect).toHaveBeenCalledWith("gemini");

    fireEvent.click(screen.getByRole("button", { name: /Show Gemini weekly quota/ }));
    expect(onSelect).toHaveBeenCalledWith("gemini_weekly");

    fireEvent.click(screen.getByRole("button", { name: /Show Claude & GPT 5-hour quota/ }));
    expect(onSelect).toHaveBeenCalledWith("antigravity_3p");

    fireEvent.click(screen.getByRole("button", { name: /Show Claude & GPT weekly quota/ }));
    expect(onSelect).toHaveBeenCalledWith("antigravity_3p_weekly");

    fireEvent.click(screen.getByRole("button", { name: /Show Gemini 4 Argon weekly quota/ }));
    expect(onSelect).toHaveBeenCalledWith("gemini_argon_weekly");
  });

  it("isolates signed-out state to one provider without hiding the other and offers direct Sign in plus Open Antigravity", () => {
    const onOpen = vi.fn();
    const onSignIn = vi.fn();
    const onCancel = vi.fn();
    const signedOutSnapshots: ProviderSnapshot[] = [
      codexSnapshot,
      {
        ...geminiSnapshot,
        status: "signed_out",
        shortWindow: null,
        weeklyWindow: null,
        extraPools: [],
        message: "Antigravity is not signed in. Sign in directly from QuotaPulse.",
      },
    ];
    const { rerender } = renderCard(signedOutSnapshots, preferences, noop, onOpen, noop, onSignIn, onCancel, false);

    expect(screen.getByRole("progressbar", { name: "Codex 5-hour quota remaining 74%" })).toBeInTheDocument();
    expect(screen.getByText("Sign in to Antigravity")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(onSignIn).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "Open Antigravity" }));
    expect(onOpen).toHaveBeenCalledTimes(1);

    rerender(
      <QuotaCard
        snapshots={signedOutSnapshots}
        preferences={preferences}
        onSelectTrayProvider={noop}
        onToggleService={noop}
        onLock={noop}
        onLanguage={noop}
        onDrag={noop}
        onHover={noop}
        onRefresh={noop}
        onOpenAntigravity={onOpen}
        onSignInAntigravity={onSignIn}
        onCancelSignInAntigravity={onCancel}
        signingInAntigravity
      />,
    );

    expect(screen.getByRole("button", { name: "Finish in browser…" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("opens and closes Codex reset credit expiration details", () => {
    renderCard();
    const button = screen.getByRole("button", { name: "View" });
    fireEvent.click(button);
    expect(button).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText(/Credit 1:/)).toBeInTheDocument();

    fireEvent.click(button);
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText(/Credit 1:/)).not.toBeInTheDocument();
  });
});

describe("floating orb multi-provider tracking", () => {
  it("tracks Codex, Gemini, and Antigravity 3P according to trayProvider", () => {
    const { rerender } = render(
      <QuotaOrb snapshots={[codexSnapshot, geminiSnapshot]} trayProvider="codex" language="en" onDrag={noop} onHover={noop} />,
    );
    expect(screen.getByText("74")).toBeInTheDocument();
    expect(screen.getByText("GPT")).toBeInTheDocument();
    expect(screen.getByText("1")).toBeInTheDocument();

    rerender(
      <QuotaOrb snapshots={[codexSnapshot, geminiSnapshot]} trayProvider="gemini" language="en" onDrag={noop} onHover={noop} />,
    );
    expect(screen.getByText("93")).toBeInTheDocument();
    expect(screen.getByText("GEM")).toBeInTheDocument();
    expect(screen.getByText("97")).toBeInTheDocument();

    rerender(
      <QuotaOrb snapshots={[codexSnapshot, geminiSnapshot]} trayProvider="antigravity_3p" language="en" onDrag={noop} onHover={noop} />,
    );
    expect(screen.getByText("0")).toBeInTheDocument();
    expect(screen.getByText("3P")).toBeInTheDocument();
    expect(screen.getByText("44")).toBeInTheDocument();

    rerender(
      <QuotaOrb snapshots={[codexSnapshot, geminiSnapshot]} trayProvider="codex_weekly" language="en" onDrag={noop} onHover={noop} />,
    );
    expect(screen.getByText("42")).toBeInTheDocument();
    expect(screen.getByText("GPT·W")).toBeInTheDocument();
    expect(screen.getByText("1")).toBeInTheDocument();

    rerender(
      <QuotaOrb snapshots={[codexSnapshot, geminiSnapshot]} trayProvider="gemini_argon_weekly" language="en" onDrag={noop} onHover={noop} />,
    );
    expect(screen.getByText("100")).toBeInTheDocument();
    expect(screen.getByText("ARGON")).toBeInTheDocument();
    expect(screen.getByText("7d")).toBeInTheDocument();
  });

  it("caps large reset credit badges at 99+", () => {
    render(
      <QuotaOrb
        snapshots={[{ ...codexSnapshot, resetCredits: 137 }, geminiSnapshot]}
        trayProvider="codex"
        language="en"
        onDrag={noop}
        onHover={noop}
      />,
    );
    expect(screen.getByText("99+")).toBeInTheDocument();
  });
});

describe("GPT Pro / Gemini Ultra weekly-only subscriptions and service toggles", () => {
  const codexPro: ProviderSnapshot = {
    ...codexSnapshot,
    plan: "PRO",
    shortWindow: null,
    weeklyWindow: {
      remainingPercent: 82,
      resetsAt: "2026-08-14T00:00:00Z",
      windowSeconds: 604_800,
    },
  };

  const geminiUltra: ProviderSnapshot = {
    ...geminiSnapshot,
    plan: "ULTRA",
    shortWindow: null,
    weeklyWindow: {
      remainingPercent: 91,
      resetsAt: "2026-08-14T00:00:00Z",
      windowSeconds: 604_800,
    },
    extraPools: [
      {
        id: "antigravity_3p",
        displayName: "Claude & GPT",
        shortWindow: null,
        weeklyWindow: {
          remainingPercent: 56,
          resetsAt: "2026-08-14T00:00:00Z",
          windowSeconds: 604_800,
        },
      },
    ],
  };

  it("renders GPT Pro and Gemini Ultra weekly-only quotas cleanly without --% or missing 5h tray buttons", () => {
    renderCard([codexPro, geminiUltra], { ...preferences, trayProvider: "codex" });

    expect(screen.getByRole("progressbar", { name: "Codex Weekly quota remaining 82%" })).toHaveAttribute("aria-valuenow", "82");
    expect(screen.getByRole("progressbar", { name: "Gemini Weekly quota remaining 91%" })).toHaveAttribute("aria-valuenow", "91");
    expect(screen.getByLabelText("Claude & GPT Weekly quota remaining 56%")).toHaveTextContent("56%");
    expect(screen.queryByText("--")).not.toBeInTheDocument();
    expect(screen.queryByText(/5h quota unavailable/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Codex \(GPT\) 5-hour quota/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Gemini 5-hour quota/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Claude & GPT 5-hour quota/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Showing Codex \(GPT\) weekly quota/ })).toHaveAttribute("aria-pressed", "true");
  });

  it("renders Gemini Ultra with merged Argon cleanly when extraPools is empty", () => {
    renderCard(
      [codexPro, { ...geminiUltra, extraPools: [] }],
      { ...preferences, trayProvider: "gemini_argon_weekly" },
    );

    expect(screen.getByRole("progressbar", { name: "Gemini Weekly quota remaining 91%" })).toHaveAttribute("aria-valuenow", "91");
    expect(screen.queryByText("Claude & GPT")).not.toBeInTheDocument();
    expect(screen.queryByText("Gemini 4 Argon")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Showing Gemini weekly quota/ })).toHaveAttribute("aria-pressed", "true");
  });

  it("falls back smoothly in QuotaOrb for GPT Pro and Gemini Ultra weekly-only quotas", () => {
    const { rerender } = render(
      <QuotaOrb snapshots={[codexPro, geminiUltra]} trayProvider="codex" language="en" onDrag={noop} onHover={noop} />,
    );
    expect(screen.getByText("82")).toBeInTheDocument();
    expect(screen.getByText("GPT·W")).toBeInTheDocument();
    expect(screen.getByText("1")).toBeInTheDocument();

    rerender(
      <QuotaOrb snapshots={[codexPro, geminiUltra]} trayProvider="gemini" language="en" onDrag={noop} onHover={noop} />,
    );
    expect(screen.getByText("91")).toBeInTheDocument();
    expect(screen.getByText("GEM·W")).toBeInTheDocument();
    expect(screen.getByText("7d")).toBeInTheDocument();

    rerender(
      <QuotaOrb snapshots={[codexPro, geminiUltra]} trayProvider="gemini_argon_weekly" language="en" onDrag={noop} onHover={noop} />,
    );
    expect(screen.getByText("91")).toBeInTheDocument();
    expect(screen.getByText("GEM·W")).toBeInTheDocument();
  });

  it("supports single-service visibility and blocks disabling the last remaining service", () => {
    const onToggle = vi.fn();
    const { rerender } = render(
      <QuotaCard
        snapshots={[codexSnapshot, geminiSnapshot]}
        preferences={{ ...preferences, showCodex: true, showAntigravity: false, trayProvider: "gemini" }}
        onSelectTrayProvider={noop}
        onToggleService={onToggle}
        onLock={noop}
        onLanguage={noop}
        onDrag={noop}
        onHover={noop}
        onRefresh={noop}
        onOpenAntigravity={noop}
      />,
    );

    expect(screen.getByRole("region", { name: "Codex" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Gemini" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Service visibility" }));
    const codexToggle = screen.getByRole("button", { name: "Codex (GPT)" });
    const antigravityToggle = screen.getByRole("button", { name: "Antigravity (Gemini)" });
    expect(codexToggle).toBeDisabled();
    expect(codexToggle).toHaveAttribute("aria-pressed", "true");
    expect(antigravityToggle).not.toBeDisabled();
    expect(antigravityToggle).toHaveAttribute("aria-pressed", "false");

    fireEvent.click(antigravityToggle);
    expect(onToggle).toHaveBeenCalledWith("gemini");

    rerender(
      <QuotaCard
        snapshots={[codexSnapshot, geminiSnapshot]}
        preferences={{ ...preferences, showCodex: false, showAntigravity: true, trayProvider: "codex" }}
        onSelectTrayProvider={noop}
        onToggleService={onToggle}
        onLock={noop}
        onLanguage={noop}
        onDrag={noop}
        onHover={noop}
        onRefresh={noop}
        onOpenAntigravity={noop}
      />,
    );

    expect(screen.queryByRole("region", { name: "Codex" })).not.toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Gemini" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Antigravity (Gemini)" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Codex (GPT)" }));
    expect(onToggle).toHaveBeenCalledWith("codex");
  });
});
