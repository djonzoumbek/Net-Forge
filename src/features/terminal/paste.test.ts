import { describe, expect, it } from "vitest";
import { needsPasteConfirmation, pastedLineCount } from "./paste";

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
