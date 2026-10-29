export type SessionStatus =
  | { kind: "connecting" }
  | { kind: "hostKey" }
  | { kind: "connected" }
  | { kind: "closed"; reason: string; byRemote: boolean };

interface StatusBarProps {
  label: string;
  status: SessionStatus;
  onDisconnect: () => void;
  onBack: () => void;
}

const statusText: Record<SessionStatus["kind"], string> = {
  connecting: "Connexion…",
  hostKey: "Vérification de la clé d'hôte",
  connected: "Connecté · SSH chiffré",
  closed: "Déconnecté",
};

export function StatusBar({ label, status, onDisconnect, onBack }: StatusBarProps) {
  const closed = status.kind === "closed";
  return (
    <header className="status-bar">
      <span className={`status-dot ${status.kind}`} aria-hidden />
      <span className="session-label">{label}</span>
      <span className="status-text">{statusText[status.kind]}</span>
      <span className="spacer" />
      {closed ? (
        <button onClick={onBack}>Nouvelle connexion</button>
      ) : (
        <button onClick={onDisconnect}>
          {status.kind === "connected" ? "Déconnecter" : "Annuler"}
        </button>
      )}
    </header>
  );
}
