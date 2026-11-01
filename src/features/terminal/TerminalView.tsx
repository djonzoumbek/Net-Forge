import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
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
import type { ThemeName } from "../../types/settings";
import { matchShortcut } from "../shortcuts";
import { createAckBatcher, createInputQueue } from "./flowControl";
import { isPlainCtrlV, needsPasteConfirmation } from "./paste";
import { SearchBar, type SearchOptions } from "./SearchBar";
import { fontStack, themes } from "./theme";

/** What the settings (and the zoom) control; applied live to open terminals. */
export interface TerminalAppearance {
  theme: ThemeName;
  fontFamily: string;
  fontSize: number;
  scrollback: number;
  /** Clipboard behaviour: read at event time, so changes apply at once. */
  copyOnSelect: boolean;
  rightClickPaste: boolean;
  ctrlVPaste: boolean;
}

interface TerminalViewProps {
  /** Must keep the same identity for the life of the session (it is a dependency). */
  open: SessionOpener;
  onEvent: (event: SessionEvent) => void;
  onSessionId: (id: SessionId) => void;
  /** Visible tab: gets the keyboard focus. Hidden tabs keep running. */
  active: boolean;
  /** Multi-line paste: ask the user, then call `paste()` to really send it. */
  onPasteRequest: (text: string, paste: () => void) => void;
  appearance: TerminalAppearance;
  /** `null`: search bar closed; otherwise changes on each Ctrl+Shift+F. */
  searchToken: number | null;
  onSearchClose: () => void;
}

/**
 * Owns one xterm.js instance and the backend session behind it: the session is
 * opened on mount and closed on unmount.
 */
export function TerminalView(props: TerminalViewProps) {
  const { open, active, appearance, searchToken, onSearchClose } = props;
  const { theme, fontFamily, fontSize, scrollback } = appearance;
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const searchRef = useRef<SearchAddon | null>(null);
  // Callbacks change on every parent render; keeping them in refs avoids
  // reopening the session each time.
  const onEventRef = useRef(props.onEvent);
  const onSessionIdRef = useRef(props.onSessionId);
  const onPasteRequestRef = useRef(props.onPasteRequest);
  // Read once at creation; later changes go through the effect below.
  const appearanceRef = useRef(appearance);
  useEffect(() => {
    onEventRef.current = props.onEvent;
    onSessionIdRef.current = props.onSessionId;
    onPasteRequestRef.current = props.onPasteRequest;
    appearanceRef.current = appearance;
  });

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const initial = appearanceRef.current;
    const term = new Terminal({
      cursorBlink: true,
      fontFamily: fontStack(initial.fontFamily),
      fontSize: initial.fontSize,
      scrollback: initial.scrollback,
      theme: themes[initial.theme].colors,
      // Right click pastes (option) instead of selecting a word.
      rightClickSelectsWord: false,
    });
    const fit = new FitAddon();
    const search = new SearchAddon();
    term.loadAddon(fit);
    term.loadAddon(search);
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

    // Every paste goes through here: more than one line needs a confirmation.
    const pasteChecked = (text: string) => {
      if (needsPasteConfirmation(text)) onPasteRequestRef.current(text, () => term.paste(text));
      else term.paste(text);
    };
    // Browser pastes (context menu, middle click on Linux) reach xterm through a
    // `paste` event: intercept it first (capture phase).
    const onPaste = (event: ClipboardEvent) => {
      const text = event.clipboardData?.getData("text/plain") ?? "";
      if (!needsPasteConfirmation(text)) return;
      event.preventDefault();
      event.stopPropagation();
      pasteChecked(text);
    };
    container.addEventListener("paste", onPaste, true);

    const pasteClipboard = () => {
      void navigator.clipboard
        .readText()
        .then(pasteChecked)
        .catch(() => {});
    };
    const copySelection = () => {
      if (term.hasSelection()) void navigator.clipboard.writeText(term.getSelection()).catch(() => {});
    };

    // Copy on select: the selection changes on every mouse move while dragging,
    // so the clipboard is written once the selection has settled.
    let copyTimer: ReturnType<typeof setTimeout> | undefined;
    subscriptions.push(
      term.onSelectionChange(() => {
        if (!appearanceRef.current.copyOnSelect) return;
        clearTimeout(copyTimer);
        copyTimer = setTimeout(copySelection, 150);
      }),
    );

    // Right click pastes the clipboard (PuTTY / SecureCRT habit). Without the
    // option, the webview's own context menu stays.
    const onContextMenu = (event: MouseEvent) => {
      if (!appearanceRef.current.rightClickPaste) return;
      event.preventDefault();
      pasteClipboard();
    };
    container.addEventListener("contextmenu", onContextMenu);

    // Ctrl+Shift+C / Ctrl+Shift+V: copy/paste without stealing Ctrl+C from the shell.
    term.attachCustomKeyEventHandler((event) => {
      // Application shortcuts are not sent to the remote shell: the window
      // listener (App) acts on them.
      if (matchShortcut(event) !== null) return false;
      if (appearanceRef.current.ctrlVPaste && isPlainCtrlV(event)) {
        // Stops the webview's native paste too, which would paste a second time.
        event.preventDefault();
        pasteClipboard();
        return false;
      }
      if (event.type !== "keydown" || !event.ctrlKey || !event.shiftKey) return true;
      if (event.code === "KeyC" && term.hasSelection()) {
        copySelection();
        return false;
      }
      if (event.code === "KeyV") {
        pasteClipboard();
        return false;
      }
      return true;
    });

    // Refit when the container size changes (window resize, layout change), at
    // most once per animation frame.
    let frame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        // A hidden tab has a 0x0 container: fitting would shrink the remote
        // terminal to 1 row. It is refitted when shown again.
        if (container.clientWidth > 0 && container.clientHeight > 0) fit.fit();
      });
    });
    observer.observe(container);
    termRef.current = term;
    fitRef.current = fit;
    searchRef.current = search;

    return () => {
      disposed = true;
      container.removeEventListener("paste", onPaste, true);
      container.removeEventListener("contextmenu", onContextMenu);
      clearTimeout(copyTimer);
      observer.disconnect();
      cancelAnimationFrame(frame);
      subscriptions.forEach((s) => s.dispose());
      acks.dispose();
      if (sessionId !== null) void closeSession(sessionId).catch(() => {});
      termRef.current = null;
      fitRef.current = null;
      searchRef.current = null;
      term.dispose();
    };
  }, [open]);

  useEffect(() => {
    if (active) termRef.current?.focus();
  }, [active, open]);

  // Settings changed (or zoom): applied without reopening the session.
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.theme = themes[theme].colors;
    term.options.fontFamily = fontStack(fontFamily);
    term.options.fontSize = fontSize;
    term.options.scrollback = scrollback;
    // New character size: recompute rows/cols (sent to the remote by onResize).
    // Next frame: the renderer has measured the new font by then.
    const frame = requestAnimationFrame(() => {
      const container = containerRef.current;
      if (container && container.clientWidth > 0 && container.clientHeight > 0) fitRef.current?.fit();
    });
    return () => cancelAnimationFrame(frame);
  }, [theme, fontFamily, fontSize, scrollback]);

  function runSearch(text: string, options: SearchOptions, backwards: boolean): boolean {
    const search = searchRef.current;
    if (!search) return false;
    try {
      return backwards ? search.findPrevious(text, options) : search.findNext(text, options);
    } catch {
      // Invalid regular expression while it is being typed.
      return false;
    }
  }

  function closeSearch() {
    termRef.current?.clearSelection();
    onSearchClose();
    termRef.current?.focus();
  }

  return (
    <>
      {searchToken !== null && <SearchBar focusToken={searchToken} onSearch={runSearch} onClose={closeSearch} />}
      <div
        className="terminal-host"
        ref={containerRef}
        style={{ background: themes[theme].colors.background }}
      />
    </>
  );
}
