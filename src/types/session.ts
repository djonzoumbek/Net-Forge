// Types mirrored from the Rust side (src-tauri/src/sessions, commands, errors).

export type SessionId = number;

export interface TermSize {
  cols: number;
  rows: number;
}

export type AuthRequest =
  | { method: "password"; password: string }
  | { method: "keyFile"; path: string; passphrase?: string };

export interface SshConnectRequest {
  host: string;
  port: number;
  username: string;
  auth: AuthRequest;
}

export interface HostKeyInfo {
  host: string;
  port: number;
  algorithm: string;
  /** OpenSSH format, e.g. "SHA256:..." */
  fingerprint: string;
}

export type SessionEvent =
  | { type: "hostKeyPrompt"; info: HostKeyInfo }
  | { type: "connected" }
  | { type: "failed"; code: string; message: string }
  | { type: "closed"; reason: string; byRemote: boolean };

/** Error shape returned by every Tauri command (`AppError` in Rust). */
export interface AppError {
  code: string;
  message: string;
}
