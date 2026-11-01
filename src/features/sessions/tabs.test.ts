import { describe, expect, it } from "vitest";
import type { SessionEvent } from "../../types/session";
import {
  initialTabs,
  MAX_AUTO_RECONNECT,
  reconnectDelay,
  tabsReducer,
  type SessionTarget,
  type TabsState,
} from "./tabs";

const target = (label: string): SessionTarget => ({
  label,
  title: label,
  protocol: "ssh",
  open: () => Promise.resolve(1),
});

function openTabs(...labels: string[]): TabsState {
  return labels.reduce((state, label) => tabsReducer(state, { type: "open", target: target(label) }), initialTabs);
}

const connected: SessionEvent = {
  type: "connected",
  info: { protocol: "ssh", encrypted: true, algorithms: null, via: [] },
  logFile: null,
  logError: null,
};

describe("tabsReducer", () => {
  it("moves a tab by drag and drop, keeping the active one", () => {
    const state = openTabs("a", "b", "c", "d"); // keys 1..4, "d" active
    const labels = (s: TabsState) => s.tabs.map((t) => t.target.label).join("");
    expect(labels(tabsReducer(state, { type: "move", key: 4, beforeKey: 1 }))).toBe("dabc");
    expect(labels(tabsReducer(state, { type: "move", key: 1, beforeKey: 3 }))).toBe("bacd");
    expect(labels(tabsReducer(state, { type: "move", key: 1, beforeKey: null }))).toBe("bcda");
    expect(tabsReducer(state, { type: "move", key: 2, beforeKey: 2 })).toBe(state);
    expect(tabsReducer(state, { type: "move", key: 99, beforeKey: 1 })).toBe(state);
    expect(tabsReducer(state, { type: "move", key: 4, beforeKey: 1 }).active).toBe(4);
  });

  it("opens tabs with unique keys and activates the new one", () => {
    const state = openTabs("a", "b");
    expect(state.tabs.map((t) => t.key)).toEqual([1, 2]);
    expect(state.active).toBe(2);
  });

  it("closing the active tab activates its neighbour", () => {
    let state = openTabs("a", "b", "c");
    state = tabsReducer(state, { type: "activate", key: 2 });
    state = tabsReducer(state, { type: "close", key: 2 });
    expect(state.active).toBe(3);
    state = tabsReducer(state, { type: "close", key: 3 });
    expect(state.active).toBe(1);
    state = tabsReducer(state, { type: "close", key: 1 });
    expect(state.active).toBeNull();
  });

  it("closing a background tab keeps the active one", () => {
    let state = openTabs("a", "b", "c");
    state = tabsReducer(state, { type: "close", key: 1 });
    expect(state.active).toBe(3);
    expect(state.tabs).toHaveLength(2);
  });

  it("ignores events of a previous attempt", () => {
    let state = openTabs("a");
    state = tabsReducer(state, { type: "retry", key: 1 });
    // Late "closed" from attempt 0 must not overwrite attempt 1.
    state = tabsReducer(state, {
      type: "event",
      key: 1,
      attempt: 0,
      event: { type: "closed", reason: "old", byRemote: true, lost: false },
    });
    expect(state.tabs[0].status.kind).toBe("connecting");
    state = tabsReducer(state, { type: "event", key: 1, attempt: 1, event: connected });
    expect(state.tabs[0].status.kind).toBe("connected");
  });

  it("routes events to their own tab only", () => {
    let state = openTabs("a", "b");
    state = tabsReducer(state, { type: "event", key: 1, attempt: 0, event: connected });
    expect(state.tabs[0].status.kind).toBe("connected");
    expect(state.tabs[1].status.kind).toBe("connecting");
  });

  it("cycles through tabs in both directions", () => {
    let state = openTabs("a", "b", "c");
    state = tabsReducer(state, { type: "cycle", step: 1 });
    expect(state.active).toBe(1);
    state = tabsReducer(state, { type: "cycle", step: -1 });
    expect(state.active).toBe(3);
  });

  it("dismissing a changed host key turns it into a failure", () => {
    let state = openTabs("a");
    state = tabsReducer(state, {
      type: "event",
      key: 1,
      attempt: 0,
      event: {
        type: "hostKeyChanged",
        mismatch: {
          host: "r1",
          port: 22,
          algorithm: "ssh-ed25519",
          oldFingerprint: "SHA256:old",
          newFingerprint: "SHA256:new",
          file: "known_hosts",
          line: 1,
        },
      },
    });
    state = tabsReducer(state, { type: "dismissMismatch", key: 1 });
    expect(state.tabs[0].status).toMatchObject({ kind: "failed", code: "host_key_changed" });
  });

  it("counts automatic reconnections and resets them once connected", () => {
    let state = openTabs("a");
    state = tabsReducer(state, { type: "scheduleReconnect", key: 1, at: 123 });
    expect(state.tabs[0].reconnectAt).toBe(123);
    state = tabsReducer(state, { type: "retry", key: 1, auto: true });
    state = tabsReducer(state, { type: "retry", key: 1, auto: true });
    expect(state.tabs[0]).toMatchObject({ autoRetries: 2, reconnectAt: null, attempt: 2 });
    state = tabsReducer(state, { type: "event", key: 1, attempt: 2, event: connected });
    expect(state.tabs[0].autoRetries).toBe(0);
  });

  it("a manual retry or a cancel stops the automatic cycle", () => {
    let state = openTabs("a");
    state = tabsReducer(state, { type: "retry", key: 1, auto: true });
    state = tabsReducer(state, { type: "scheduleReconnect", key: 1, at: 5 });
    state = tabsReducer(state, { type: "cancelReconnect", key: 1 });
    expect(state.tabs[0]).toMatchObject({ autoRetries: 0, reconnectAt: null });
    state = tabsReducer(state, { type: "retry", key: 1, auto: true });
    state = tabsReducer(state, { type: "retry", key: 1 });
    expect(state.tabs[0].autoRetries).toBe(0);
  });
});

describe("reconnectDelay", () => {
  it("grows then stops after the last attempt", () => {
    expect(reconnectDelay(1)).toBe(2);
    expect(reconnectDelay(MAX_AUTO_RECONNECT)).toBe(30);
    expect(reconnectDelay(MAX_AUTO_RECONNECT + 1)).toBeNull();
    expect(reconnectDelay(0)).toBeNull();
  });
});
