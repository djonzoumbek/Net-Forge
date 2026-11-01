// Mirrors src-tauri/src/backups/mod.rs.

export interface BackupEntry {
  /** File name, e.g. 2026-11-01_09-30-00.txt (identifier for reading it back). */
  name: string;
  /** "2026-11-01 09:30:00", local time. */
  takenAt: string;
  bytes: number;
  path: string;
}
