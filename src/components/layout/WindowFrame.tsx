// Replaces the system title bar (decorations: false): window buttons, and
// invisible borders to resize the window.
import { getCurrentWindow } from "@tauri-apps/api/window";

type ResizeDirection =
  | "North"
  | "South"
  | "East"
  | "West"
  | "NorthEast"
  | "NorthWest"
  | "SouthEast"
  | "SouthWest";

const appWindow = () => getCurrentWindow();

export function WindowControls() {
  return (
    <div className="window-controls">
      <button aria-label="Réduire" title="Réduire" onClick={() => void appWindow().minimize()}>
        ─
      </button>
      <button
        aria-label="Agrandir ou restaurer"
        title="Agrandir / restaurer"
        onClick={() => void appWindow().toggleMaximize()}
      >
        ☐
      </button>
      <button
        className="close"
        aria-label="Fermer NetForge"
        title="Fermer"
        onClick={() => void appWindow().close()}
      >
        ✕
      </button>
    </div>
  );
}

const HANDLES: ResizeDirection[] = [
  "North",
  "South",
  "East",
  "West",
  "NorthEast",
  "NorthWest",
  "SouthEast",
  "SouthWest",
];

/** Thin borders that hand the resize over to the window manager. */
export function ResizeHandles() {
  return (
    <>
      {HANDLES.map((direction) => (
        <div
          key={direction}
          className={`resize-handle ${direction.toLowerCase()}`}
          onMouseDown={(event) => {
            if (event.button === 0) void appWindow().startResizeDragging(direction);
          }}
        />
      ))}
    </>
  );
}
