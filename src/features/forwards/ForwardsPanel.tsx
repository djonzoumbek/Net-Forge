import { useCallback, useEffect, useState, type FormEvent } from "react";
import { listForwards, startForward, stopForward } from "../../services/forwardService";
import { toAppError } from "../../services/sessionService";
import type { ForwardInfo, ForwardSpec } from "../../types/forwards";
import type { SessionId } from "../../types/session";
import { ForwardFields } from "./ForwardFields";
import { describeForward, emptyForward, forwardError, isExposed, usageHint } from "./forwards";

interface ForwardsPanelProps {
  sessionId: SessionId;
  visible: boolean;
  /** Profile forwards that could not start at connection. */
  startupErrors: string[];
}

/** Counters change with every connection: refreshed while the panel is shown. */
const REFRESH_MS = 2000;

/** Running port forwards of one SSH session: list, add, stop. */
export function ForwardsPanel({ sessionId, visible, startupErrors }: ForwardsPanelProps) {
  const [forwards, setForwards] = useState<ForwardInfo[]>([]);
  const [draft, setDraft] = useState<ForwardSpec>(() => emptyForward());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setForwards(await listForwards(sessionId));
    } catch (e) {
      setError(toAppError(e).message);
    }
  }, [sessionId]);

  useEffect(() => {
    if (!visible) return;
    // setState happens after the await (same pattern as the SFTP panel).
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void refresh();
    const timer = setInterval(() => void refresh(), REFRESH_MS);
    return () => clearInterval(timer);
  }, [visible, refresh]);

  async function add(event: FormEvent) {
    event.preventDefault();
    const invalid = forwardError(draft);
    if (invalid) {
      setError(invalid);
      return;
    }
    setBusy(true);
    try {
      await startForward(sessionId, draft);
      setError(null);
      setDraft(emptyForward(draft.kind));
      await refresh();
    } catch (e) {
      setError(toAppError(e).message);
    } finally {
      setBusy(false);
    }
  }

  async function stop(forward: ForwardInfo) {
    try {
      await stopForward(sessionId, forward.id);
      await refresh();
    } catch (e) {
      setError(toAppError(e).message);
    }
  }

  return (
    <section className={`forwards-panel ${visible ? "" : "hidden"}`} aria-label="Redirections de port">
      {startupErrors.map((message) => (
        <p key={message} className="form-error" role="alert">
          {message}
        </p>
      ))}
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <div className="forward-list">
        {forwards.length === 0 ? (
          <p className="hint">
            Aucune redirection active. Une redirection locale rend un service joignable depuis le serveur SSH
            (interface web d'un équipement, base de données…) sur un port de cette machine ; un proxy SOCKS fait
            passer un navigateur par le serveur.
          </p>
        ) : (
          forwards.map((forward) => (
            <div key={forward.id} className="forward">
              <span className="badge neutral">{forward.spec.kind === "dynamic" ? "SOCKS" : "-L"}</span>
              {isExposed(forward.spec) && (
                <span className="badge danger" title="Écoute sur le réseau local">
                  Réseau
                </span>
              )}
              <span className="forward-route">{describeForward(forward.spec, forward.localPort)}</span>
              <span className="forward-usage">{usageHint(forward.spec, forward.localPort)}</span>
              <span className="forward-stats" title="Connexions en cours / depuis le démarrage">
                {forward.activeConnections} / {forward.totalConnections}
              </span>
              {forward.lastError && (
                <span className="forward-error" title={forward.lastError}>
                  ⚠ {forward.lastError}
                </span>
              )}
              <button onClick={() => void stop(forward)}>Arrêter</button>
            </div>
          ))
        )}
      </div>
      <form className="forward-add" onSubmit={add}>
        <ForwardFields spec={draft} onChange={setDraft} />
        <button type="submit" className="primary" disabled={busy}>
          Démarrer
        </button>
      </form>
    </section>
  );
}
