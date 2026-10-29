import { useCallback, useState } from "react";
import { HostKeyDialog } from "./components/dialogs/HostKeyDialog";
import { StatusBar, type SessionStatus } from "./components/layout/StatusBar";
import {
  ConnectionForm,
  emptyDraft,
  type ConnectionDraft,
} from "./features/connections/ConnectionForm";
import { TerminalView } from "./features/terminal/TerminalView";
import { answerHostKey, closeSession } from "./services/sessionService";
import type { HostKeyInfo, SessionEvent, SessionId, SshConnectRequest } from "./types/session";
import "./styles/app.css";

type View =
  | { kind: "form"; error?: string }
  | { kind: "session"; request: SshConnectRequest; label: string };

function App() {
  const [view, setView] = useState<View>({ kind: "form" });
  const [draft, setDraft] = useState<ConnectionDraft>(emptyDraft);
  const [status, setStatus] = useState<SessionStatus>({ kind: "connecting" });
  const [sessionId, setSessionId] = useState<SessionId | null>(null);
  const [hostKey, setHostKey] = useState<HostKeyInfo | null>(null);

  function handleConnect(request: SshConnectRequest, nextDraft: ConnectionDraft) {
    setDraft(nextDraft);
    setStatus({ kind: "connecting" });
    setSessionId(null);
    setHostKey(null);
    setView({
      kind: "session",
      request,
      label: `${request.username}@${request.host}:${request.port}`,
    });
  }

  const handleEvent = useCallback((event: SessionEvent) => {
    switch (event.type) {
      case "hostKeyPrompt":
        setHostKey(event.info);
        setStatus({ kind: "hostKey" });
        break;
      case "connected":
        setStatus({ kind: "connected" });
        break;
      case "failed":
        // Back to the form, prefilled, with the reason of the failure.
        setHostKey(null);
        setView({ kind: "form", error: event.message });
        break;
      case "closed":
        setHostKey(null);
        setStatus({ kind: "closed", reason: event.reason, byRemote: event.byRemote });
        break;
    }
  }, []);

  function handleHostKeyAnswer(accept: boolean) {
    setHostKey(null);
    setStatus({ kind: "connecting" });
    if (sessionId !== null) void answerHostKey(sessionId, accept).catch(() => {});
  }

  function handleDisconnect() {
    if (sessionId !== null) void closeSession(sessionId).catch(() => {});
  }

  if (view.kind === "form") {
    return (
      <main className="app form-view">
        <ConnectionForm initial={draft} error={view.error} onConnect={handleConnect} />
      </main>
    );
  }

  return (
    <main className="app session-view">
      <StatusBar
        label={view.label}
        status={status}
        onDisconnect={handleDisconnect}
        onBack={() => setView({ kind: "form" })}
      />
      {status.kind === "closed" && (
        <div className={`banner ${status.byRemote ? "warning" : ""}`} role="status">
          {status.reason}
        </div>
      )}
      <TerminalView request={view.request} onEvent={handleEvent} onSessionId={setSessionId} />
      {hostKey && <HostKeyDialog info={hostKey} onAnswer={handleHostKeyAnswer} />}
    </main>
  );
}

export default App;
