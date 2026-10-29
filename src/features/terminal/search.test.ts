// Criterion of phase 6: "the search finds a text in a 10 000-line history".
// Runs the real xterm.js buffer and search addon, without a browser.
import { SearchAddon } from "@xterm/addon-search";
import { Terminal } from "@xterm/headless";
import type { Terminal as BrowserTerminal } from "@xterm/xterm";
import { describe, expect, it } from "vitest";

/** The headless terminal has no selection (a UI feature) but the addon selects
 * the match: minimal single-line selection reading the real buffer. */
function addSelectionShim(term: Terminal) {
  let selection: { column: number; row: number; length: number } | undefined;
  Object.assign(term, {
    select(column: number, row: number, length: number) {
      selection = { column, row, length };
    },
    clearSelection() {
      selection = undefined;
    },
    hasSelection: () => selection !== undefined,
    getSelectionPosition: () =>
      selection && {
        start: { x: selection.column, y: selection.row },
        end: { x: selection.column + selection.length, y: selection.row },
      },
    getSelection() {
      if (!selection) return "";
      const line = term.buffer.active.getLine(selection.row)?.translateToString(true) ?? "";
      return line.substring(selection.column, selection.column + selection.length);
    },
  });
}

async function terminalWithHistory(lines: number) {
  const term = new Terminal({ cols: 80, rows: 24, scrollback: 10000, allowProposedApi: true });
  addSelectionShim(term);
  const search = new SearchAddon();
  // The headless terminal has the same buffer API the addon uses.
  search.activate(term as unknown as BrowserTerminal);
  const text = Array.from({ length: lines }, (_, i) => `ligne ${i + 1} interface Gi0/${i % 48}`).join("\r\n");
  await new Promise<void>((resolve) => term.write(text, resolve));
  return { term, search };
}

describe("search in the scrollback", () => {
  it("finds the oldest line of a 10 000-line history", async () => {
    const { term, search } = await terminalWithHistory(10000);
    expect(term.buffer.active.length).toBeGreaterThanOrEqual(10000);
    expect(search.findNext("ligne 1 interface", { caseSensitive: true })).toBe(true);
    expect((term as unknown as BrowserTerminal).getSelection()).toBe("ligne 1 interface");
    // The match is far above the visible area: the view scrolled to it.
    expect(term.buffer.active.viewportY).toBeLessThan(10);
  });

  it("supports regular expressions and reports a missing text", async () => {
    const { term, search } = await terminalWithHistory(10000);
    expect(search.findNext("ligne 4321 interface Gi0/\\d+", { regex: true })).toBe(true);
    // Line 4321 holds index 4320, and 4320 % 48 = 0.
    expect((term as unknown as BrowserTerminal).getSelection()).toBe("ligne 4321 interface Gi0/0");
    expect(search.findNext("introuvable")).toBe(false);
  });
});
