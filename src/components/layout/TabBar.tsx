import type { Tab } from "../../features/sessions/tabs";
import { WindowControls } from "./WindowFrame";

interface TabBarProps {
  tabs: Tab[];
  active: number | null;
  onActivate: (key: number | null) => void;
  onClose: (key: number) => void;
}

/** Status dot colour: reuses the status bar classes. */
function dotClass(tab: Tab): string {
  switch (tab.status.kind) {
    case "connected":
      return "connected";
    case "connecting":
      return "connecting";
    case "hostKey":
    case "hostKeyChanged":
      return "attention";
    default:
      return "closed";
  }
}

export function TabBar({ tabs, active, onActivate, onClose }: TabBarProps) {
  return (
    <div className="tab-bar">
      <div className="tabs-scroll" role="tablist" aria-label="Sessions">
        {tabs.map((tab) => {
          const legacy = tab.status.kind === "connected" && (tab.status.info.algorithms?.legacy.length ?? 0) > 0;
          return (
            <div
              key={tab.key}
              role="tab"
              aria-selected={tab.key === active}
              className={`tab ${tab.key === active ? "active" : ""}`}
              title={tab.target.title}
              onClick={() => onActivate(tab.key)}
              onAuxClick={(e) => {
                if (e.button === 1) onClose(tab.key);
              }}
            >
              <span className={`status-dot ${dotClass(tab)}`} aria-hidden />
              <span className="tab-label">{tab.target.label}</span>
              {tab.target.protocol === "telnet" && (
                <span className="badge danger small" title="Telnet : non chiffré">
                  clair
                </span>
              )}
              {legacy && (
                <span className="badge legacy small" title="Algorithmes legacy négociés">
                  legacy
                </span>
              )}
              <button
                className="tab-close"
                aria-label={`Fermer ${tab.target.label}`}
                onClick={(e) => {
                  e.stopPropagation();
                  onClose(tab.key);
                }}
              >
                ×
              </button>
            </div>
          );
        })}
        <button
          className={`tab new-tab ${active === null ? "active" : ""}`}
          onClick={() => onActivate(null)}
          title="Nouvelle connexion"
        >
          +
        </button>
      </div>
      {/* Free space: moves the window (title bar), double-click maximizes. */}
      <div className="drag-region" data-tauri-drag-region />
      <WindowControls />
    </div>
  );
}
