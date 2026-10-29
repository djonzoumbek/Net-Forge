import { StatusBar } from "../../components/layout/StatusBar";
import { closeSession } from "../../services/sessionService";
import type { SessionEvent, SessionId } from "../../types/session";
import { TerminalView } from "../terminal/TerminalView";
import type { Tab } from "./tabs";

interface SessionPanelProps {
  tab: Tab;
  active: boolean;
  sidebarOpen: boolean;
  onToggleSidebar: () => void;
  onEvent: (tab: Tab, event: SessionEvent) => void;
  onSessionId: (tab: Tab, id: SessionId) => void;
  onRetry: (tab: Tab) => void;
  onClose: (tab: Tab) => void;
  onEditProfile: (tab: Tab) => void;
}

/** One session: status bar, information banner and terminal. Hidden tabs stay
 * mounted (display: none) so their session keeps running. */
export function SessionPanel({
  tab,
  active,
  sidebarOpen,
  onToggleSidebar,
  onEvent,
  onSessionId,
  onRetry,
  onClose,
  onEditProfile,
}: SessionPanelProps) {
  const { status } = tab;
  return (
    <div className={`session-panel ${active ? "" : "inactive"}`} role="tabpanel">
      <StatusBar
        title={tab.target.title}
        status={status}
        sidebarOpen={sidebarOpen}
        onToggleSidebar={onToggleSidebar}
        onDisconnect={() => {
          if (tab.sessionId !== null) void closeSession(tab.sessionId).catch(() => {});
        }}
        onCloseTab={() => onClose(tab)}
      />
      {(status.kind === "failed" || status.kind === "closed") && (
        <div
          className={`banner ${status.kind === "failed" || status.byRemote ? "warning" : ""}`}
          role="status"
        >
          <span>{status.kind === "failed" ? status.message : status.reason}</span>
          <button className="link" onClick={() => onRetry(tab)}>
            {status.kind === "failed" ? "Réessayer" : "Se reconnecter"}
          </button>
          {status.kind === "failed" && tab.target.profile && (
            <button className="link" onClick={() => onEditProfile(tab)}>
              Modifier le profil
            </button>
          )}
        </div>
      )}
      <TerminalView
        key={tab.attempt}
        open={tab.target.open}
        active={active}
        onEvent={(event) => onEvent(tab, event)}
        onSessionId={(id) => onSessionId(tab, id)}
      />
    </div>
  );
}
