import type { HostKeyInfo } from "../../types/session";
import { Modal } from "./Modal";

interface HostKeyDialogProps {
  info: HostKeyInfo;
  onAnswer: (accept: boolean) => void;
}

export function HostKeyDialog({ info, onAnswer }: HostKeyDialogProps) {
  return (
    <Modal title="Nouveau serveur : vérifier son identité" onCancel={() => onAnswer(false)}>
      <p>
        C'est la première connexion à <strong>{info.host}:{info.port}</strong>. Ne continue que si
        cette empreinte correspond à celle du serveur attendu.
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
        Sur un serveur Linux : <code>ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub</code>. Sur
        Cisco IOS : <code>show crypto key mypubkey rsa</code>. La clé acceptée est mémorisée : la
        question ne sera plus posée, et tout changement ultérieur sera bloqué.
      </p>
      <div className="dialog-actions">
        <button autoFocus onClick={() => onAnswer(false)}>
          Refuser
        </button>
        <button className="primary" onClick={() => onAnswer(true)}>
          Faire confiance et continuer
        </button>
      </div>
    </Modal>
  );
}
