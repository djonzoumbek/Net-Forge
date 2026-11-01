// Mirrors src-tauri/src/forwarding/mod.rs.

/** local: `ssh -L` (fixed target); dynamic: `ssh -D` (SOCKS5 proxy). */
export type ForwardKind = "local" | "dynamic";

export interface ForwardSpec {
  kind: ForwardKind;
  /** "127.0.0.1" (this machine only) or "0.0.0.0" (the whole local network). */
  bindAddress: string;
  /** 0: a free port chosen by the system. */
  bindPort: number;
  /** local only ("" and 0 for dynamic). */
  targetHost: string;
  targetPort: number;
}

/** A forward running on a session. */
export interface ForwardInfo {
  id: number;
  spec: ForwardSpec;
  /** The port really listened on. */
  localPort: number;
  activeConnections: number;
  totalConnections: number;
  lastError: string | null;
}
