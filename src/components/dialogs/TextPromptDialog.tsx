import { useState, type FormEvent } from "react";
import { Modal } from "./Modal";

interface TextPromptDialogProps {
  title: string;
  label: string;
  initial?: string;
  submitLabel: string;
  onSubmit: (value: string) => void;
  onCancel: () => void;
}

export function TextPromptDialog({ title, label, initial = "", submitLabel, onSubmit, onCancel }: TextPromptDialogProps) {
  const [value, setValue] = useState(initial);

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (value.trim()) onSubmit(value.trim());
  }

  return (
    <Modal title={title} onCancel={onCancel}>
      <form onSubmit={handleSubmit} className="dialog-form">
        <label className="field">
          <span>{label}</span>
          <input autoFocus value={value} onChange={(e) => setValue(e.target.value)} maxLength={100} />
        </label>
        <div className="dialog-actions">
          <button type="button" onClick={onCancel}>
            Annuler
          </button>
          <button type="submit" className="primary" disabled={!value.trim()}>
            {submitLabel}
          </button>
        </div>
      </form>
    </Modal>
  );
}
