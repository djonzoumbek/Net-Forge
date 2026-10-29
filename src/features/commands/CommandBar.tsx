import type { CommandButton } from "../../types/profiles";

interface CommandBarProps {
  buttons: CommandButton[];
  /** Buttons only send when the session is connected. */
  enabled: boolean;
  onRun: (button: CommandButton) => void;
  onEdit: (button: CommandButton) => void;
  onAdd: () => void;
  onBroadcast: () => void;
}

export function CommandBar({ buttons, enabled, onRun, onEdit, onAdd, onBroadcast }: CommandBarProps) {
  return (
    <div className="command-bar" role="toolbar" aria-label="Commandes enregistrées">
      {buttons.map((button) => (
        <button
          key={button.id}
          disabled={!enabled}
          title={`${button.commands}\n\nClic droit : modifier`}
          onClick={() => onRun(button)}
          onContextMenu={(e) => {
            e.preventDefault();
            onEdit(button);
          }}
        >
          {button.label}
        </button>
      ))}
      <button className="add" title="Nouveau bouton de commandes" onClick={onAdd}>
        + Bouton
      </button>
      <span className="spacer" />
      <button disabled={!enabled} title="Envoyer une commande à plusieurs sessions" onClick={onBroadcast}>
        Diffuser…
      </button>
    </div>
  );
}
