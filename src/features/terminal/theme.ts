import type { ITheme } from "@xterm/xterm";
import type { ThemeName } from "../../types/settings";

/** Terminal colour schemes. The interface palette of each theme lives in
 * src/styles/app.css (`:root[data-theme="<name>"]`). */
export const themes: Record<ThemeName, { label: string; colors: ITheme }> = {
  "netforge-dark": {
    label: "NetForge sombre",
    // Deep blue-grey background, warm amber cursor.
    colors: {
      background: "#0f141a",
      foreground: "#d7dde4",
      cursor: "#f2b134",
      cursorAccent: "#0f141a",
      selectionBackground: "#2f4a63",
      black: "#1b222b",
      red: "#e5675e",
      green: "#8cc27a",
      yellow: "#e9c46a",
      blue: "#5fa8e0",
      magenta: "#c38bd9",
      cyan: "#56c2c6",
      white: "#d7dde4",
      brightBlack: "#5c6773",
      brightRed: "#ff8a80",
      brightGreen: "#b5e08f",
      brightYellow: "#ffd98a",
      brightBlue: "#8cc8ff",
      brightMagenta: "#e0b0f5",
      brightCyan: "#8ae6ea",
      brightWhite: "#ffffff",
    },
  },
  "netforge-light": {
    label: "NetForge clair",
    colors: {
      background: "#f7f6f2",
      foreground: "#2a2f36",
      cursor: "#b36b00",
      cursorAccent: "#f7f6f2",
      selectionBackground: "#c9d8e8",
      black: "#2a2f36",
      red: "#b3261e",
      green: "#2f7d32",
      yellow: "#8a6100",
      blue: "#1f5fa8",
      magenta: "#8e3fa6",
      cyan: "#00777d",
      white: "#d4d4cf",
      brightBlack: "#6b7280",
      brightRed: "#d9473f",
      brightGreen: "#3f9a43",
      brightYellow: "#a87900",
      brightBlue: "#3577c2",
      brightMagenta: "#a65cbd",
      brightCyan: "#159299",
      brightWhite: "#ffffff",
    },
  },
  "solarized-dark": {
    label: "Solarized sombre",
    // Ethan Schoonover's public Solarized palette.
    colors: {
      background: "#002b36",
      foreground: "#839496",
      cursor: "#93a1a1",
      cursorAccent: "#002b36",
      selectionBackground: "#073642",
      black: "#073642",
      red: "#dc322f",
      green: "#859900",
      yellow: "#b58900",
      blue: "#268bd2",
      magenta: "#d33682",
      cyan: "#2aa198",
      white: "#eee8d5",
      brightBlack: "#586e75",
      brightRed: "#cb4b16",
      brightGreen: "#586e75",
      brightYellow: "#657b83",
      brightBlue: "#839496",
      brightMagenta: "#6c71c4",
      brightCyan: "#93a1a1",
      brightWhite: "#fdf6e3",
    },
  },
  "high-contrast": {
    label: "Contraste élevé",
    colors: {
      background: "#000000",
      foreground: "#ffffff",
      cursor: "#ffff00",
      cursorAccent: "#000000",
      selectionBackground: "#0050c8",
      black: "#000000",
      red: "#ff5555",
      green: "#55ff55",
      yellow: "#ffff55",
      blue: "#6699ff",
      magenta: "#ff55ff",
      cyan: "#55ffff",
      white: "#ffffff",
      brightBlack: "#9a9a9a",
      brightRed: "#ff8080",
      brightGreen: "#80ff80",
      brightYellow: "#ffff80",
      brightBlue: "#99bbff",
      brightMagenta: "#ff80ff",
      brightCyan: "#80ffff",
      brightWhite: "#ffffff",
    },
  },
};

/** Monospace fonts usually present on Ubuntu (any installed font can be typed). */
export const suggestedFonts = ["DejaVu Sans Mono", "Ubuntu Mono", "Liberation Mono", "Noto Sans Mono", "Monospace"];

/** CSS font list: the chosen font first, then safe fallbacks. */
export function fontStack(family: string): string {
  return `"${family}", "DejaVu Sans Mono", "Ubuntu Mono", monospace`;
}
