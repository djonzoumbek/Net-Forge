import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import {
  ackSession,
  closeSession,
  resizeSession,
  toAppError,
  writeToSession,
  type SessionOpener,
} from "../../services/sessionService";
import type { SessionEvent, SessionId } from "../../types/session";
import { createAckBatcher, createInputQueue } from "./flowControl";
import { terminalTheme } from "./theme";

interface TerminalViewProps {
  /** Must keep the same identity for the life of the session (it is a dependency). */
  open: SessionOpener;
  onEvent: (event: SessionEvent) => void;
  onSessionId: (id: SessionId) => void;
}

/**
 * Owns one xterm.js instance and the backend session behind it: the session is
 * opened on mount and closed on unmount.
 */
export function TerminalView({ open, onEvent, onSessionId }: TerminalViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  // Callbacks change on every parent render; keeping them in refs avoids
  // reopening the session each time.
  const onEventRef = useRef(onEvent);
  const onSessionIdRef = useRef(onSessionId);
  useEffect(() => {
    onEventRef.current = onEvent;
    onSessionIdRef.current = onSessionId;
  });

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const term = new Terminal({
      cursorBlink: true,
      fontFamily: '"DejaVu Sans Mono", "Ubuntu Mono", "Liberation Mono", monospace',
      fontSize: 14,
      scrollback: 5000,
      theme: terminalTheme,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(container);
    fit.fit();

    let sessionId: SessionId | null = null;
    let disposed = false;
    let ended = false;
    const encoder = new TextEncoder();
    const input = createInputQueue((bytes) =>
      sessionId === null ? Promise.resolve() : writeToSession(sessionId, bytes),
    );
    const acks = createAckBatcher((bytes) =>
      sessionId === null ? Promise.resolve() : ackSession(sessionId, bytes),
    );

    const initialSize = { cols: term.cols, rows: term.rows };
    open(
      initialSize,
      // xterm decodes UTF-8 itself, including characters split across chunks.
      (bytes) => term.write(bytes, () => acks.add(bytes.length)),
      (event) => {
        // The backend session is gone: stop sending input, resizes and closes to it.
        if (event.type === "closed" || event.type === "failed") {
          ended = true;
          sessionId = null;
        }
        onEventRef.current(event);
      },
    )
      .then((id) => {
        if (disposed) {
          // Unmounted while the command was in flight (React StrictMode does this
          // in development): close the orphan session right away.
          void closeSession(id).catch(() => {});
          return;
        }
        if (ended) return;
        sessionId = id;
        onSessionIdRef.current(id);
        if (term.cols !== initialSize.cols || term.rows !== initialSize.rows) {
          void resizeSession(id, { cols: term.cols, rows: term.rows }).catch(() => {});
        }
      })
      .catch((error) => {
        const { code, message } = toAppError(error);
        onEventRef.current({ type: "failed", code, message });
      });

    const subscriptions = [
      term.onData((text) => input.push(encoder.encode(text))),
      // Some mouse reporting modes produce binary strings (one char = one byte).
      term.onBinary((text) => input.push(Uint8Array.from(text, (c) => c.charCodeAt(0) & 0xff))),
      term.onResize(({ cols, rows }) => {
        if (sessionId !== null) void resizeSession(sessionId, { cols, rows }).catch(() => {});
      }),
    ];

    // Ctrl+Shift+C / Ctrl+Shift+V: copy/paste without stealing Ctrl+C from the shell.
    term.attachCustomKeyEventHandler((event) => {
      if (event.type !== "keydown" || !event.ctrlKey || !event.shiftKey) return true;
      if (event.code === "KeyC" && term.hasSelection()) {
        void navigator.clipboard.writeText(term.getSelection()).catch(() => {});
        return false;
      }
      if (event.code === "KeyV") {
        void navigator.clipboard
          .readText()
          .then((text) => term.paste(text))
          .catch(() => {});
        return false;
      }
      return true;
    });

    // Refit when the container size changes (window resize, layout change), at
    // most once per animation frame.
    let frame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => fit.fit());
    });
    observer.observe(container);
    term.focus();

    return () => {
      disposed = true;
      observer.disconnect();
      cancelAnimationFrame(frame);
      subscriptions.forEach((s) => s.dispose());
      acks.dispose();
      if (sessionId !== null) void closeSession(sessionId).catch(() => {});
      term.dispose();
    };
  }, [open]);

  return <div className="terminal-host" ref={containerRef} />;
}
