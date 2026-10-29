import type { SerialSettings } from "../../types/profiles";

/** Usual speeds; any other value can be typed. */
export const baudRates = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];

/** Classic notation: "9600 8N1", plus the flow control when there is one. */
export function serialSummary(s: SerialSettings): string {
  const parity = { none: "N", odd: "O", even: "E" }[s.parity];
  const flow = { none: "", software: " XON/XOFF", hardware: " RTS/CTS" }[s.flowControl];
  return `${s.baudRate} ${s.dataBits}${parity}${s.stopBits}${flow}`;
}

/** `/dev/ttyUSB0` -> `ttyUSB0`, for the tab label. */
export function deviceName(path: string): string {
  return path.split("/").filter(Boolean).pop() ?? path;
}
