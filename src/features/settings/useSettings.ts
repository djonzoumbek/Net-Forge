import { useCallback, useEffect, useState } from "react";
import { getSettings, updateSettings } from "../../services/profileService";
import { toAppError } from "../../services/sessionService";
import { defaultSettings, type AppSettings } from "../../types/settings";

/** Application settings. Defaults are used until the saved ones are loaded. */
export function useSettings() {
  const [settings, setSettings] = useState<AppSettings>(defaultSettings);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getSettings()
      .then((saved) => {
        setSettings(saved);
        setError(null);
      })
      .catch((e) => setError(`Réglages non chargés : ${toAppError(e).message}`));
  }, []);

  /** Validated by the backend: throws (with a readable message) when refused. */
  const save = useCallback(async (next: AppSettings) => {
    setSettings(await updateSettings(next));
    setError(null);
  }, []);

  return { settings, error, save };
}
