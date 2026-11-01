import { useEffect, useState } from "react";
import { StatusBar } from "../../components/layout/StatusBar";
import { closeSession } from "../../services/sessionService";
import type { CommandButton } from "../../types/profiles";
import type { SessionEvent, SessionId } from "../../types/session";
import { CommandBar } from "../commands/CommandBar";
import { ForwardsPanel } from "../forwards/ForwardsPanel";
import { useProfileForwards } from "../forwards/useProfileForwards";
import { TerminalView, type TerminalAppearance } from "../terminal/TerminalView";
import { SftpPanel } from "../transfers/SftpPanel";
import { MAX_AUTO_RECONNECT, type Tab } from "./tabs";

interface SessionPanelProps {
  tab: Tab;
  active: boolean;
  sidebarOpen: boolean;
  buttons: CommandButton[];
  onToggleSidebar: () => void;
  onEvent: (tab: Tab, event: SessionEvent) => void;
  onSessionId: (tab: Tab, id: SessionId) => void;
  onRetry: (tab: Tab) => void;
  onCancelReconnect: (tab: Tab) => void;
  onClose: (tab: Tab) => void;
  onEditProfile: (tab: Tab) => void;
  onRunButton: (tab: Tab, button: CommandButton) => void;
  onEditButton: (button?: CommandButton) => void;
  onBroadcast: (tab: Tab) => void;
  onPasteRequest: (tab: Tab, text: string, paste: () => void) => void;
  appearance: TerminalAppearance;
  /** See TerminalView: `null` when the search bar is closed. */
  searchToken: number | null;
  onSearchClose: () => void;
}

const NO_FORWARDS: never[] = [];

/** Seconds left before `at` (a Date.now() timestamp), refreshed every second. */
function useCountdown(at: number | null): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (at === null) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [at]);
  return at === null ? 0 : Math.max(0, Math.ceil((at - now) / 1000));
}

/** One session: status bar, command bar, information banner and terminal.
 * Hidden tabs stay mounted (display: none) so their session keeps running. */
export function SessionPanel(props: SessionPanelProps) {
  const { tab, active } = props;
  const { status } = tab;
  const secondsLeft = useCountdown(tab.reconnectAt);
  const connected = status.kind === "connected";
  // SFTP panel: mounted on first opening, then only hidden, so that running
  // transfers keep their progress display when it is closed.
  const [filesOpen, setFilesOpen] = useState(false);
  const [filesMounted, setFilesMounted] = useState(false);
  const sftpSession = connected && status.info.protocol === "ssh" ? tab.sessionId : null;
  // Port forwards: same lifetime rule as the SFTP panel.
  const [forwardsOpen, setForwardsOpen] = useState(false);
  const [forwardsMounted, setForwardsMounted] = useState(false);
  const forwardErrors = useProfileForwards(sftpSession, tab.target.profile?.portForwards ?? NO_FORWARDS);

  return (
    <div className={`session-panel ${active ? "" : "inactive"}`} role="tabpanel">
      <StatusBar
        title={tab.target.title}
        status={status}
        sidebarOpen={props.sidebarOpen}
        onToggleSidebar={props.onToggleSidebar}
        onDisconnect={() => {
          if (tab.sessionId !== null) void closeSession(tab.sessionId).catch(() => {});
        }}
        onCloseTab={() => props.onClose(tab)}
        filesOpen={filesOpen}
        onToggleFiles={() => {
          setFilesMounted(true);
          setFilesOpen((open) => !open);
        }}
        forwardsOpen={forwardsOpen}
        forwardsAlert={forwardErrors.length > 0}
        onToggleForwards={() => {
          setForwardsMounted(true);
          setForwardsOpen((open) => !open);
        }}
      />
      <CommandBar
        buttons={props.buttons}
        enabled={connected}
        onRun={(button) => props.onRunButton(tab, button)}
        onEdit={(button) => props.onEditButton(button)}
        onAdd={() => props.onEditButton()}
        onBroadcast={() => props.onBroadcast(tab)}
      />
      {tab.reconnectAt !== null ? (
        <div className="banner warning" role="status">
          <span>
            Connexion perdue. Reconnexion automatique dans {secondsLeft} s (tentative{" "}
            {tab.autoRetries + 1}/{MAX_AUTO_RECONNECT}).
          </span>
          <button className="link" onClick={() => props.onRetry(tab)}>
            Reconnecter maintenant
          </button>
          <button className="link" onClick={() => props.onCancelReconnect(tab)}>
            Annuler
          </button>
        </div>
      ) : (
        (status.kind === "failed" || status.kind === "closed") && (
          <div
            className={`banner ${status.kind === "failed" || status.byRemote ? "warning" : ""}`}
            role="status"
          >
            <span>{status.kind === "failed" ? status.message : status.reason}</span>
            <button className="link" onClick={() => props.onRetry(tab)}>
              {status.kind === "failed" ? "Réessayer" : "Se reconnecter"}
            </button>
            {status.kind === "failed" && tab.target.profile && (
              <button className="link" onClick={() => props.onEditProfile(tab)}>
                Modifier le profil
              </button>
            )}
          </div>
        )
      )}
      <TerminalView
        key={tab.attempt}
        open={tab.target.open}
        active={active}
        onEvent={(event) => props.onEvent(tab, event)}
        onSessionId={(id) => props.onSessionId(tab, id)}
        onPasteRequest={(text, paste) => props.onPasteRequest(tab, text, paste)}
        appearance={props.appearance}
        searchToken={props.searchToken}
        onSearchClose={props.onSearchClose}
      />
      {filesMounted && sftpSession !== null && (
        // New session (reconnection): new SFTP channel, fresh panel.
        <SftpPanel key={sftpSession} sessionId={sftpSession} visible={filesOpen} />
      )}
      {(forwardsMounted || forwardErrors.length > 0) && sftpSession !== null && (
        <ForwardsPanel
          key={sftpSession}
          sessionId={sftpSession}
          visible={forwardsOpen}
          startupErrors={forwardErrors}
        />
      )}
    </div>
  );
}
