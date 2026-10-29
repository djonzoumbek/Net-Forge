import { pastedLineCount } from "../../features/terminal/paste";
import { Modal } from "./Modal";

interface PasteConfirmDialogProps {
  text: string;
  target: string;
  onConfirm: () => void;
  onCancel: () => void;
}

const PREVIEW_LINES = 20;

export function PasteConfirmDialog({ text, target, onConfirm, onCancel }: PasteConfirmDialogProps) {
  const lines = text.replace(/\r\n?/g, "\n").replace(/\n$/, "").split("\n");
  const count = pastedLineCount(text);
  return (
    <Modal title={`Coller ${count} lignes ?`} onCancel={onCancel} wide>
      <p>
        Le texte sera envoyé à <strong>{target}</strong> et chaque ligne exécutée comme une commande.
      </p>
      <pre className="preview">
        {lines.slice(0, PREVIEW_LINES).join("\n")}
        {lines.length > PREVIEW_LINES && `\n… (${lines.length - PREVIEW_LINES} lignes de plus)`}
      </pre>
      <div className="dialog-actions">
        <button autoFocus onClick={onCancel}>
          Annuler
        </button>
        <button className="primary" onClick={onConfirm}>
          Coller {count} lignes
        </button>
      </div>
    </Modal>
  );
}
