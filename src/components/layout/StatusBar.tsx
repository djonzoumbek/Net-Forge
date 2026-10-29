import type { ConnectionInfo } from "../../types/session";

export type SessionStatus =
  | { kind: "connecting" }
  | { kind: "hostKey" }
  | { kind: "connected"; info: ConnectionInfo }
  | { kind: "closed"; reason: string; byRemote: boolean };

interface StatusBarProps {
  label: string;
  status: SessionStatus;
  sidebarOpen: boolean;
  onToggleSidebar: () => void;
  onDisconnect: () => void;
  onBack: () => void;
}

const statusText: Record<SessionStatus["kind"], string> = {
  connecting: "Connexion…",
  hostKey: "Vérification de la clé d'hôte",
  connected: "Connecté",
  closed: "Déconnecté",
};

function SecurityBadges({ info }: { info: ConnectionInfo }) {
  const algorithms = info.algorithms;
  const details = algorithms
    ? `Échange de clés : ${algorithms.kex}\nClé d'hôte : ${algorithms.hostKey}\nChiffrement : ${algorithms.cipher}\nMAC : ${algorithms.mac}`
    : undefined;
  return (
    <>
      {info.encrypted ? (
        <span className="badge secure" title={details}>
          {info.protocol.toUpperCase()} chiffré
        </span>
      ) : (
        <span className="badge danger" title="Tout circule en clair, mots de passe compris.">
          Non chiffré
        </span>
      )}
      {algorithms && algorithms.legacy.length > 0 && (
        <span
          className="badge legacy"
          title={`Algorithmes obsolètes négociés : ${algorithms.legacy.join(", ")}`}
        >
          ⚠ Legacy : {algorithms.legacy.join(", ")}
        </span>
      )}
    </>
  );
}

export function StatusBar({ label, status, sidebarOpen, onToggleSidebar, onDisconnect, onBack }: StatusBarProps) {
  const closed = status.kind === "closed";
  return (
    <header className="status-bar">
      <button
        className="icon-button"
        onClick={onToggleSidebar}
        title={sidebarOpen ? "Masquer les profils" : "Afficher les profils"}
        aria-label="Profils"
      >
        ☰
      </button>
      <span className={`status-dot ${status.kind}`} aria-hidden />
      <span className="session-label">{label}</span>
      <span className="status-text">{statusText[status.kind]}</span>
      {status.kind === "connected" && <SecurityBadges info={status.info} />}
      <span className="spacer" />
      {closed ? (
        <button onClick={onBack}>Fermer</button>
      ) : (
        <button onClick={onDisconnect}>{status.kind === "connected" ? "Déconnecter" : "Annuler"}</button>
      )}
    </header>
  );
}
