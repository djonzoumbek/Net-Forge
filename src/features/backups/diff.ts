// Line diff between two configurations (Myers' O((N+M)·D) algorithm: fast when
// two backups differ by a few lines, which is the usual case).

export type DiffOp = { kind: "same" | "added" | "removed"; text: string };

/** Lines that change by themselves on every backup (timestamps, counters). */
const VOLATILE: RegExp[] = [
  /^! Last configuration change at /, // Cisco IOS
  /^! NVRAM config last updated at /,
  /^! No configuration change since last restart/,
  /^ntp clock-period /,
  /^Current configuration : \d+ bytes/,
  /^Building configuration\.\.\./,
  /^!Time: /, // Cisco NX-OS
  /^## Last commit: /, // Juniper
];

export function isVolatile(line: string): boolean {
  return VOLATILE.some((pattern) => pattern.test(line));
}

export function splitLines(text: string, ignoreVolatile: boolean): string[] {
  const lines = text.replace(/\r\n?/g, "\n").replace(/\n$/, "").split("\n");
  return ignoreVolatile ? lines.filter((line) => !isVolatile(line)) : lines;
}

/** Edit script turning `a` into `b`. */
export function diffLines(a: string[], b: string[]): DiffOp[] {
  const n = a.length;
  const m = b.length;
  const max = n + m;
  const offset = max;
  // v[k + offset]: furthest x reached on diagonal k; one copy per step D to
  // walk the path back.
  let v: Int32Array = new Int32Array(2 * max + 2);
  const trace: Int32Array[] = [];
  let found = false;
  for (let d = 0; d <= max && !found; d++) {
    trace.push(v.slice());
    for (let k = -d; k <= d; k += 2) {
      let x =
        k === -d || (k !== d && v[k - 1 + offset] < v[k + 1 + offset]) ? v[k + 1 + offset] : v[k - 1 + offset] + 1;
      let y = x - k;
      while (x < n && y < m && a[x] === b[y]) {
        x++;
        y++;
      }
      v[k + offset] = x;
      if (x >= n && y >= m) {
        found = true;
        break;
      }
    }
    if (found) trace.push(v.slice());
  }

  // Backtrack from (n, m) to (0, 0).
  const ops: DiffOp[] = [];
  let x = n;
  let y = m;
  for (let d = trace.length - 2; d >= 0; d--) {
    v = trace[d];
    const k = x - y;
    const prevK = k === -d || (k !== d && v[k - 1 + offset] < v[k + 1 + offset]) ? k + 1 : k - 1;
    const prevX = d === 0 ? 0 : v[prevK + offset];
    const prevY = prevX - prevK;
    while (x > prevX && y > prevY) {
      ops.push({ kind: "same", text: a[--x] });
      y--;
    }
    if (d > 0) {
      if (x === prevX) ops.push({ kind: "added", text: b[--y] });
      else ops.push({ kind: "removed", text: a[--x] });
    }
  }
  while (x > 0 && y > 0) {
    ops.push({ kind: "same", text: a[--x] });
    y--;
  }
  return ops.reverse();
}

export interface Hunk {
  /** 1-based line numbers where the hunk starts in the old / new text. */
  oldStart: number;
  newStart: number;
  ops: DiffOp[];
}

/** Changes with `context` unchanged lines around them, like `diff -u`. */
export function hunks(ops: DiffOp[], context = 3): Hunk[] {
  const result: Hunk[] = [];
  let oldLine = 1;
  let newLine = 1;
  let current: Hunk | null = null;
  let sameRun = 0;
  ops.forEach((op, index) => {
    if (op.kind === "same") {
      if (current) {
        // Keep the context after a change, unless the next change is close.
        const nextChange = ops.slice(index, index + 2 * context + 1).findIndex((o) => o.kind !== "same");
        if (sameRun < context || (nextChange !== -1 && nextChange <= 2 * context - sameRun)) {
          current.ops.push(op);
          sameRun++;
        } else {
          result.push(current);
          current = null;
        }
      }
    } else {
      if (!current) {
        const before = [];
        for (let i = index - 1; i >= 0 && before.length < context && ops[i].kind === "same"; i--) before.unshift(ops[i]);
        current = { oldStart: oldLine - before.length, newStart: newLine - before.length, ops: before };
      }
      current.ops.push(op);
      sameRun = 0;
    }
    if (op.kind !== "added") oldLine++;
    if (op.kind !== "removed") newLine++;
  });
  if (current) result.push(current);
  return result;
}

export function diffStats(ops: DiffOp[]): { added: number; removed: number } {
  return {
    added: ops.filter((o) => o.kind === "added").length,
    removed: ops.filter((o) => o.kind === "removed").length,
  };
}
