// Typed wrappers around the port forwarding commands (SSH sessions only).
import { invoke } from "@tauri-apps/api/core";
import type { ForwardInfo, ForwardSpec } from "../types/forwards";
import type { SessionId } from "../types/session";

export const startForward = (sessionId: SessionId, spec: ForwardSpec) =>
  invoke<ForwardInfo>("forward_start", { sessionId, spec });

export const stopForward = (sessionId: SessionId, forwardId: number) =>
  invoke<void>("forward_stop", { sessionId, forwardId });

export const listForwards = (sessionId: SessionId) => invoke<ForwardInfo[]>("forward_list", { sessionId });
