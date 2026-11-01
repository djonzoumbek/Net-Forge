// Mirrors src-tauri/src/persistence/settings.rs.

export type ThemeName = "netforge-dark" | "netforge-light" | "solarized-dark" | "high-contrast";

export interface AppSettings {
  /** Lines kept in each terminal's history. */
  scrollback: number;
  theme: ThemeName;
  fontFamily: string;
  fontSize: number;
  /** Session logs directory; empty = ~/Documents/NetForge/logs. */
  logDir: string;
  /** A mouse selection is copied to the clipboard at once. */
  copyOnSelect: boolean;
  /** A right click in the terminal pastes the clipboard. */
  rightClickPaste: boolean;
  /** Ctrl+V pastes (otherwise ^V goes to the remote shell). */
  ctrlVPaste: boolean;
}

export const defaultSettings: AppSettings = {
  scrollback: 10000,
  theme: "netforge-dark",
  fontFamily: "DejaVu Sans Mono",
  fontSize: 14,
  logDir: "",
  copyOnSelect: true,
  rightClickPaste: true,
  ctrlVPaste: true,
};
