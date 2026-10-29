import type { Tab } from "../../features/sessions/tabs";
import { usePointerDrag } from "../../hooks/usePointerDrag";
import { WindowControls } from "./WindowFrame";

interface TabBarProps {
  tabs: Tab[];
  active: number | null;
  onActivate: (key: number | null) => void;
  onClose: (key: number) => void;
  /** Drag and drop: put tab `key` before `beforeKey` (`null`: at the end). */
  onMove: (key: number, beforeKey: number | null) => void;
}

/** Where a dragged tab lands; `mark` / `side` draw the insertion line. */
interface TabDrop {
  beforeKey: number | null;
  mark: number;
  side: "before" | "after";
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

export function TabBar({ tabs, active, onActivate, onClose, onMove }: TabBarProps) {
  const { drag, startDrag } = usePointerDrag<number, TabDrop>({
    findDrop: (element, _key, x) => {
      const tabElement = element?.closest<HTMLElement>("[data-tab-key]");
      if (!tabElement) {
        // Free space after the last tab: move to the end.
        const last = tabs[tabs.length - 1];
        return element?.closest(".tabs-scroll") && last ? { beforeKey: null, mark: last.key, side: "after" } : null;
      }
      const key = Number(tabElement.dataset.tabKey);
      const rect = tabElement.getBoundingClientRect();
      if (x < rect.left + rect.width / 2) return { beforeKey: key, mark: key, side: "before" };
      const index = tabs.findIndex((tab) => tab.key === key);
      return { beforeKey: tabs[index + 1]?.key ?? null, mark: key, side: "after" };
    },
    onDrop: (key, drop) => onMove(key, drop.beforeKey),
  });
  const draggedTab = drag ? tabs.find((tab) => tab.key === drag.item) : undefined;

  return (
    <div className="tab-bar">
      <div className="tabs-scroll" role="tablist" aria-label="Sessions">
        {tabs.map((tab) => {
          const legacy = tab.status.kind === "connected" && (tab.status.info.algorithms?.legacy.length ?? 0) > 0;
          return (
            <div
              key={tab.key}
              role="tab"
              data-tab-key={tab.key}
              aria-selected={tab.key === active}
              className={[
                "tab",
                tab.key === active ? "active" : "",
                drag?.item === tab.key ? "dragged" : "",
                drag?.drop?.mark === tab.key ? `drop-${drag.drop.side}` : "",
              ].join(" ")}
              title={tab.target.title}
              onPointerDown={(e) => {
                if (!(e.target as Element).closest(".tab-close")) startDrag(e, tab.key);
              }}
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
              {tab.target.protocol === "serial" && (
                <span className="badge neutral small" title="Console série (câble local)">
                  série
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
      {drag && draggedTab && (
        <div className="drag-ghost" style={{ left: drag.x + 12, top: drag.y + 8 }}>
          {draggedTab.target.label}
        </div>
      )}
    </div>
  );
}
