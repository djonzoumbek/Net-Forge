import { useCallback, useEffect, useState } from "react";
import { listButtons } from "../../services/profileService";
import { toAppError } from "../../services/sessionService";
import type { CommandButton } from "../../types/profiles";

/** Command bar buttons (shared by every session); `reload` after each change. */
export function useButtons() {
  const [buttons, setButtons] = useState<CommandButton[]>([]);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setButtons(await listButtons());
      setError(null);
    } catch (e) {
      setError(toAppError(e).message);
    }
  }, []);

  useEffect(() => {
    // Initial load; setState happens after the await, not synchronously.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void reload();
  }, [reload]);

  return { buttons, error, reload };
}
