// Tab state: one tab per session. Pure reducer, no side effects.
import type { SessionOpener } from "../../services/sessionService";
import type { Profile, Protocol } from "../../types/profiles";
import type {
  ConnectionInfo,
  HostKeyInfo,
  HostKeyMismatch,
  SessionEvent,
  SessionId,
} from "../../types/session";

export interface SessionTarget {
  /** Short name shown in the tab. */
  label: string;
  /** Full description (user@host:port), shown as tooltip and in the status bar. */
  title: string;
  protocol: Protocol;
  open: SessionOpener;
  profile?: Profile;
  /** Secret typed at connection time, saved in the keyring once logged in. */
  secretToSave?: string;
}

export type TabStatus =
  | { kind: "connecting" }
  | { kind: "hostKey"; info: HostKeyInfo }
  | { kind: "connected"; info: ConnectionInfo }
  | { kind: "hostKeyChanged"; mismatch: HostKeyMismatch }
  | { kind: "failed"; code: string; message: string }
  | { kind: "closed"; reason: string; byRemote: boolean };

export interface Tab {
  key: number;
  target: SessionTarget;
  /** Incremented on each reconnection: also the TerminalView React key. */
  attempt: number;
  sessionId: SessionId | null;
  status: TabStatus;
  /** Automatic reconnections used since the connection was lost (0 = none). */
  autoRetries: number;
  /** Next automatic reconnection, as a `Date.now()` timestamp. */
  reconnectAt: number | null;
}

/** Waits between automatic reconnections, in seconds; its length is the maximum. */
const RECONNECT_DELAYS = [2, 5, 10, 20, 30];

/** Delay before automatic reconnection number `attempt` (1-based), or null when
 * the attempts are exhausted. */
export function reconnectDelay(attempt: number): number | null {
  return attempt >= 1 && attempt <= RECONNECT_DELAYS.length ? RECONNECT_DELAYS[attempt - 1] : null;
}

export const MAX_AUTO_RECONNECT = RECONNECT_DELAYS.length;

export interface TabsState {
  tabs: Tab[];
  /** `null`: the "new connection" page is shown. */
  active: number | null;
  nextKey: number;
}

export type TabsAction =
  | { type: "open"; target: SessionTarget }
  | { type: "activate"; key: number | null }
  | { type: "cycle"; step: 1 | -1 }
  | { type: "close"; key: number }
  /** `auto`: automatic reconnection (keeps counting attempts). */
  | { type: "retry"; key: number; target?: SessionTarget; auto?: boolean }
  | { type: "scheduleReconnect"; key: number; at: number }
  | { type: "cancelReconnect"; key: number }
  | { type: "sessionId"; key: number; attempt: number; id: SessionId }
  | { type: "event"; key: number; attempt: number; event: SessionEvent }
  | { type: "hostKeyAnswered"; key: number }
  | { type: "dismissMismatch"; key: number };

export const initialTabs: TabsState = { tabs: [], active: null, nextKey: 1 };

function statusFromEvent(event: SessionEvent): TabStatus {
  switch (event.type) {
    case "hostKeyPrompt":
      return { kind: "hostKey", info: event.info };
    case "hostKeyChanged":
      return { kind: "hostKeyChanged", mismatch: event.mismatch };
    case "connected":
      return { kind: "connected", info: event.info };
    case "failed":
      return { kind: "failed", code: event.code, message: event.message };
    case "closed":
      return { kind: "closed", reason: event.reason, byRemote: event.byRemote };
  }
}

function updateTab(state: TabsState, key: number, update: (tab: Tab) => Tab): TabsState {
  return { ...state, tabs: state.tabs.map((tab) => (tab.key === key ? update(tab) : tab)) };
}

export function tabsReducer(state: TabsState, action: TabsAction): TabsState {
  switch (action.type) {
    case "open": {
      const tab: Tab = {
        key: state.nextKey,
        target: action.target,
        attempt: 0,
        sessionId: null,
        status: { kind: "connecting" },
        autoRetries: 0,
        reconnectAt: null,
      };
      return { tabs: [...state.tabs, tab], active: tab.key, nextKey: state.nextKey + 1 };
    }
    case "activate":
      return { ...state, active: action.key };
    case "cycle": {
      if (state.tabs.length === 0) return state;
      const index = state.tabs.findIndex((tab) => tab.key === state.active);
      const next = (index + action.step + state.tabs.length) % state.tabs.length;
      return { ...state, active: state.tabs[index === -1 ? 0 : next].key };
    }
    case "close": {
      const index = state.tabs.findIndex((tab) => tab.key === action.key);
      if (index === -1) return state;
      const tabs = state.tabs.filter((tab) => tab.key !== action.key);
      // Closing the active tab activates its neighbour, like a browser.
      const active =
        state.active !== action.key
          ? state.active
          : (tabs[Math.min(index, tabs.length - 1)]?.key ?? null);
      return { ...state, tabs, active };
    }
    case "retry":
      return updateTab(state, action.key, (tab) => ({
        ...tab,
        target: action.target ?? tab.target,
        attempt: tab.attempt + 1,
        sessionId: null,
        status: { kind: "connecting" },
        autoRetries: action.auto ? tab.autoRetries + 1 : 0,
        reconnectAt: null,
      }));
    case "scheduleReconnect":
      return updateTab(state, action.key, (tab) => ({ ...tab, reconnectAt: action.at }));
    case "cancelReconnect":
      return updateTab(state, action.key, (tab) => ({ ...tab, reconnectAt: null, autoRetries: 0 }));
    case "sessionId":
      return updateTab(state, action.key, (tab) =>
        tab.attempt === action.attempt ? { ...tab, sessionId: action.id } : tab,
      );
    case "event":
      // Events of a previous attempt (late messages) must not overwrite the current one.
      return updateTab(state, action.key, (tab) => {
        if (tab.attempt !== action.attempt) return tab;
        const status = statusFromEvent(action.event);
        // Connected again: the outage is over.
        const autoRetries = status.kind === "connected" ? 0 : tab.autoRetries;
        return { ...tab, status, autoRetries };
      });
    case "hostKeyAnswered":
      return updateTab(state, action.key, (tab) => ({ ...tab, status: { kind: "connecting" } }));
    case "dismissMismatch":
      return updateTab(state, action.key, (tab) =>
        tab.status.kind === "hostKeyChanged"
          ? {
              ...tab,
              status: {
                kind: "failed",
                code: "host_key_changed",
                message: "Connexion bloquée : la clé du serveur a changé.",
              },
            }
          : tab,
      );
  }
}
