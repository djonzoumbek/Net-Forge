import { describe, expect, it } from "vitest";
import type { ForwardSpec } from "../../types/forwards";
import { describeForward, emptyForward, forwardError, isExposed, usageHint } from "./forwards";

const web: ForwardSpec = {
  kind: "local",
  bindAddress: "127.0.0.1",
  bindPort: 8443,
  targetHost: "192.168.10.1",
  targetPort: 443,
};

describe("port forwards", () => {
  it("describes local and SOCKS forwards like ssh -L / -D", () => {
    expect(describeForward(web)).toBe("127.0.0.1:8443 → 192.168.10.1:443");
    expect(describeForward({ ...web, bindPort: 0 }, 40123)).toBe("127.0.0.1:40123 → 192.168.10.1:443");
    expect(describeForward({ ...web, targetHost: "fe80::1" })).toBe("127.0.0.1:8443 → [fe80::1]:443");
    expect(describeForward(emptyForward("dynamic"))).toBe("SOCKS 127.0.0.1:1080");
  });

  it("flags forwards reachable from the network", () => {
    expect(isExposed(web)).toBe(false);
    expect(isExposed({ ...web, bindAddress: "localhost" })).toBe(false);
    expect(isExposed({ ...web, bindAddress: "0.0.0.0" })).toBe(true);
    expect(isExposed({ ...web, bindAddress: "192.168.1.20" })).toBe(true);
  });

  it("suggests how to use the forward", () => {
    expect(usageHint(web, 8443)).toBe("https://localhost:8443");
    expect(usageHint({ ...web, targetPort: 22 }, 2201)).toBe("localhost:2201");
    expect(usageHint(emptyForward("dynamic"), 1080)).toContain("SOCKS5 127.0.0.1:1080");
  });

  it("checks the fields before sending them", () => {
    expect(forwardError(web)).toBeNull();
    expect(forwardError({ ...web, targetHost: " " })).not.toBeNull();
    expect(forwardError({ ...web, targetPort: 0 })).not.toBeNull();
    expect(forwardError({ ...web, bindPort: 70000 })).not.toBeNull();
    expect(forwardError(emptyForward("dynamic"))).toBeNull();
  });
});
