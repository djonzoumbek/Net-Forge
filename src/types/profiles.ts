// Mirrors src-tauri/src/persistence/profiles.rs.

export type AuthMethod = "password" | "keyFile" | "agent";

export interface Folder {
  id: number;
  name: string;
  parentId: number | null;
}

export interface Profile {
  id: number;
  folderId: number | null;
  name: string;
  host: string;
  port: number;
  username: string;
  authMethod: AuthMethod;
  keyPath: string | null;
  /** A password or key passphrase is saved in the system keyring. */
  hasSavedSecret: boolean;
  legacyMode: boolean;
}

export interface Library {
  folders: Folder[];
  profiles: Profile[];
}

export type SecretChange =
  | { action: "keep" }
  | { action: "set"; value: string }
  | { action: "clear" };

export interface ProfileInput {
  folderId: number | null;
  name: string;
  host: string;
  port: number;
  username: string;
  authMethod: AuthMethod;
  keyPath: string | null;
  legacyMode: boolean;
  secret: SecretChange;
}
