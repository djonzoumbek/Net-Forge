import { useCallback, useEffect, useState } from "react";
import { listSerialPorts, toAppError } from "../../services/sessionService";
import type { FlowControl, Parity, SerialSettings } from "../../types/profiles";
import type { SerialPortEntry } from "../../types/session";
import { baudRates } from "./serial";

interface SerialFieldsProps {
  device: string;
  settings: SerialSettings;
  onDeviceChange: (device: string) => void;
  onSettingsChange: (settings: SerialSettings) => void;
}

/** Port picker (detected ports + free input) and line settings. */
export function SerialFields({ device, settings, onDeviceChange, onSettingsChange }: SerialFieldsProps) {
  const [ports, setPorts] = useState<SerialPortEntry[]>([]);
  const [portsError, setPortsError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setPorts(await listSerialPorts());
      setPortsError(null);
    } catch (e) {
      setPortsError(toAppError(e).message);
    }
  }, []);

  useEffect(() => {
    // Initial load; setState happens after the await, not synchronously.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void refresh();
  }, [refresh]);

  const set = <K extends keyof SerialSettings>(key: K, value: SerialSettings[K]) =>
    onSettingsChange({ ...settings, [key]: value });
  const selected = ports.find((p) => p.path === device);

  return (
    <>
      <div className="field-row">
        <label className="field grow">
          <span>Port série</span>
          <input
            value={device}
            onChange={(e) => onDeviceChange(e.target.value)}
            list="serial-ports"
            placeholder="/dev/ttyUSB0"
            spellCheck={false}
          />
          <datalist id="serial-ports">
            {ports.map((port) => (
              <option key={port.path} value={port.path}>
                {port.description}
              </option>
            ))}
          </datalist>
        </label>
        <button type="button" className="align-end" onClick={() => void refresh()} title="Détecter les ports">
          ⟳
        </button>
      </div>
      {portsError && <p className="hint danger-text">{portsError}</p>}
      {ports.length > 0 ? (
        <ul className="port-list">
          {ports.map((port) => (
            <li key={port.path}>
              <button
                type="button"
                className={`link ${port.path === device ? "current" : ""}`}
                onClick={() => onDeviceChange(port.path)}
              >
                {port.path}
              </button>{" "}
              <span className="hint">{port.description}</span>
              {!port.accessible && (
                <span className="badge danger small" title="Ton compte n'a pas les droits sur ce port (groupe dialout)">
                  accès refusé
                </span>
              )}
            </li>
          ))}
        </ul>
      ) : (
        !portsError && <p className="hint">Aucun port détecté. Branche l'adaptateur USB puis clique sur ⟳.</p>
      )}
      {selected && !selected.accessible && (
        <p className="hint warning-text">
          Ton compte n'a pas accès à {selected.path} : ajoute-le au groupe <code>dialout</code>{" "}
          (<code>sudo usermod -aG dialout $USER</code>) puis ferme et rouvre ta session.
        </p>
      )}

      <div className="field-row">
        <label className="field grow">
          <span>Vitesse (bauds)</span>
          <input
            value={String(settings.baudRate)}
            onChange={(e) => set("baudRate", Number(e.target.value.replace(/\D/g, "")) || 0)}
            list="baud-rates"
            inputMode="numeric"
          />
          <datalist id="baud-rates">
            {baudRates.map((rate) => (
              <option key={rate} value={rate} />
            ))}
          </datalist>
        </label>
        <label className="field">
          <span>Données</span>
          <select value={settings.dataBits} onChange={(e) => set("dataBits", Number(e.target.value))}>
            {[8, 7, 6, 5].map((bits) => (
              <option key={bits} value={bits}>
                {bits}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Parité</span>
          <select value={settings.parity} onChange={(e) => set("parity", e.target.value as Parity)}>
            <option value="none">Aucune</option>
            <option value="even">Paire</option>
            <option value="odd">Impaire</option>
          </select>
        </label>
        <label className="field">
          <span>Stop</span>
          <select value={settings.stopBits} onChange={(e) => set("stopBits", Number(e.target.value))}>
            <option value={1}>1</option>
            <option value={2}>2</option>
          </select>
        </label>
      </div>
      <label className="field">
        <span>Contrôle de flux</span>
        <select value={settings.flowControl} onChange={(e) => set("flowControl", e.target.value as FlowControl)}>
          <option value="none">Aucun (consoles Cisco / Huawei)</option>
          <option value="software">Logiciel (XON/XOFF)</option>
          <option value="hardware">Matériel (RTS/CTS)</option>
        </select>
      </label>
    </>
  );
}
