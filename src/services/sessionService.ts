// Typed wrappers around the Tauri session commands.
import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  AppError,
  SerialConnectRequest,
  SerialPortEntry,
  SessionEvent,
  SessionId,
  SshConnectRequest,
  TelnetConnectRequest,
  TermSize,
} from "../types/session";

function toBytes(message: unknown): Uint8Array {
  // Raw channel payloads arrive as ArrayBuffer; the other shapes are fallbacks.
  if (message instanceof ArrayBuffer) return new Uint8Array(message);
  if (message instanceof Uint8Array) return message;
  if (Array.isArray(message)) return Uint8Array.from(message as number[]);
  return new Uint8Array();
}

export function toAppError(error: unknown): AppError {
  if (typeof error === "object" && error !== null && "message" in error) {
    const e = error as Partial<AppError>;
    return { code: e.code ?? "unknown", message: String(e.message) };
  }
  return { code: "unknown", message: String(error) };
}

/**
 * Opens a session. Common shape for every way of connecting, so the terminal does
 * not need to know whether it is a quick connection or a saved profile.
 */
export type SessionOpener = (
  size: TermSize,
  onData: (bytes: Uint8Array) => void,
  onEvent: (event: SessionEvent) => void,
) => Promise<SessionId>;

// One pair of channels per session: output can never reach another tab.
function channels(onData: (bytes: Uint8Array) => void, onEvent: (event: SessionEvent) => void) {
  return {
    onData: new Channel<unknown>((message) => onData(toBytes(message))),
    onEvent: new Channel<SessionEvent>(onEvent),
  };
}

export function sshOpener(request: SshConnectRequest): SessionOpener {
  return (size, onData, onEvent) =>
    invoke<SessionId>("session_open_ssh", { request, size, ...channels(onData, onEvent) });
}

export function telnetOpener(request: TelnetConnectRequest): SessionOpener {
  return (size, onData, onEvent) =>
    invoke<SessionId>("session_open_telnet", { request, size, ...channels(onData, onEvent) });
}

export function serialOpener(request: SerialConnectRequest): SessionOpener {
  return (size, onData, onEvent) =>
    invoke<SessionId>("session_open_serial", { request, size, ...channels(onData, onEvent) });
}

export const listSerialPorts = () => invoke<SerialPortEntry[]>("serial_ports");

/** `secret`: password/passphrase typed by the user when none is saved. */
export function profileOpener(profileId: number, secret?: string): SessionOpener {
  return (size, onData, onEvent) =>
    invoke<SessionId>("session_open_profile", {
      profileId,
      secret: secret ?? null,
      size,
      ...channels(onData, onEvent),
    });
}

/** Types each line of `text` (followed by Enter), `delayMs` apart. */
export function sendSequence(id: SessionId, text: string, delayMs: number): Promise<void> {
  return invoke("session_send_sequence", { id, text, delayMs });
}

export function replaceHostKey(host: string, port: number, fingerprint: string): Promise<void> {
  return invoke("known_hosts_replace", { host, port, fingerprint });
}

export function writeToSession(id: SessionId, data: Uint8Array): Promise<void> {
  return invoke("session_write", { id, data: Array.from(data) });
}

export function resizeSession(id: SessionId, size: TermSize): Promise<void> {
  return invoke("session_resize", { id, size });
}

export function ackSession(id: SessionId, bytes: number): Promise<void> {
  return invoke("session_ack", { id, bytes });
}

export function closeSession(id: SessionId): Promise<void> {
  return invoke("session_close", { id });
}

export function answerHostKey(id: SessionId, accept: boolean): Promise<void> {
  return invoke("session_answer_host_key", { id, accept });
}
