import codexLogo from "../../codex.svg";
import type { ProviderId } from "../types";

export function ProviderMark({ provider = "codex" }: { provider?: ProviderId }) {
  if (provider === "gemini") {
    return (
      <div className="provider-mark provider-mark--gemini" aria-label="Gemini">
        <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <defs>
            <linearGradient id="gemini-spark" x1="2" y1="2" x2="22" y2="22" gradientUnits="userSpaceOnUse">
              <stop offset="0%" stopColor="#4285F4" />
              <stop offset="52%" stopColor="#7C5CFC" />
              <stop offset="100%" stopColor="#26C6DA" />
            </linearGradient>
          </defs>
          <path
            d="M12 1.8C12.7 7.2 16.8 11.3 22.2 12C16.8 12.7 12.7 16.8 12 22.2C11.3 16.8 7.2 12.7 1.8 12C7.2 11.3 11.3 7.2 12 1.8Z"
            fill="url(#gemini-spark)"
          />
        </svg>
      </div>
    );
  }

  return (
    <div className="provider-mark provider-mark--codex" aria-label="Codex">
      <img src={codexLogo} alt="" />
    </div>
  );
}
