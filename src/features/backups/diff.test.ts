import { describe, expect, it } from "vitest";
import { diffLines, diffStats, hunks, isVolatile, splitLines } from "./diff";

/** Rebuilds both texts from the edit script: the diff must be lossless. */
function sides(ops: ReturnType<typeof diffLines>) {
  return {
    old: ops.filter((o) => o.kind !== "added").map((o) => o.text),
    new: ops.filter((o) => o.kind !== "removed").map((o) => o.text),
  };
}

describe("configuration diff", () => {
  it("finds the changed lines of a configuration", () => {
    const before = ["hostname R1", "interface Gi0/0", " ip address 10.0.0.1 255.255.255.0", " shutdown", "end"];
    const after = ["hostname R1", "interface Gi0/0", " ip address 10.0.0.2 255.255.255.0", "end"];
    const ops = diffLines(before, after);
    expect(sides(ops)).toEqual({ old: before, new: after });
    expect(diffStats(ops)).toEqual({ added: 1, removed: 2 });
    expect(ops.filter((o) => o.kind !== "same").map((o) => `${o.kind} ${o.text}`)).toEqual([
      "removed  ip address 10.0.0.1 255.255.255.0",
      "removed  shutdown",
      "added  ip address 10.0.0.2 255.255.255.0",
    ]);
  });

  it("handles empty sides and identical texts", () => {
    expect(diffStats(diffLines([], ["a", "b"]))).toEqual({ added: 2, removed: 0 });
    expect(diffStats(diffLines(["a"], []))).toEqual({ added: 0, removed: 1 });
    expect(diffLines(["a", "b"], ["a", "b"]).every((o) => o.kind === "same")).toBe(true);
    expect(hunks(diffLines(["a", "b"], ["a", "b"]))).toEqual([]);
  });

  it("stays lossless on random edits", () => {
    let seed = 42;
    const random = () => (seed = (seed * 1103515245 + 12345) % 2 ** 31) / 2 ** 31;
    for (let round = 0; round < 50; round++) {
      const a = Array.from({ length: Math.floor(random() * 40) }, () => String(Math.floor(random() * 6)));
      const b = Array.from({ length: Math.floor(random() * 40) }, () => String(Math.floor(random() * 6)));
      expect(sides(diffLines(a, b))).toEqual({ old: a, new: b });
    }
  });

  it("is fast on a big configuration with few changes", () => {
    const a = Array.from({ length: 20000 }, (_, i) => `line ${i}`);
    const b = [...a];
    b[5000] = "changed";
    b.splice(15000, 0, "inserted");
    const start = performance.now();
    const stats = diffStats(diffLines(a, b));
    expect(stats).toEqual({ added: 2, removed: 1 });
    expect(performance.now() - start).toBeLessThan(1000);
  });

  it("groups changes with three lines of context", () => {
    const a = Array.from({ length: 20 }, (_, i) => `l${i}`);
    const b = [...a];
    b[2] = "X";
    b[15] = "Y";
    const result = hunks(diffLines(a, b));
    expect(result).toHaveLength(2);
    expect(result[0].oldStart).toBe(1);
    expect(result[0].ops.map((o) => o.text)).toEqual(["l0", "l1", "l2", "X", "l3", "l4", "l5"]);
    expect(result[1].oldStart).toBe(13);
  });

  it("ignores the lines that change by themselves", () => {
    expect(isVolatile("! Last configuration change at 10:00:01 UTC Sun Nov 1 2026 by admin")).toBe(true);
    expect(isVolatile("ntp clock-period 17179869")).toBe(true);
    expect(isVolatile("ntp server 10.0.0.1")).toBe(false);
    const text = "Building configuration...\r\n\r\nCurrent configuration : 1234 bytes\r\nhostname R1\r\n";
    expect(splitLines(text, true)).toEqual(["", "hostname R1"]);
    expect(splitLines(text, false)).toHaveLength(4);
  });
});
