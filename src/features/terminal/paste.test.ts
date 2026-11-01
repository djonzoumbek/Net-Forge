import { describe, expect, it } from "vitest";
import { isPlainCtrlV, needsPasteConfirmation, pastedLineCount } from "./paste";

describe("paste confirmation", () => {
  it("counts lines whatever the line endings", () => {
    expect(pastedLineCount("a\nb\nc")).toBe(3);
    expect(pastedLineCount("a\r\nb\r\n")).toBe(2);
    expect(pastedLineCount("a\rb")).toBe(2);
    expect(pastedLineCount("")).toBe(0);
  });

  it("asks only beyond one line", () => {
    expect(needsPasteConfirmation("show run")).toBe(false);
    expect(needsPasteConfirmation("show run\n")).toBe(false);
    expect(needsPasteConfirmation("conf t\nhostname R1")).toBe(true);
    expect(needsPasteConfirmation("interface Gi0/0\n shutdown\n")).toBe(true);
  });
});

describe("isPlainCtrlV", () => {
  const key = (init: Partial<KeyboardEvent>) =>
    ({ type: "keydown", key: "v", ctrlKey: true, shiftKey: false, altKey: false, metaKey: false, ...init }) as KeyboardEvent;

  it("recognises Ctrl+V only", () => {
    expect(isPlainCtrlV(key({}))).toBe(true);
    expect(isPlainCtrlV(key({ key: "V" }))).toBe(true);
    // Ctrl+Shift+V has its own handling; Ctrl+Alt+V is left to the shell.
    expect(isPlainCtrlV(key({ shiftKey: true }))).toBe(false);
    expect(isPlainCtrlV(key({ altKey: true }))).toBe(false);
    expect(isPlainCtrlV(key({ ctrlKey: false }))).toBe(false);
    expect(isPlainCtrlV(key({ type: "keyup" }))).toBe(false);
  });
});
