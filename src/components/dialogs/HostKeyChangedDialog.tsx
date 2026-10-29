import { useState } from "react";
import type { HostKeyMismatch } from "../../types/session";
import { Modal } from "./Modal";

interface HostKeyChangedDialogProps {
  mismatch: HostKeyMismatch;
  onReplace: () => void;
  onCancel: () => void;
}

/** Shown when the server key differs from the recorded one. Replacing the key
 * requires ticking an explicit confirmation first. */
export function HostKeyChangedDialog({ mismatch, onReplace, onCancel }: HostKeyChangedDialogProps) {
  const [verified, setVerified] = useState(false);

  return (
    <Modal title="Attention : la clé du serveur a changé" tone="danger" wide onCancel={onCancel}>
      <p>
        <strong>
          {mismatch.host}:{mismatch.port}
        </strong>{" "}
        présente une clé différente de celle enregistrée. La connexion a été bloquée.
      </p>
      <p>
        Cause légitime possible : réinstallation, régénération des clés (<code>crypto key generate</code>),
        nouvel équipement à la même adresse. Cause dangereuse : quelqu'un intercepte la connexion
        (attaque de l'homme du milieu).
      </p>
      <dl className="fingerprint">
        <dt>Type</dt>
        <dd>{mismatch.algorithm}</dd>
        <dt>Clé enregistrée</dt>
        <dd>
          <code>{mismatch.oldFingerprint}</code>
        </dd>
        <dt>Clé présentée</dt>
        <dd>
          <code className="danger-text">{mismatch.newFingerprint}</code>
        </dd>
        <dt>Fichier</dt>
        <dd>
          <code>
            {mismatch.file}:{mismatch.line}
          </code>
        </dd>
      </dl>
      <label className="checkbox">
        <input type="checkbox" checked={verified} onChange={(e) => setVerified(e.target.checked)} />
        J'ai vérifié la nouvelle empreinte directement sur l'équipement.
      </label>
      <div className="dialog-actions">
        <button autoFocus onClick={onCancel}>
          Annuler
        </button>
        <button className="danger" disabled={!verified} onClick={onReplace}>
          Remplacer la clé et se reconnecter
        </button>
      </div>
    </Modal>
  );
}
