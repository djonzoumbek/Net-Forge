import { useState, type FormEvent } from "react";
import { Modal } from "../../components/dialogs/Modal";
import { toAppError } from "../../services/sessionService";
import type { ButtonInput, CommandButton } from "../../types/profiles";

interface ButtonEditorProps {
  button?: CommandButton;
  onSave: (input: ButtonInput) => Promise<void>;
  onDelete: (button: CommandButton) => Promise<void>;
  onCancel: () => void;
}

export function ButtonEditor({ button, onSave, onDelete, onCancel }: ButtonEditorProps) {
  const [label, setLabel] = useState(button?.label ?? "");
  const [commands, setCommands] = useState(button?.commands ?? "");
  const [delayMs, setDelayMs] = useState(String(button?.delayMs ?? 300));
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function run(action: () => Promise<void>) {
    try {
      await action();
    } catch (e) {
      setError(toAppError(e).message);
    }
  }

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    const delay = Number(delayMs);
    if (!Number.isInteger(delay) || delay < 0 || delay > 60000) {
      setError("Délai : entier entre 0 et 60000 ms.");
      return;
    }
    void run(() => onSave({ label, commands, delayMs: delay }));
  }

  return (
    <Modal title={button ? `Modifier « ${button.label} »` : "Nouveau bouton"} onCancel={onCancel} wide>
      <form className="dialog-form" onSubmit={handleSubmit}>
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        <div className="field-row">
          <label className="field grow">
            <span>Libellé</span>
            <input autoFocus value={label} onChange={(e) => setLabel(e.target.value)} maxLength={40} />
          </label>
          <label className="field port">
            <span>Délai (ms)</span>
            <input value={delayMs} onChange={(e) => setDelayMs(e.target.value)} inputMode="numeric" />
          </label>
        </div>
        <label className="field">
          <span>Commandes (une par ligne, envoyées dans l'ordre)</span>
          <textarea
            rows={7}
            value={commands}
            onChange={(e) => setCommands(e.target.value)}
            spellCheck={false}
            placeholder={"show ip interface brief\nshow version | include uptime"}
          />
        </label>
        <p className="hint">
          Le délai sépare deux lignes : augmente-le pour les commandes lentes (ex. <code>write memory</code>).
          Une ligne vide envoie seulement Entrée (utile pour <code>[confirm]</code>).
        </p>
        <div className="dialog-actions">
          {button &&
            (confirmDelete ? (
              <button type="button" className="danger" onClick={() => void run(() => onDelete(button))}>
                Confirmer la suppression
              </button>
            ) : (
              <button type="button" onClick={() => setConfirmDelete(true)}>
                Supprimer
              </button>
            ))}
          <span className="spacer" />
          <button type="button" onClick={onCancel}>
            Annuler
          </button>
          <button type="submit" className="primary">
            Enregistrer
          </button>
        </div>
      </form>
    </Modal>
  );
}
