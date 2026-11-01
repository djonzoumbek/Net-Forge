import { useState, type FormEvent } from "react";
import { Modal } from "../../components/dialogs/Modal";
import { toAppError } from "../../services/sessionService";
import type { AppSettings, ThemeName } from "../../types/settings";
import { shortcutList } from "../shortcuts";
import { suggestedFonts, themes } from "../terminal/theme";

interface SettingsDialogProps {
  settings: AppSettings;
  onSave: (settings: AppSettings) => Promise<void>;
  onCancel: () => void;
}

export function SettingsDialog({ settings, onSave, onCancel }: SettingsDialogProps) {
  const [scrollback, setScrollback] = useState(String(settings.scrollback));
  const [theme, setTheme] = useState<ThemeName>(settings.theme);
  const [fontFamily, setFontFamily] = useState(settings.fontFamily);
  const [fontSize, setFontSize] = useState(String(settings.fontSize));
  const [logDir, setLogDir] = useState(settings.logDir);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    setSaving(true);
    try {
      // Ranges and font name are checked by the backend (single source of truth).
      await onSave({
        scrollback: Number(scrollback),
        theme,
        fontFamily,
        fontSize: Number(fontSize),
        logDir,
      });
    } catch (e) {
      setError(toAppError(e).message);
      setSaving(false);
    }
  }

  return (
    <Modal title="Réglages" onCancel={onCancel} wide>
      <form className="dialog-form" onSubmit={handleSubmit}>
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        <fieldset className="section">
          <legend>Terminal</legend>
          <div className="field-row">
            <label className="field grow">
              <span>Thème</span>
              <select value={theme} onChange={(e) => setTheme(e.target.value as ThemeName)}>
                {Object.entries(themes).map(([name, { label }]) => (
                  <option key={name} value={name}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
            <label className="field grow">
              <span>Historique (lignes)</span>
              <input value={scrollback} onChange={(e) => setScrollback(e.target.value)} inputMode="numeric" />
            </label>
          </div>
          <div className="field-row">
            <label className="field grow">
              <span>Police</span>
              <input
                value={fontFamily}
                onChange={(e) => setFontFamily(e.target.value)}
                list="font-suggestions"
                spellCheck={false}
              />
              <datalist id="font-suggestions">
                {suggestedFonts.map((font) => (
                  <option key={font} value={font} />
                ))}
              </datalist>
            </label>
            <label className="field port">
              <span>Taille</span>
              <input value={fontSize} onChange={(e) => setFontSize(e.target.value)} inputMode="numeric" />
            </label>
          </div>
          <p className="hint">
            Historique : de 1 000 à 200 000 lignes par onglet (chaque ligne coûte de la mémoire).
            Ctrl++ / Ctrl+- changent la taille temporairement.
          </p>
        </fieldset>

        <fieldset className="section">
          <legend>Journaux de session</legend>
          <label className="field">
            <span>Dossier (chemin absolu ; vide = ~/Documents/NetForge/logs)</span>
            <input value={logDir} onChange={(e) => setLogDir(e.target.value)} spellCheck={false} />
          </label>
          <p className="hint">L'enregistrement s'active profil par profil (Modifier le profil → Journal de session).</p>
        </fieldset>

        <fieldset className="section">
          <legend>Raccourcis clavier</legend>
          <dl className="shortcut-list">
            {shortcutList.map(({ keys, label }) => (
              <div key={keys}>
                <dt>
                  <kbd>{keys}</kbd>
                </dt>
                <dd>{label}</dd>
              </div>
            ))}
          </dl>
          <p className="hint">
            Les raccourcis d'onglet, de recherche et de zoom agissent sur la session active : ils sont sans effet
            tant qu'aucune session n'est ouverte, ainsi que lorsqu'une fenêtre comme celle-ci est affichée.
          </p>
        </fieldset>

        <div className="dialog-actions">
          <button type="button" onClick={onCancel}>
            Annuler
          </button>
          <button type="submit" className="primary" disabled={saving}>
            Enregistrer
          </button>
        </div>
      </form>
    </Modal>
  );
}
