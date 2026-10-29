// Mirrors src-tauri/src/persistence/profiles.rs.

export type Protocol = "ssh" | "telnet";

/** "none": Telnet, the device asks for credentials in the terminal. */
export type AuthMethod = "none" | "password" | "keyFile" | "agent";

export interface Folder {
  id: number;
  name: string;
  parentId: number | null;
}

export interface Profile extends SessionOptions {
  id: number;
  folderId: number | null;
  name: string;
  protocol: Protocol;
  host: string;
  port: number;
  username: string;
  authMethod: AuthMethod;
  keyPath: string | null;
  /** A password or key passphrase is saved in the system keyring. */
  hasSavedSecret: boolean;
  legacyMode: boolean;
}

/** How a session of the profile behaves once connected. */
export interface SessionOptions {
  /** Keepalive interval in seconds, 0 = disabled. */
  keepaliveSecs: number;
  /** Reconnect when the connection is lost (never after `exit`). */
  autoReconnect: boolean;
  /** Answer --More-- like pager prompts with a space. */
  pagerAutoReply: boolean;
  /** One command per line, sent at the first prompt. Stored in clear. */
  loginCommands: string;
}

export interface Library {
  folders: Folder[];
  profiles: Profile[];
}

export type SecretChange =
  | { action: "keep" }
  | { action: "set"; value: string }
  | { action: "clear" };

export interface ProfileInput extends SessionOptions {
  folderId: number | null;
  name: string;
  protocol: Protocol;
  host: string;
  port: number;
  username: string;
  authMethod: AuthMethod;
  keyPath: string | null;
  legacyMode: boolean;
  secret: SecretChange;
}

export interface CommandButton {
  id: number;
  label: string;
  /** One command per line. */
  commands: string;
  /** Pause between two lines. */
  delayMs: number;
}

export type ButtonInput = Omit<CommandButton, "id">;
