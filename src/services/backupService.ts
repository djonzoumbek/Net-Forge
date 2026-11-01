// Typed wrappers around the configuration backup commands.
import { invoke } from "@tauri-apps/api/core";
import type { BackupEntry } from "../types/backups";
import type { SessionId } from "../types/session";

/** Types `command` in the session and saves its output. */
export const captureBackup = (sessionId: SessionId, device: string, command: string, profileId: number | null) =>
  invoke<BackupEntry>("backup_capture", { sessionId, device, command, profileId });

export const listBackups = (device: string) => invoke<BackupEntry[]>("backup_list", { device });

export const readBackup = (device: string, name: string) => invoke<string>("backup_read", { device, name });
