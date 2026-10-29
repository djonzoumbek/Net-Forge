import { describe, expect, it } from "vitest";
import { defaultSerial } from "../../types/profiles";
import { deviceName, serialSummary } from "./serial";

describe("serial helpers", () => {
  it("summarises the line settings", () => {
    expect(serialSummary(defaultSerial)).toBe("9600 8N1");
    expect(
      serialSummary({ baudRate: 115200, dataBits: 7, parity: "even", stopBits: 2, flowControl: "hardware" }),
    ).toBe("115200 7E2 RTS/CTS");
    expect(serialSummary({ ...defaultSerial, parity: "odd", flowControl: "software" })).toBe("9600 8O1 XON/XOFF");
  });

  it("names a tab after the device", () => {
    expect(deviceName("/dev/ttyUSB0")).toBe("ttyUSB0");
    expect(deviceName("/dev/serial/by-id/usb-FTDI-if00")).toBe("usb-FTDI-if00");
  });
});
