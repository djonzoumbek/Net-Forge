import { useEffect, type ReactNode } from "react";

interface ModalProps {
  title: string;
  /** Called on Escape. Omit to make the dialog impossible to dismiss that way. */
  onCancel?: () => void;
  tone?: "default" | "danger";
  wide?: boolean;
  children: ReactNode;
}

export function Modal({ title, onCancel, tone = "default", wide, children }: ModalProps) {
  useEffect(() => {
    if (!onCancel) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  return (
    <div className="dialog-backdrop">
      <div
        className={`dialog ${tone === "danger" ? "danger" : ""} ${wide ? "wide" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-label={title}
      >
        <h2>{title}</h2>
        {children}
      </div>
    </div>
  );
}
