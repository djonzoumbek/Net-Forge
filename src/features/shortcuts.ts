// Application keyboard shortcuts. One definition used both by the terminal (so
// that these keys are not sent to the remote shell) and by the app (to act).

export type ShortcutAction =
  | "newTab"
  | "closeTab"
  | "duplicateTab"
  | "nextTab"
  | "previousTab"
  | "search"
  | "zoomIn"
  | "zoomOut"
  | "zoomReset"
  | "toggleSidebar"
  | "settings";

/** Shown in the settings dialog. */
export const shortcutList: { keys: string; label: string }[] = [
  { keys: "Ctrl+Shift+T", label: "Nouvelle connexion" },
  { keys: "Ctrl+Shift+D", label: "Dupliquer l'onglet (même cible)" },
  { keys: "Ctrl+Shift+W", label: "Fermer l'onglet" },
  { keys: "Ctrl+Tab / Ctrl+Shift+Tab", label: "Onglet suivant / précédent" },
  { keys: "Ctrl+PageDown / Ctrl+PageUp", label: "Onglet suivant / précédent" },
  { keys: "Ctrl+Shift+F", label: "Rechercher dans l'historique" },
  { keys: "Ctrl+Shift+C / Ctrl+Shift+V", label: "Copier / coller" },
  { keys: "Ctrl++ / Ctrl+- / Ctrl+0", label: "Agrandir / réduire / taille normale du texte" },
  { keys: "Ctrl+Shift+B", label: "Afficher / masquer les profils" },
  { keys: "Ctrl+,", label: "Réglages" },
];

/** Letters are matched on `key` (the character typed, so AZERTY works), digits
 * on `code` (Ctrl+0 is Ctrl+à on an AZERTY keyboard). */
export function matchShortcut(event: KeyboardEvent): ShortcutAction | null {
  if (event.type !== "keydown" || !event.ctrlKey || event.altKey || event.metaKey) return null;
  const key = event.key.toLowerCase();
  if (event.key === "Tab") return event.shiftKey ? "previousTab" : "nextTab";
  if (event.key === "PageDown") return "nextTab";
  if (event.key === "PageUp") return "previousTab";
  if (event.shiftKey) {
    switch (key) {
      case "t":
        return "newTab";
      case "w":
        return "closeTab";
      case "d":
        return "duplicateTab";
      case "f":
        return "search";
      case "b":
        return "toggleSidebar";
    }
  }
  if (key === "+" || key === "=") return "zoomIn";
  if (key === "-" && !event.shiftKey) return "zoomOut";
  if (event.code === "Digit0" || event.code === "Numpad0") return "zoomReset";
  if (key === "," && !event.shiftKey) return "settings";
  return null;
}
