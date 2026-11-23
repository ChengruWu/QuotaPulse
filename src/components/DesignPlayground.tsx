import { useMemo, useState, type CSSProperties } from "react";
import type { ProviderSnapshot, TrayProviderId, WidgetPreferences } from "../types";
import { QuotaCard, QuotaOrb } from "./QuotaCard";

const codexPreview: ProviderSnapshot = {
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
};

const geminiPreview: ProviderSnapshot = {
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
};

const codexProWeeklyPreview: ProviderSnapshot = {
  ...codexPreview,
  plan: "PRO",
  shortWindow: null,
  weeklyWindow: { remainingPercent: 82, resetsAt: new Date(Date.now() + 4.3 * 86_400_000).toISOString(), windowSeconds: 604_800 },
};

const geminiUltraWeeklyPreview: ProviderSnapshot = {
  ...geminiPreview,
  plan: "ULTRA",
  shortWindow: null,
  weeklyWindow: { remainingPercent: 91, resetsAt: new Date(Date.now() + 5.6 * 86_400_000).toISOString(), windowSeconds: 604_800 },
  extraPools: [
    {
      id: "antigravity_3p",
      displayName: "Claude & GPT",
      shortWindow: null,
      weeklyWindow: { remainingPercent: 56, resetsAt: new Date(Date.now() + 5.6 * 86_400_000).toISOString(), windowSeconds: 604_800 },
    },
  ],
};

interface Values {
  radius: number;
  numberSize: number;
  progressHeight: number;
  brightness: number;
  cool: string;
  glow: string;
  warm: string;
}

type PreviewMode =
  | "healthy"
  | "caution"
  | "critical"
  | "weekly-only"
  | "gemini-signed-out"
  | "codex-stale"
  | "orb-codex"
  | "orb-gemini"
  | "orb-3p";

const previewModes: Array<{ value: PreviewMode; label: string }> = [
  { value: "healthy", label: "Healthy Dual" },
  { value: "caution", label: "Caution" },
  { value: "critical", label: "Critical" },
  { value: "weekly-only", label: "Pro / Ultra (Weekly)" },
  { value: "gemini-signed-out", label: "Gemini Signed Out" },
  { value: "codex-stale", label: "Codex Stale" },
  { value: "orb-codex", label: "Orb · GPT" },
  { value: "orb-gemini", label: "Orb · Gemini" },
  { value: "orb-3p", label: "Orb · 3P" },
];

const defaults: Values = {
  radius: 28,
  numberSize: 36,
  progressHeight: 6,
  brightness: 100,
  cool: "#b8d3f2",
  glow: "#dff5eb",
  warm: "#ccd8f7",
};

function initialPreviewMode(): PreviewMode {
  const mode = new URLSearchParams(window.location.search).get("mode");
  if (
    mode === "healthy"
    || mode === "caution"
    || mode === "critical"
    || mode === "weekly-only"
    || mode === "gemini-signed-out"
    || mode === "codex-stale"
    || mode === "orb-codex"
    || mode === "orb-gemini"
    || mode === "orb-3p"
  ) {
    return mode;
  }
  return "healthy";
}

function isOrbMode(mode: PreviewMode): boolean {
  return mode.startsWith("orb-");
}

function makePreviewSnapshots(mode: PreviewMode): ProviderSnapshot[] {
  if (mode === "caution") {
    return [
      { ...codexPreview, shortWindow: codexPreview.shortWindow ? { ...codexPreview.shortWindow, remainingPercent: 32 } : null },
      { ...geminiPreview, shortWindow: geminiPreview.shortWindow ? { ...geminiPreview.shortWindow, remainingPercent: 28 } : null },
    ];
  }
  if (mode === "critical") {
    return [
      { ...codexPreview, shortWindow: codexPreview.shortWindow ? { ...codexPreview.shortWindow, remainingPercent: 7 } : null },
      { ...geminiPreview, shortWindow: geminiPreview.shortWindow ? { ...geminiPreview.shortWindow, remainingPercent: 5 } : null },
    ];
  }
  if (mode === "weekly-only") {
    return [codexProWeeklyPreview, geminiUltraWeeklyPreview];
  }
  if (mode === "gemini-signed-out") {
    return [
      codexPreview,
      {
        ...geminiPreview,
        status: "signed_out",
        shortWindow: null,
        weeklyWindow: null,
        extraPools: [],
        message: "Antigravity login expired. Open Antigravity or sign in again.",
      },
    ];
  }
  if (mode === "codex-stale") {
    return [
      {
        ...codexPreview,
        status: "stale",
        updatedAt: new Date(Date.now() - 2 * 60 * 60_000).toISOString(),
        message: "Refresh failed. Please try again later.",
      },
      geminiPreview,
    ];
  }
  return [codexPreview, geminiPreview];
}

export function DesignPlayground() {
  const [values, setValues] = useState(defaults);
  const [previewMode, setPreviewMode] = useState<PreviewMode>(() => initialPreviewMode());
  const params = new URLSearchParams(window.location.search);
  const [trayProvider, setTrayProvider] = useState<TrayProviderId>(() => {
    const param = params.get("tray");
    if (param === "gemini" || param === "antigravity_3p") return param;
    return "codex";
  });
  const screenshotMode = params.has("shot");
  const shotKind = params.get("shot");
  const showCreditTip = params.has("creditTip");
  const previewPreferences: WidgetPreferences = {
    locked: false,
    alwaysOnTop: true,
    pinnedProvider: null,
    autoRotateSeconds: 12,
    language: params.get("lang") === "en" ? "en" : "zh-CN",
    trayProvider: previewMode === "orb-gemini" ? "gemini" : previewMode === "orb-3p" ? "antigravity_3p" : trayProvider,
    showCodex: true,
    showAntigravity: true,
  };

  const style = useMemo(() => ({
    "--card-radius": `${values.radius}px`,
    "--number-size": `${values.numberSize}px`,
    "--progress-height": `${values.progressHeight}px`,
    "--card-brightness": `${values.brightness}%`,
    "--cool": values.cool,
    "--glow": values.glow,
    "--warm": values.warm,
  }) as CSSProperties, [values]);

  const activeSnapshots = useMemo(() => makePreviewSnapshots(previewMode), [previewMode]);
  const update = <K extends keyof Values>(key: K, value: Values[K]) => setValues((current) => ({ ...current, [key]: value }));

  if (screenshotMode) {
    if (shotKind === "states") {
      return (
        <div className="screenshot-stage screenshot-stage--states" style={style}>
          {(["healthy", "caution", "critical"] as PreviewMode[]).map((mode) => (
            <div className="design-card-frame" key={mode}>
              <QuotaCard
                snapshots={makePreviewSnapshots(mode)}
                preferences={{ ...previewPreferences, trayProvider: mode === "caution" ? "gemini" : "codex" }}
                onSelectTrayProvider={() => {}}
                onToggleService={() => {}}
                onLock={() => {}}
                onLanguage={() => {}}
                onDrag={() => {}}
                onHover={() => {}}
                onRefresh={() => {}}
              />
            </div>
          ))}
        </div>
      );
    }

    if (shotKind === "orbs") {
      const orbItems: Array<{ key: string; snapshots: ProviderSnapshot[]; trayProvider: TrayProviderId }> = [
        { key: "codex-healthy", snapshots: makePreviewSnapshots("healthy"), trayProvider: "codex" },
        { key: "gemini-healthy", snapshots: makePreviewSnapshots("healthy"), trayProvider: "gemini" },
        { key: "3p-weekly-caution", snapshots: makePreviewSnapshots("healthy"), trayProvider: "antigravity_3p_weekly" },
        { key: "codex-critical", snapshots: makePreviewSnapshots("critical"), trayProvider: "codex" },
      ];
      return (
        <div className="screenshot-stage screenshot-stage--orbs" style={style}>
          {orbItems.map((item) => (
            <div className="design-orb-frame" key={item.key}>
              <QuotaOrb
                snapshots={item.snapshots}
                trayProvider={item.trayProvider}
                language={previewPreferences.language}
                onDrag={() => {}}
                onHover={() => {}}
              />
            </div>
          ))}
        </div>
      );
    }

    if (shotKind === "single") {
      const healthySnapshots = makePreviewSnapshots("healthy");
      return (
        <div className="screenshot-stage screenshot-stage--single" style={style}>
          <div className="design-card-frame design-card-frame--codex-only">
            <QuotaCard
              snapshots={healthySnapshots}
              preferences={{ ...previewPreferences, showCodex: true, showAntigravity: false, trayProvider: "codex" }}
              onSelectTrayProvider={() => {}}
              onToggleService={() => {}}
              onLock={() => {}}
              onLanguage={() => {}}
              onDrag={() => {}}
              onHover={() => {}}
              onRefresh={() => {}}
            />
          </div>
          <div className="design-card-frame design-card-frame--gemini-only">
            <QuotaCard
              snapshots={healthySnapshots}
              preferences={{ ...previewPreferences, showCodex: false, showAntigravity: true, trayProvider: "gemini" }}
              onSelectTrayProvider={() => {}}
              onToggleService={() => {}}
              onLock={() => {}}
              onLanguage={() => {}}
              onDrag={() => {}}
              onHover={() => {}}
              onRefresh={() => {}}
            />
          </div>
        </div>
      );
    }

    if (shotKind === "weekly") {
      return (
        <div className="screenshot-stage" style={style}>
          <div className="design-card-frame">
            <QuotaCard
              snapshots={makePreviewSnapshots("weekly-only")}
              preferences={{ ...previewPreferences, trayProvider: "codex_weekly" }}
              onSelectTrayProvider={() => {}}
              onToggleService={() => {}}
              onLock={() => {}}
              onLanguage={() => {}}
              onDrag={() => {}}
              onHover={() => {}}
              onRefresh={() => {}}
            />
          </div>
        </div>
      );
    }

    return (
      <div className="screenshot-stage" style={style}>
        <div className={isOrbMode(previewMode) ? "design-orb-frame" : "design-card-frame"}>
          {isOrbMode(previewMode) ? (
            <QuotaOrb
              snapshots={activeSnapshots}
              trayProvider={previewPreferences.trayProvider}
              language={previewPreferences.language}
              onDrag={() => {}}
              onHover={() => {}}
            />
          ) : (
            <QuotaCard
              snapshots={activeSnapshots}
              preferences={previewPreferences}
              onSelectTrayProvider={setTrayProvider}
              onToggleService={() => {}}
              onLock={() => {}}
              onLanguage={() => {}}
              onDrag={() => {}}
              onHover={() => {}}
              onRefresh={() => {}}
              onOpenAntigravity={() => {}}
              initialShowCreditTip={showCreditTip}
            />
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="design-workbench">
      <section className="design-stage" style={style}>
        <div className="design-preview-switch" aria-label="Quota status preview">
          {previewModes.map((mode) => (
            <button key={mode.value} className={previewMode === mode.value ? "is-active" : ""} onClick={() => setPreviewMode(mode.value)}>
              {mode.label}
            </button>
          ))}
        </div>
        <div className={isOrbMode(previewMode) ? "design-orb-frame" : "design-card-frame"}>
          {isOrbMode(previewMode) ? (
            <QuotaOrb
              snapshots={activeSnapshots}
              trayProvider={previewPreferences.trayProvider}
              language={previewPreferences.language}
              onDrag={() => {}}
              onHover={() => {}}
            />
          ) : (
            <QuotaCard
              snapshots={activeSnapshots}
              preferences={previewPreferences}
              onSelectTrayProvider={setTrayProvider}
              onToggleService={() => {}}
              onLock={() => {}}
              onLanguage={() => {}}
              onDrag={() => {}}
              onHover={() => {}}
              onRefresh={() => {}}
              onOpenAntigravity={() => {}}
            />
          )}
        </div>
      </section>
      <aside className="design-controls">
        <div>
          <p className="design-kicker">QUOTAPULSE</p>
          <h1>Visual Tuning</h1>
          <p className="design-description">Preview the dual-service Windows overview card and floating orb live.</p>
        </div>
        <Range label="Radius" value={values.radius} min={18} max={44} unit="px" onChange={(v) => update("radius", v)} />
        <Range label="Main number" value={values.numberSize} min={28} max={48} unit="px" onChange={(v) => update("numberSize", v)} />
        <Range label="Progress" value={values.progressHeight} min={4} max={10} unit="px" onChange={(v) => update("progressHeight", v)} />
        <Range label="Brightness" value={values.brightness} min={75} max={120} unit="%" onChange={(v) => update("brightness", v)} />
        <div className="color-row">
          <Color label="Cool" value={values.cool} onChange={(v) => update("cool", v)} />
          <Color label="Glow" value={values.glow} onChange={(v) => update("glow", v)} />
          <Color label="Warm" value={values.warm} onChange={(v) => update("warm", v)} />
        </div>
        <button className="reset-design" onClick={() => setValues(defaults)}>Reset design</button>
      </aside>
    </div>
  );
}

function Range({ label, value, min, max, unit, onChange }: { label: string; value: number; min: number; max: number; unit: string; onChange: (value: number) => void }) {
  return <label className="range-control"><span>{label}<output>{value}{unit}</output></span><input type="range" min={min} max={max} value={value} onChange={(event) => onChange(Number(event.target.value))} /></label>;
}

function Color({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return <label className="color-control"><input type="color" value={value} onChange={(event) => onChange(event.target.value)} /><span>{label}</span></label>;
}
