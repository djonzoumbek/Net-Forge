import { useCallback, useEffect, useMemo, useState } from "react";
import { Modal } from "../../components/dialogs/Modal";
import { captureBackup, listBackups, readBackup } from "../../services/backupService";
import { toAppError } from "../../services/sessionService";
import type { BackupEntry } from "../../types/backups";
import type { SessionId } from "../../types/session";
import { formatBytes } from "../transfers/remote";
import { diffLines, diffStats, hunks, splitLines } from "./diff";
import { backupPresets, DEFAULT_BACKUP_COMMAND } from "./presets";

interface BackupsDialogProps {
  /** Folder name of the device: the profile name, or the target of a quick connection. */
  device: string;
  /** `null` when the session is not connected: backups can only be viewed. */
  sessionId: SessionId | null;
  profileId: number | null;
  /** Command remembered for the profile ("" if none yet). */
  savedCommand: string;
  onClose: () => void;
}

type View =
  | { kind: "none" }
  | { kind: "show"; entry: BackupEntry; text: string }
  | { kind: "diff"; older: BackupEntry; newer: BackupEntry; oldText: string; newText: string };

/** Capture, list, view and compare the configuration backups of one device. */
export function BackupsDialog({ device, sessionId, profileId, savedCommand, onClose }: BackupsDialogProps) {
  const [command, setCommand] = useState(savedCommand || DEFAULT_BACKUP_COMMAND);
  const [backups, setBackups] = useState<BackupEntry[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [view, setView] = useState<View>({ kind: "none" });
  const [ignoreVolatile, setIgnoreVolatile] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const list = await listBackups(device);
      setBackups(list);
      return list;
    } catch (e) {
      setError(toAppError(e).message);
      return [];
    }
  }, [device]);

  useEffect(() => {
    // setState happens after the await (same pattern as the SFTP panel).
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void refresh();
  }, [refresh]);

  async function compare(older: BackupEntry, newer: BackupEntry) {
    try {
      const [oldText, newText] = await Promise.all([readBackup(device, older.name), readBackup(device, newer.name)]);
      setView({ kind: "diff", older, newer, oldText, newText });
      setError(null);
    } catch (e) {
      setError(toAppError(e).message);
    }
  }

  async function show(entry: BackupEntry) {
    try {
      setView({ kind: "show", entry, text: await readBackup(device, entry.name) });
      setError(null);
    } catch (e) {
      setError(toAppError(e).message);
    }
  }

  async function capture() {
    if (sessionId === null) return;
    setBusy(true);
    setNotice(null);
    try {
      const entry = await captureBackup(sessionId, device, command, profileId);
      const list = await refresh();
      setError(null);
      // Compare at once with the previous backup: the usual question is "what changed?".
      const previous = list.find((b) => b.name !== entry.name && b.name < entry.name);
      if (previous) await compare(previous, entry);
      else {
        setNotice(`Première sauvegarde enregistrée : ${entry.path}`);
        await show(entry);
      }
    } catch (e) {
      setError(toAppError(e).message);
    } finally {
      setBusy(false);
    }
  }

  function toggle(name: string) {
    setSelected((list) => (list.includes(name) ? list.filter((n) => n !== name) : [...list.slice(-1), name]));
  }

  function compareSelected() {
    const [a, b] = selected.map((name) => backups.find((x) => x.name === name)).filter((x) => x !== undefined);
    if (!a || !b) return;
    // Names sort chronologically: the older one on the left.
    if (a.name < b.name) void compare(a, b);
    else void compare(b, a);
  }

  return (
    <Modal title={`Sauvegardes de configuration — ${device}`} onCancel={onClose} wide>
      <div className="backups">
        {sessionId !== null ? (
          <div className="backup-capture">
            <div className="field-row">
              <label className="field grow">
                <span>Commande (tapée dans la session, à l'invite)</span>
                <input value={command} onChange={(e) => setCommand(e.target.value)} spellCheck={false} />
              </label>
              <button className="primary" onClick={() => void capture()} disabled={busy || command.trim() === ""}>
                {busy ? "Sauvegarde…" : "Sauvegarder maintenant"}
              </button>
            </div>
            <div className="presets">
              {backupPresets.map((preset) => (
                <button key={preset.label} type="button" onClick={() => setCommand(preset.command)}>
                  {preset.label}
                </button>
              ))}
            </div>
            <p className="hint">
              La session doit être à l'invite (mode privilégié pour Cisco : <code>enable</code>). La sortie s'affiche
              aussi dans le terminal ; le pager est répondu automatiquement.
            </p>
          </div>
        ) : (
          <p className="hint">Session déconnectée : consultation et comparaison seulement.</p>
        )}
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        {notice && <p className="hint">{notice}</p>}

        <div className="backup-body">
          <div className="backup-list">
            {backups.length === 0 ? (
              <p className="hint">Aucune sauvegarde pour cet équipement.</p>
            ) : (
              <>
                <ul>
                  {backups.map((entry, index) => (
                    <li key={entry.name} className={selected.includes(entry.name) ? "selected" : ""}>
                      <label>
                        <input
                          type="checkbox"
                          checked={selected.includes(entry.name)}
                          onChange={() => toggle(entry.name)}
                        />
                        <span>{entry.takenAt}</span>
                      </label>
                      <span className="backup-size">{formatBytes(entry.bytes)}</span>
                      <button className="link" onClick={() => void show(entry)}>
                        Voir
                      </button>
                      {index + 1 < backups.length && (
                        <button className="link" onClick={() => void compare(backups[index + 1], entry)}>
                          Diff préc.
                        </button>
                      )}
                    </li>
                  ))}
                </ul>
                <button onClick={compareSelected} disabled={selected.length !== 2}>
                  Comparer la sélection
                </button>
              </>
            )}
          </div>
          <BackupView view={view} ignoreVolatile={ignoreVolatile} onIgnoreVolatile={setIgnoreVolatile} />
        </div>
      </div>
      <div className="dialog-actions">
        <button onClick={onClose}>Fermer</button>
      </div>
    </Modal>
  );
}

function BackupView({
  view,
  ignoreVolatile,
  onIgnoreVolatile,
}: {
  view: View;
  ignoreVolatile: boolean;
  onIgnoreVolatile: (value: boolean) => void;
}) {
  const diff = useMemo(() => {
    if (view.kind !== "diff") return null;
    const ops = diffLines(splitLines(view.oldText, ignoreVolatile), splitLines(view.newText, ignoreVolatile));
    return { stats: diffStats(ops), hunks: hunks(ops) };
  }, [view, ignoreVolatile]);

  if (view.kind === "none") {
    return <div className="backup-view empty hint">Choisis une sauvegarde à afficher, ou deux à comparer.</div>;
  }
  if (view.kind === "show") {
    return (
      <div className="backup-view">
        <div className="backup-view-header">
          <span>{view.entry.takenAt}</span>
          <span className="hint" title={view.entry.path}>
            {view.entry.path}
          </span>
        </div>
        <pre className="backup-text">{view.text}</pre>
      </div>
    );
  }
  return (
    <div className="backup-view">
      <div className="backup-view-header">
        <span>
          {view.older.takenAt} → {view.newer.takenAt}
        </span>
        {diff && (
          <span>
            <span className="diff-added-count">+{diff.stats.added}</span>{" "}
            <span className="diff-removed-count">−{diff.stats.removed}</span>
          </span>
        )}
        <label className="checkbox">
          <input type="checkbox" checked={ignoreVolatile} onChange={(e) => onIgnoreVolatile(e.target.checked)} />
          Ignorer horodatages et compteurs
        </label>
      </div>
      {diff && diff.hunks.length === 0 ? (
        <p className="hint">Aucune différence.</p>
      ) : (
        <pre className="backup-text diff">
          {diff?.hunks.map((hunk) => (
            <div key={`${hunk.oldStart}-${hunk.newStart}`}>
              <div className="diff-hunk">
                @@ -{hunk.oldStart} +{hunk.newStart} @@
              </div>
              {hunk.ops.map((op, i) => (
                <div key={i} className={`diff-${op.kind}`}>
                  {op.kind === "added" ? "+ " : op.kind === "removed" ? "- " : "  "}
                  {op.text}
                </div>
              ))}
            </div>
          ))}
        </pre>
      )}
    </div>
  );
}
