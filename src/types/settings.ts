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
}

export const defaultSettings: AppSettings = {
  scrollback: 10000,
  theme: "netforge-dark",
  fontFamily: "DejaVu Sans Mono",
  fontSize: 14,
  logDir: "",
};
