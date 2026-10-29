// Typed wrappers around the Tauri session commands.
import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  AppError,
  SessionEvent,
  SessionId,
  SshConnectRequest,
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

export async function openSsh(
  request: SshConnectRequest,
  size: TermSize,
  onData: (bytes: Uint8Array) => void,
  onEvent: (event: SessionEvent) => void,
): Promise<SessionId> {
  // One pair of channels per session: output can never reach another tab.
  const dataChannel = new Channel<unknown>((message) => onData(toBytes(message)));
  const eventChannel = new Channel<SessionEvent>(onEvent);
  return invoke<SessionId>("session_open_ssh", {
    request,
    size,
    onData: dataChannel,
    onEvent: eventChannel,
  });
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
