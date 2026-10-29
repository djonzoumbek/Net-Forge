import type { TabStatus } from "../../features/sessions/tabs";
import type { ConnectionInfo } from "../../types/session";

interface StatusBarProps {
  title: string;
  status: TabStatus;
  sidebarOpen: boolean;
  onToggleSidebar: () => void;
  onDisconnect: () => void;
  onCloseTab: () => void;
}

const statusText: Record<TabStatus["kind"], string> = {
  connecting: "Connexion…",
  hostKey: "Vérification de la clé d'hôte",
  connected: "Connecté",
  hostKeyChanged: "Clé d'hôte modifiée",
  failed: "Échec de connexion",
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

export function StatusBar({ title, status, sidebarOpen, onToggleSidebar, onDisconnect, onCloseTab }: StatusBarProps) {
  const live = status.kind === "connecting" || status.kind === "hostKey" || status.kind === "connected";
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
      <span className="session-label">{title}</span>
      <span className="status-text">{statusText[status.kind]}</span>
      {status.kind === "connected" && <SecurityBadges info={status.info} />}
      <span className="spacer" />
      {live ? (
        <button onClick={onDisconnect}>{status.kind === "connected" ? "Déconnecter" : "Annuler"}</button>
      ) : (
        <button onClick={onCloseTab}>Fermer l'onglet</button>
      )}
    </header>
  );
}
