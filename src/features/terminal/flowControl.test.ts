import { afterEach, describe, expect, it, vi } from "vitest";
import { createAckBatcher, createInputQueue } from "./flowControl";

const bytes = (s: string) => new TextEncoder().encode(s);
const text = (b: Uint8Array) => new TextDecoder().decode(b);

describe("createInputQueue", () => {
  it("keeps keystrokes in order and merges those typed while a send is in flight", async () => {
    const sent: string[] = [];
    let release: () => void = () => {};
    const send = vi.fn((chunk: Uint8Array) => {
      sent.push(text(chunk));
      return new Promise<void>((resolve) => (release = resolve));
    });
    const queue = createInputQueue(send);
    queue.push(bytes("l"));
    queue.push(bytes("s"));
    queue.push(bytes("\r"));
    expect(sent).toEqual(["l"]);
    release();
    await vi.waitFor(() => expect(sent).toEqual(["l", "s\r"]));
    release();
  });

  it("splits very large pastes into bounded chunks", async () => {
    const sizes: number[] = [];
    const queue = createInputQueue(async (chunk) => {
      sizes.push(chunk.length);
    });
    queue.push(new Uint8Array(600 * 1024));
    await vi.waitFor(() => expect(sizes.reduce((a, b) => a + b, 0)).toBe(600 * 1024));
    expect(Math.max(...sizes)).toBeLessThanOrEqual(256 * 1024);
  });
});

describe("createAckBatcher", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("groups small acks and flushes big ones immediately", () => {
    vi.useFakeTimers();
    const acks: number[] = [];
    const batcher = createAckBatcher(async (n) => {
      acks.push(n);
    });
    batcher.add(100);
    batcher.add(200);
    expect(acks).toEqual([]);
    vi.advanceTimersByTime(25);
    expect(acks).toEqual([300]);
    batcher.add(70 * 1024);
    expect(acks).toEqual([300, 70 * 1024]);
  });
});
