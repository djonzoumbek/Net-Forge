import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { downloadDir, join } from "@tauri-apps/api/path";
import { open, save } from "@tauri-apps/plugin-dialog";
import { toAppError } from "../../services/sessionService";
import {
  cancelTransfer,
  listRemote,
  makeRemoteDir,
  startDownload,
  startUpload,
} from "../../services/transferService";
import type { SessionId } from "../../types/session";
import type { RemoteEntry, RemoteListing, TransferEvent, TransferId } from "../../types/transfers";
import { baseName, formatBytes, joinRemote, parentRemote, percent } from "./remote";

type TransferStatus =
  | { kind: "running"; done: number; total: number | null }
  | { kind: "done"; bytes: number }
  | { kind: "cancelled" }
  | { kind: "failed"; code: string; message: string };

interface Transfer {
  /** Local key (the backend id is only known once the command returns). */
  key: number;
  id: TransferId | null;
  direction: "up" | "down";
  name: string;
  /** Upload: the local file and remote directory, to retry with "replace". */
  localPath: string;
  remoteDir: string;
  status: TransferStatus;
}

interface SftpPanelProps {
  sessionId: SessionId;
  /** Hidden but mounted: running transfers keep their progress display. */
  visible: boolean;
}

function statusFromEvent(event: TransferEvent): TransferStatus {
  switch (event.type) {
    case "progress":
      return { kind: "running", done: event.done, total: event.total };
    case "done":
      return { kind: "done", bytes: event.bytes };
    case "cancelled":
      return { kind: "cancelled" };
    case "failed":
      return { kind: "failed", code: event.code, message: event.message };
  }
}

function entryIcon(entry: RemoteEntry): string {
  return entry.kind === "dir" ? "📁" : entry.kind === "symlink" ? "🔗" : "📄";
}

/** Remote file browser and transfer list of one SSH session. */
export function SftpPanel({ sessionId, visible }: SftpPanelProps) {
  const [listing, setListing] = useState<RemoteListing | null>(null);
  const [pathInput, setPathInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [newDir, setNewDir] = useState<string | null>(null);
  const [transfers, setTransfers] = useState<Transfer[]>([]);
  const nextKey = useRef(1);
  // Read by transfer callbacks that outlive renders (refresh after an upload).
  const currentPath = useRef<string | null>(null);

  const load = useCallback(
    async (path: string) => {
      setLoading(true);
      try {
        const result = await listRemote(sessionId, path);
        setListing(result);
        setPathInput(result.path);
        currentPath.current = result.path;
        setError(null);
      } catch (e) {
        setError(toAppError(e).message);
      } finally {
        setLoading(false);
      }
    },
    [sessionId],
  );

  useEffect(() => {
    // Initial load (home directory); setState happens after the await.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void load("");
  }, [load]);

  const updateTransfer = (key: number, change: Partial<Transfer>) =>
    setTransfers((list) => list.map((t) => (t.key === key ? { ...t, ...change } : t)));

  /** Adds a row and starts the transfer; the row follows the transfer's events. */
  async function run(
    row: Omit<Transfer, "key" | "id" | "status">,
    start: (onEvent: (event: TransferEvent) => void) => Promise<TransferId>,
  ) {
    const key = nextKey.current++;
    setTransfers((list) => [...list, { ...row, key, id: null, status: { kind: "running", done: 0, total: null } }]);
    const onEvent = (event: TransferEvent) => {
      updateTransfer(key, { status: statusFromEvent(event) });
      // A finished upload into the displayed directory: show the new file.
      if (event.type === "done" && row.direction === "up" && row.remoteDir === currentPath.current) {
        void load(row.remoteDir);
      }
    };
    try {
      updateTransfer(key, { id: await start(onEvent) });
    } catch (e) {
      const { code, message } = toAppError(e);
      updateTransfer(key, { status: { kind: "failed", code, message } });
    }
  }

  function upload(localPath: string, remoteDir: string, overwrite: boolean) {
    void run({ direction: "up", name: baseName(localPath), localPath, remoteDir }, (onEvent) =>
      startUpload(sessionId, localPath, remoteDir, overwrite, onEvent),
    );
  }

  async function chooseUploads() {
    if (!listing) return;
    const selected = await open({ multiple: true, directory: false, title: `Envoyer vers ${listing.path}` });
    const paths = selected === null ? [] : Array.isArray(selected) ? selected : [selected];
    for (const path of paths) upload(path, listing.path, false);
  }

  async function download(entry: RemoteEntry) {
    if (!listing) return;
    const remotePath = joinRemote(listing.path, entry.name);
    let defaultPath = entry.name;
    try {
      defaultPath = await join(await downloadDir(), entry.name);
    } catch {
      // No Downloads directory: the dialog starts in its default place.
    }
    const localPath = await save({ defaultPath, title: `Enregistrer ${entry.name}` });
    if (!localPath) return;
    void run({ direction: "down", name: entry.name, localPath, remoteDir: listing.path }, (onEvent) =>
      startDownload(sessionId, remotePath, localPath, onEvent),
    );
  }

  async function createDir(event: FormEvent) {
    event.preventDefault();
    if (!listing || !newDir?.trim()) return;
    try {
      await makeRemoteDir(sessionId, listing.path, newDir.trim());
      setNewDir(null);
      await load(listing.path);
    } catch (e) {
      setError(toAppError(e).message);
    }
  }

  const finished = transfers.some((t) => t.status.kind !== "running");

  return (
    <section className={`sftp-panel ${visible ? "" : "hidden"}`} aria-label="Transfert de fichiers">
      <div className="sftp-toolbar">
        <button
          onClick={() => listing && void load(parentRemote(listing.path))}
          disabled={!listing || listing.path === "/"}
          title="Dossier parent"
          aria-label="Dossier parent"
        >
          ↑
        </button>
        <form
          className="sftp-path"
          onSubmit={(e) => {
            e.preventDefault();
            void load(pathInput.trim());
          }}
        >
          <input value={pathInput} onChange={(e) => setPathInput(e.target.value)} aria-label="Chemin distant" spellCheck={false} />
        </form>
        <button onClick={() => listing && void load(listing.path)} title="Actualiser" aria-label="Actualiser">
          ⟳
        </button>
        <button onClick={() => void chooseUploads()} disabled={!listing}>
          Envoyer…
        </button>
        <button onClick={() => setNewDir(newDir === null ? "" : null)} disabled={!listing}>
          Nouveau dossier
        </button>
      </div>

      {newDir !== null && (
        <form className="sftp-toolbar" onSubmit={createDir}>
          <input
            autoFocus
            value={newDir}
            onChange={(e) => setNewDir(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") setNewDir(null);
            }}
            placeholder="Nom du dossier"
            aria-label="Nom du nouveau dossier"
          />
          <button type="submit" className="primary">
            Créer
          </button>
        </form>
      )}

      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}

      <div className="sftp-list" aria-busy={loading}>
        {listing?.entries.length === 0 && <p className="hint">Dossier vide.</p>}
        <table>
          <tbody>
            {listing?.entries.map((entry) => (
              <tr
                key={entry.name}
                className={entry.kind === "dir" ? "dir" : ""}
                onDoubleClick={() => entry.kind === "dir" && void load(joinRemote(listing.path, entry.name))}
                title={entry.kind === "dir" ? "Double-clic pour ouvrir" : undefined}
              >
                <td className="name">
                  <span aria-hidden>{entryIcon(entry)}</span> {entry.name}
                </td>
                <td className="size">{entry.kind === "file" && entry.size !== null ? formatBytes(entry.size) : ""}</td>
                <td className="perms">{entry.permissions}</td>
                <td className="actions">
                  {entry.kind !== "dir" && (
                    <button onClick={() => void download(entry)} title="Télécharger" aria-label={`Télécharger ${entry.name}`}>
                      ↓
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {transfers.length > 0 && (
        <div className="transfers">
          <div className="transfers-header">
            <span>Transferts</span>
            {finished && (
              <button
                className="link"
                onClick={() => setTransfers((list) => list.filter((t) => t.status.kind === "running"))}
              >
                Effacer les terminés
              </button>
            )}
          </div>
          {transfers.map((t) => (
            <TransferRow
              key={t.key}
              transfer={t}
              onCancel={() => t.id !== null && void cancelTransfer(t.id).catch(() => {})}
              onReplace={() => {
                setTransfers((list) => list.filter((other) => other.key !== t.key));
                upload(t.localPath, t.remoteDir, true);
              }}
            />
          ))}
        </div>
      )}
    </section>
  );
}

function TransferRow({ transfer, onCancel, onReplace }: { transfer: Transfer; onCancel: () => void; onReplace: () => void }) {
  const { status } = transfer;
  const arrow = transfer.direction === "up" ? "↑" : "↓";
  const exists = status.kind === "failed" && status.code === "remote_exists";
  return (
    <div className={`transfer ${status.kind}`}>
      <span className="transfer-name" title={transfer.localPath}>
        {arrow} {transfer.name}
      </span>
      {status.kind === "running" && (
        <>
          <progress max={100} value={percent(status.done, status.total) ?? undefined} />
          <span className="transfer-info">
            {formatBytes(status.done)}
            {status.total !== null && ` / ${formatBytes(status.total)}`}
          </span>
          <button onClick={onCancel} disabled={transfer.id === null}>
            Annuler
          </button>
        </>
      )}
      {status.kind === "done" && <span className="transfer-info ok">Terminé ({formatBytes(status.bytes)})</span>}
      {status.kind === "cancelled" && <span className="transfer-info">Annulé</span>}
      {status.kind === "failed" && (
        <>
          <span className="transfer-info danger-text" title={status.message}>
            {status.message}
          </span>
          {exists && (
            <button onClick={onReplace} title="Écraser le fichier distant">
              Remplacer
            </button>
          )}
        </>
      )}
    </div>
  );
}
