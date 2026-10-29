import { useCallback, useEffect, useState } from "react";
import { listLibrary } from "../../services/profileService";
import { toAppError } from "../../services/sessionService";
import type { Library } from "../../types/profiles";

/** Loads profiles and folders; `reload` after every change. */
export function useLibrary() {
  const [library, setLibrary] = useState<Library>({ folders: [], profiles: [] });
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setLibrary(await listLibrary());
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

  return { library, error, reload };
}
