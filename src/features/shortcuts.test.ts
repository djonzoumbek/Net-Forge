import { describe, expect, it } from "vitest";
import { matchShortcut } from "./shortcuts";

function key(init: KeyboardEventInit & { key: string }): KeyboardEvent {
  // Node has no KeyboardEvent: the matcher only reads these fields.
  return { type: "keydown", ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, code: "", ...init } as KeyboardEvent;
}

describe("matchShortcut", () => {
  it("recognises the application shortcuts", () => {
    expect(matchShortcut(key({ key: "T", ctrlKey: true, shiftKey: true }))).toBe("newTab");
    expect(matchShortcut(key({ key: "F", ctrlKey: true, shiftKey: true }))).toBe("search");
    expect(matchShortcut(key({ key: "Tab", ctrlKey: true, shiftKey: true }))).toBe("previousTab");
    expect(matchShortcut(key({ key: "PageDown", ctrlKey: true }))).toBe("nextTab");
    expect(matchShortcut(key({ key: "+", ctrlKey: true, shiftKey: true }))).toBe("zoomIn");
    expect(matchShortcut(key({ key: "-", ctrlKey: true }))).toBe("zoomOut");
  });

  it("works with an AZERTY keyboard", () => {
    // Ctrl+Shift+W: the W key is where QWERTY has Z.
    expect(matchShortcut(key({ key: "W", code: "KeyZ", ctrlKey: true, shiftKey: true }))).toBe("closeTab");
    // Ctrl+0: the 0 key types "à" without Shift.
    expect(matchShortcut(key({ key: "à", code: "Digit0", ctrlKey: true }))).toBe("zoomReset");
  });

  it("leaves the shell's control keys alone", () => {
    for (const k of ["c", "d", "z", "r", "w", "a", "e", "l", "u"]) {
      expect(matchShortcut(key({ key: k, ctrlKey: true }))).toBeNull();
    }
    expect(matchShortcut(key({ key: "t", shiftKey: true }))).toBeNull();
    expect(matchShortcut(key({ key: "T", ctrlKey: true, shiftKey: true, altKey: true }))).toBeNull();
  });
});
