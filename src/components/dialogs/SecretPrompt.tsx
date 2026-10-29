import { useState, type FormEvent } from "react";
import { Modal } from "./Modal";

interface SecretPromptProps {
  kind: "password" | "passphrase";
  target: string;
  /** Offer to save in the keyring (saved only after a successful login). */
  canSave: boolean;
  onSubmit: (secret: string, save: boolean) => void;
  onCancel: () => void;
}

export function SecretPrompt({ kind, target, canSave, onSubmit, onCancel }: SecretPromptProps) {
  const [secret, setSecret] = useState("");
  const [save, setSave] = useState(false);
  const label = kind === "password" ? "Mot de passe" : "Passphrase de la clé privée";

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    onSubmit(secret, save && canSave);
  }

  return (
    <Modal title={`${label} pour ${target}`} onCancel={onCancel}>
      <form onSubmit={handleSubmit} className="dialog-form">
        <label className="field">
          <span>{label}</span>
          <input
            type="password"
            autoFocus
            value={secret}
            onChange={(e) => setSecret(e.target.value)}
            autoComplete="off"
          />
        </label>
        {canSave && (
          <label className="checkbox">
            <input type="checkbox" checked={save} onChange={(e) => setSave(e.target.checked)} />
            Enregistrer dans le trousseau système (après une connexion réussie)
          </label>
        )}
        <div className="dialog-actions">
          <button type="button" onClick={onCancel}>
            Annuler
          </button>
          <button type="submit" className="primary">
            Se connecter
          </button>
        </div>
      </form>
    </Modal>
  );
}
