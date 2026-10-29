// Pure helpers of the SFTP panel (remote paths always use "/").

export function joinRemote(dir: string, name: string): string {
  return dir.endsWith("/") ? `${dir}${name}` : `${dir}/${name}`;
}

/** Parent directory; the root stays the root. */
export function parentRemote(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  const index = trimmed.lastIndexOf("/");
  return index <= 0 ? "/" : trimmed.slice(0, index);
}

/** Last component of a local or remote path. */
export function baseName(path: string): string {
  return path.split("/").filter(Boolean).pop() ?? path;
}

const UNITS = ["o", "Ko", "Mo", "Go", "To"];

/** 1536 -> "1,5 Ko" (French notation, binary multiples). */
export function formatBytes(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  const digits = unit === 0 || value >= 100 ? 0 : 1;
  return `${value.toLocaleString("fr-FR", { maximumFractionDigits: digits })} ${UNITS[unit]}`;
}

/** 0..100, or null when the total is unknown. */
export function percent(done: number, total: number | null): number | null {
  if (total === null) return null;
  if (total === 0) return 100;
  return Math.min(100, Math.floor((done / total) * 100));
}
