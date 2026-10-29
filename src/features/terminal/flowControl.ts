// Helpers keeping the IPC traffic between xterm.js and the Rust session sane.

/**
 * Sends user input in order. Two concurrent `invoke` calls may be handled out of
 * order by the backend, which would swap keystrokes: we keep a single request in
 * flight and merge everything typed meanwhile into the next one.
 */
// Stays below the backend limit (1 MiB per call) for very large pastes.
const MAX_CHUNK = 256 * 1024;

export function createInputQueue(send: (bytes: Uint8Array) => Promise<void>) {
  const pending: number[] = [];
  let inFlight = false;

  async function flush() {
    inFlight = true;
    while (pending.length > 0) {
      const chunk = Uint8Array.from(pending.splice(0, MAX_CHUNK));
      try {
        await send(chunk);
      } catch (error) {
        console.warn("input not delivered", error);
      }
    }
    inFlight = false;
  }

  return {
    push(bytes: Uint8Array) {
      for (const b of bytes) pending.push(b);
      if (!inFlight) void flush();
    },
  };
}

const ACK_THRESHOLD = 64 * 1024;
const ACK_DELAY_MS = 20;

/**
 * Tells the backend how much output xterm.js has actually rendered (the backend
 * stops reading when too much is unacknowledged). Acks are grouped to avoid one
 * IPC call per output batch.
 */
export function createAckBatcher(sendAck: (bytes: number) => Promise<void>) {
  let pending = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;

  function flush() {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
    if (pending === 0) return;
    const bytes = pending;
    pending = 0;
    sendAck(bytes).catch(() => {
      // Session already closed: nothing left to unblock.
    });
  }

  return {
    add(bytes: number) {
      pending += bytes;
      if (pending >= ACK_THRESHOLD) flush();
      else if (timer === null) timer = setTimeout(flush, ACK_DELAY_MS);
    },
    dispose() {
      if (timer !== null) clearTimeout(timer);
      timer = null;
    },
  };
}
