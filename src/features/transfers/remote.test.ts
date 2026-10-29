import { describe, expect, it } from "vitest";
import { baseName, formatBytes, joinRemote, parentRemote, percent } from "./remote";

describe("remote path helpers", () => {
  it("joins and goes up", () => {
    expect(joinRemote("/home/admin", "a.cfg")).toBe("/home/admin/a.cfg");
    expect(joinRemote("/", "etc")).toBe("/etc");
    expect(parentRemote("/home/admin")).toBe("/home");
    expect(parentRemote("/home/admin/")).toBe("/home");
    expect(parentRemote("/home")).toBe("/");
    expect(parentRemote("/")).toBe("/");
    expect(baseName("/home/kamis/Téléchargements/backup R1.cfg")).toBe("backup R1.cfg");
  });

  it("formats sizes and progress", () => {
    expect(formatBytes(0)).toBe("0 o");
    expect(formatBytes(1536)).toBe("1,5 Ko");
    expect(formatBytes(3 * 1024 * 1024)).toBe("3 Mo");
    expect(formatBytes(250 * 1024 * 1024)).toBe("250 Mo");
    expect(percent(50, 200)).toBe(25);
    expect(percent(0, 0)).toBe(100);
    expect(percent(10, null)).toBeNull();
    expect(percent(300, 200)).toBe(100);
  });
});
