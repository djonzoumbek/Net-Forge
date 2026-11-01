// Mirrors src-tauri/src/persistence/profiles.rs.
import type { ForwardSpec } from "./forwards";

/** "serial": `host` is the device path (/dev/ttyUSB0), `port` is 0. */
export type Protocol = "ssh" | "telnet" | "serial";

export type Parity = "none" | "odd" | "even";
/** software: XON/XOFF, hardware: RTS/CTS. */
export type FlowControl = "none" | "software" | "hardware";

/** Serial line settings (mirrors transport/serial/mod.rs). */
export interface SerialSettings {
  baudRate: number;
  dataBits: number;
  parity: Parity;
  stopBits: number;
  flowControl: FlowControl;
}

/** Cisco / Huawei console: 9600 8N1, no flow control. */
export const defaultSerial: SerialSettings = {
  baudRate: 9600,
  dataBits: 8,
  parity: "none",
  stopBits: 1,
  flowControl: "none",
};

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
  serial: SerialSettings;
  /** SSH port forwards started at connection. */
  portForwards: ForwardSpec[];
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
  /** Session recording: plain text (cleaned) or raw bytes. */
  sessionLog: LogMode;
}

export type LogMode = "off" | "text" | "raw";

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
  serial: SerialSettings;
  portForwards: ForwardSpec[];
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
