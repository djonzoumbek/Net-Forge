import type { TabStatus } from "../../features/sessions/tabs";
import type { ConnectionInfo } from "../../types/session";

interface StatusBarProps {
  title: string;
  status: TabStatus;
  sidebarOpen: boolean;
  onToggleSidebar: () => void;
  onDisconnect: () => void;
  onCloseTab: () => void;
  /** SFTP panel toggle, shown for connected SSH sessions only. */
  filesOpen: boolean;
  onToggleFiles: () => void;
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
  if (info.protocol === "serial") {
    return (
      <span className="badge neutral" title="Câble console local : pas de réseau, rien à chiffrer.">
        Série
      </span>
    );
  }
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

export function StatusBar(props: StatusBarProps) {
  const { title, status, sidebarOpen, onToggleSidebar, onDisconnect, onCloseTab } = props;
  const sftpAvailable = status.kind === "connected" && status.info.protocol === "ssh";
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
      {status.kind === "connected" && status.logFile && (
        <span className="badge recording" title={`Journal de session : ${status.logFile}`}>
          ● Journal
        </span>
      )}
      {status.kind === "connected" && status.logError && (
        <span className="badge danger" title={status.logError}>
          Journal impossible
        </span>
      )}
      <span className="spacer" />
      {sftpAvailable && (
        <button
          className={props.filesOpen ? "toggled" : ""}
          aria-pressed={props.filesOpen}
          onClick={props.onToggleFiles}
          title="Transfert de fichiers (SFTP) sur cette connexion"
        >
          Fichiers
        </button>
      )}
      {live ? (
        <button onClick={onDisconnect}>{status.kind === "connected" ? "Déconnecter" : "Annuler"}</button>
      ) : (
        <button onClick={onCloseTab}>Fermer l'onglet</button>
      )}
    </header>
  );
}
