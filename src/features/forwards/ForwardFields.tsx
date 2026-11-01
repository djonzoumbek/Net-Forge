import type { ForwardKind, ForwardSpec } from "../../types/forwards";
import { ALL_INTERFACES, LOOPBACK, emptyForward, isExposed } from "./forwards";

interface ForwardFieldsProps {
  spec: ForwardSpec;
  onChange: (spec: ForwardSpec) => void;
}

/** Number field kept as text while typing ("" -> 0). */
function toPort(value: string): number {
  return value.trim() === "" ? 0 : Number(value);
}

/** The fields of one forward: type, local port, target, network exposure. */
export function ForwardFields({ spec, onChange }: ForwardFieldsProps) {
  const set = (patch: Partial<ForwardSpec>) => onChange({ ...spec, ...patch });
  const changeKind = (kind: ForwardKind) =>
    onChange({ ...emptyForward(kind), bindAddress: spec.bindAddress, bindPort: spec.bindPort || emptyForward(kind).bindPort });

  return (
    <div className="forward-fields">
      <div className="field-row">
        <label className="field">
          <span>Type</span>
          <select value={spec.kind} onChange={(e) => changeKind(e.target.value as ForwardKind)}>
            <option value="local">Local (-L)</option>
            <option value="dynamic">SOCKS (-D)</option>
          </select>
        </label>
        <label className="field port">
          <span>Port local</span>
          <input
            value={spec.bindPort === 0 ? "" : String(spec.bindPort)}
            onChange={(e) => set({ bindPort: toPort(e.target.value) })}
            inputMode="numeric"
            placeholder="auto"
          />
        </label>
        {spec.kind === "local" && (
          <>
            <label className="field grow">
              <span>Destination (vue depuis le serveur)</span>
              <input
                value={spec.targetHost}
                onChange={(e) => set({ targetHost: e.target.value })}
                placeholder="192.168.10.1"
                spellCheck={false}
              />
            </label>
            <label className="field port">
              <span>Port</span>
              <input
                value={spec.targetPort === 0 ? "" : String(spec.targetPort)}
                onChange={(e) => set({ targetPort: toPort(e.target.value) })}
                inputMode="numeric"
                placeholder="443"
              />
            </label>
          </>
        )}
      </div>
      <label className="checkbox">
        <input
          type="checkbox"
          checked={isExposed(spec)}
          onChange={(e) => set({ bindAddress: e.target.checked ? ALL_INTERFACES : LOOPBACK })}
        />
        Accessible depuis le réseau local
      </label>
      {isExposed(spec) && (
        <p className="hint warning-text">
          N'importe quelle machine de ton réseau pourra utiliser ce tunnel (et donc joindre ce que le serveur SSH
          joint) sans s'authentifier. Laisse décoché sauf besoin précis.
        </p>
      )}
    </div>
  );
}
