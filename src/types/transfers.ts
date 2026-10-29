// Mirrors src-tauri/src/transfers/mod.rs.

export type TransferId = number;

export type TransferEvent =
  /** `total`: null when the server does not give the size. */
  | { type: "progress"; done: number; total: number | null }
  | { type: "done"; bytes: number }
  | { type: "cancelled" }
  | { type: "failed"; code: string; message: string };

export type EntryKind = "dir" | "file" | "symlink" | "other";

export interface RemoteEntry {
  name: string;
  kind: EntryKind;
  size: number | null;
  /** Unix timestamp (seconds). */
  modified: number | null;
  /** `ls -l` style, e.g. rwxr-xr-x */
  permissions: string | null;
}

export interface RemoteListing {
  /** Absolute path resolved by the server. */
  path: string;
  entries: RemoteEntry[];
}
