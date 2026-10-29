import { useState } from "react";
import { Modal } from "../../components/dialogs/Modal";
import { pastedLineCount } from "../terminal/paste";
import type { Tab } from "../sessions/tabs";

interface BroadcastDialogProps {
  /** Connected sessions only. */
  tabs: Tab[];
  initiallySelected: number | null;
  onSend: (keys: number[], text: string, delayMs: number) => void;
  onCancel: () => void;
}

/** Sends the same commands to several sessions, after an explicit review step. */
export function BroadcastDialog({ tabs, initiallySelected, onSend, onCancel }: BroadcastDialogProps) {
  const [text, setText] = useState("");
  const [delayMs, setDelayMs] = useState("300");
  const [selected, setSelected] = useState<Set<number>>(
    () => new Set(initiallySelected !== null && tabs.some((t) => t.key === initiallySelected) ? [initiallySelected] : []),
  );
  const [reviewing, setReviewing] = useState(false);

  const lines = pastedLineCount(text);
  const delay = Number(delayMs);
  const valid = lines > 0 && selected.size > 0 && Number.isInteger(delay) && delay >= 0 && delay <= 60000;
  const chosen = tabs.filter((t) => selected.has(t.key));

  const toggle = (key: number) =>
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  if (reviewing) {
    return (
      <Modal title="Confirmer l'envoi" tone="danger" onCancel={() => setReviewing(false)} wide>
        <p>
          Envoyer {lines} ligne{lines > 1 ? "s" : ""} à <strong>{chosen.length} session{chosen.length > 1 ? "s" : ""}</strong> :
        </p>
        <ul className="target-list">
          {chosen.map((t) => (
            <li key={t.key}>{t.target.title}</li>
          ))}
        </ul>
        <pre className="preview">{text.replace(/\n+$/, "")}</pre>
        <div className="dialog-actions">
          <button autoFocus onClick={() => setReviewing(false)}>
            Retour
          </button>
          <button className="danger" onClick={() => onSend([...selected], text, delay)}>
            Envoyer à {chosen.length} session{chosen.length > 1 ? "s" : ""}
          </button>
        </div>
      </Modal>
    );
  }

  return (
    <Modal title="Envoyer à plusieurs sessions" onCancel={onCancel} wide>
      <div className="dialog-form">
        <fieldset className="section">
          <legend>Sessions connectées</legend>
          {tabs.length === 0 && <p className="hint">Aucune session connectée.</p>}
          {tabs.map((tab) => (
            <label key={tab.key} className="checkbox">
              <input type="checkbox" checked={selected.has(tab.key)} onChange={() => toggle(tab.key)} />
              {tab.target.title}
            </label>
          ))}
          {tabs.length > 1 && (
            <div className="presets">
              <button type="button" onClick={() => setSelected(new Set(tabs.map((t) => t.key)))}>
                Toutes
              </button>
              <button type="button" onClick={() => setSelected(new Set())}>
                Aucune
              </button>
            </div>
          )}
        </fieldset>
        <label className="field">
          <span>Commandes (une par ligne)</span>
          <textarea autoFocus rows={5} value={text} onChange={(e) => setText(e.target.value)} spellCheck={false} />
        </label>
        <label className="field port">
          <span>Délai (ms)</span>
          <input value={delayMs} onChange={(e) => setDelayMs(e.target.value)} inputMode="numeric" />
        </label>
        <div className="dialog-actions">
          <button type="button" onClick={onCancel}>
            Annuler
          </button>
          <button type="button" className="primary" disabled={!valid} onClick={() => setReviewing(true)}>
            Vérifier l'envoi…
          </button>
        </div>
      </div>
    </Modal>
  );
}
