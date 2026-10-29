// Types mirrored from the Rust side (src-tauri/src/sessions, commands, errors).

export type SessionId = number;

export interface TermSize {
  cols: number;
  rows: number;
}

export type AuthRequest =
  | { method: "password"; password: string }
  | { method: "keyFile"; path: string; passphrase?: string }
  | { method: "agent" };

export interface SshConnectRequest {
  host: string;
  port: number;
  username: string;
  auth: AuthRequest;
  /** Compatibility mode: also offer legacy algorithms (old Cisco/Huawei). */
  legacy?: boolean;
}

export interface TelnetConnectRequest {
  host: string;
  port: number;
}

export interface HostKeyInfo {
  host: string;
  port: number;
  algorithm: string;
  /** OpenSSH format, e.g. "SHA256:..." */
  fingerprint: string;
}

/** Recorded key differs from the one presented by the server. */
export interface HostKeyMismatch {
  host: string;
  port: number;
  algorithm: string;
  oldFingerprint: string;
  newFingerprint: string;
  file: string;
  line: number;
}

export interface NegotiatedAlgorithms {
  kex: string;
  hostKey: string;
  cipher: string;
  mac: string;
  /** Legacy algorithms among the negotiated ones (empty: all modern). */
  legacy: string[];
}

export interface ConnectionInfo {
  protocol: string;
  encrypted: boolean;
  algorithms: NegotiatedAlgorithms | null;
}

export type SessionEvent =
  | { type: "hostKeyPrompt"; info: HostKeyInfo }
  | { type: "hostKeyChanged"; mismatch: HostKeyMismatch }
  | { type: "connected"; info: ConnectionInfo }
  | { type: "failed"; code: string; message: string }
  | { type: "closed"; reason: string; byRemote: boolean };

/** Error shape returned by every Tauri command (`AppError` in Rust). */
export interface AppError {
  code: string;
  message: string;
}
