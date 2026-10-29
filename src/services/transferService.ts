// Typed wrappers around the SFTP commands. Each transfer reports on its own channel.
import { Channel, invoke } from "@tauri-apps/api/core";
import type { SessionId } from "../types/session";
import type { RemoteListing, TransferEvent, TransferId } from "../types/transfers";

/** `path`: "" for the home directory. */
export const listRemote = (sessionId: SessionId, path: string) =>
  invoke<RemoteListing>("sftp_list", { sessionId, path });

export const makeRemoteDir = (sessionId: SessionId, parent: string, name: string) =>
  invoke<string>("sftp_mkdir", { sessionId, parent, name });

export const startDownload = (
  sessionId: SessionId,
  remotePath: string,
  localPath: string,
  onEvent: (event: TransferEvent) => void,
) =>
  invoke<TransferId>("sftp_download", {
    sessionId,
    remotePath,
    localPath,
    onEvent: new Channel<TransferEvent>(onEvent),
  });

export const startUpload = (
  sessionId: SessionId,
  localPath: string,
  remoteDir: string,
  overwrite: boolean,
  onEvent: (event: TransferEvent) => void,
) =>
  invoke<TransferId>("sftp_upload", {
    sessionId,
    localPath,
    remoteDir,
    overwrite,
    onEvent: new Channel<TransferEvent>(onEvent),
  });

export const cancelTransfer = (id: TransferId) => invoke<void>("transfer_cancel", { id });
