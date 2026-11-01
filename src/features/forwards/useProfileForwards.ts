import { useEffect, useRef, useState } from "react";
import { startForward } from "../../services/forwardService";
import { toAppError } from "../../services/sessionService";
import type { ForwardSpec } from "../../types/forwards";
import type { SessionId } from "../../types/session";
import { describeForward } from "./forwards";

/**
 * Starts the profile's forwards once per connected SSH session (a reconnection
 * is a new session: they start again). Returns the ones that failed.
 */
export function useProfileForwards(sessionId: SessionId | null, specs: ForwardSpec[]): string[] {
  const [result, setResult] = useState<{ sessionId: SessionId; errors: string[] } | null>(null);
  // React's StrictMode runs effects twice in development: the second start
  // would fail with "port already in use".
  const startedFor = useRef<SessionId | null>(null);

  useEffect(() => {
    if (sessionId === null || specs.length === 0 || startedFor.current === sessionId) return;
    startedFor.current = sessionId;
    void Promise.allSettled(specs.map((spec) => startForward(sessionId, spec))).then((outcomes) => {
      const errors = outcomes.flatMap((outcome, index) =>
        outcome.status === "rejected"
          ? [`${describeForward(specs[index])} : ${toAppError(outcome.reason).message}`]
          : [],
      );
      setResult({ sessionId, errors });
    });
  }, [sessionId, specs]);

  return result !== null && result.sessionId === sessionId ? result.errors : [];
}
