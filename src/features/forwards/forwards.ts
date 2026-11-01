import type { ForwardSpec } from "../../types/forwards";

export const LOOPBACK = "127.0.0.1";
export const ALL_INTERFACES = "0.0.0.0";

export function emptyForward(kind: ForwardSpec["kind"] = "local"): ForwardSpec {
  return {
    kind,
    bindAddress: LOOPBACK,
    bindPort: kind === "dynamic" ? 1080 : 0,
    targetHost: "",
    targetPort: 0,
  };
}

/** Reachable from other machines of the local network. */
export function isExposed(spec: ForwardSpec): boolean {
  const address = spec.bindAddress.trim();
  return address !== "" && address !== "localhost" && !address.startsWith("127.") && address !== "::1";
}

function hostPort(host: string, port: number | string): string {
  // IPv6 literals need brackets in front of a port.
  return host.includes(":") ? `[${host}]:${port}` : `${host}:${port}`;
}

/** `ssh -L`-like summary: "127.0.0.1:8443 → 192.168.10.1:443", "SOCKS 127.0.0.1:1080". */
export function describeForward(spec: ForwardSpec, localPort?: number): string {
  const port = localPort ?? (spec.bindPort === 0 ? "auto" : spec.bindPort);
  const local = hostPort(spec.bindAddress || LOOPBACK, port);
  return spec.kind === "dynamic" ? `SOCKS ${local}` : `${local} → ${hostPort(spec.targetHost, spec.targetPort)}`;
}

/** What to type in a browser or a client to use the forward. */
export function usageHint(spec: ForwardSpec, localPort: number): string {
  if (spec.kind === "dynamic") {
    return `Proxy SOCKS5 ${hostPort(LOOPBACK, localPort)} (avec résolution DNS distante : « socks5h » ou « Proxy DNS via SOCKS »).`;
  }
  if (spec.targetPort === 443 || spec.targetPort === 8443) return `https://localhost:${localPort}`;
  if (spec.targetPort === 80 || spec.targetPort === 8080) return `http://localhost:${localPort}`;
  return `localhost:${localPort}`;
}

/** Checks done before calling the backend (which validates again). `null` when valid. */
export function forwardError(spec: ForwardSpec): string | null {
  const port = (n: number) => Number.isInteger(n) && n >= 0 && n <= 65535;
  if (!port(spec.bindPort)) return "Port local : entier entre 0 (automatique) et 65535.";
  if (spec.kind === "local") {
    if (spec.targetHost.trim() === "" || /\s/.test(spec.targetHost.trim())) return "Hôte de destination invalide.";
    if (!port(spec.targetPort) || spec.targetPort === 0) return "Port de destination : entier entre 1 et 65535.";
  }
  return null;
}
