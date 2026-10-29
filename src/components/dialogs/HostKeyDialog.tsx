import type { HostKeyInfo } from "../../types/session";

interface HostKeyDialogProps {
  info: HostKeyInfo;
  onAnswer: (accept: boolean) => void;
}

export function HostKeyDialog({ info, onAnswer }: HostKeyDialogProps) {
  return (
    <div className="dialog-backdrop">
      <div className="dialog" role="alertdialog" aria-labelledby="hostkey-title">
        <h2 id="hostkey-title">Vérifier l'identité du serveur</h2>
        <p>
          Le serveur <strong>{info.host}:{info.port}</strong> présente la clé suivante.
          Ne continue que si cette empreinte correspond à celle du serveur attendu.
        </p>
        <dl className="fingerprint">
          <dt>Type</dt>
          <dd>{info.algorithm}</dd>
          <dt>Empreinte</dt>
          <dd>
            <code>{info.fingerprint}</code>
          </dd>
        </dl>
        <p className="hint">
          Sur le serveur : <code>ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub</code>.
          Cette version ne mémorise pas encore les clés : la question sera reposée à chaque
          connexion.
        </p>
        <div className="dialog-actions">
          <button autoFocus onClick={() => onAnswer(false)}>
            Refuser
          </button>
          <button className="primary" onClick={() => onAnswer(true)}>
            Faire confiance et continuer
          </button>
        </div>
      </div>
    </div>
  );
}
