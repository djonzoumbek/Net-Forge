import { useState, type FormEvent } from "react";
import type { Protocol } from "../../types/profiles";
import type { SshConnectRequest, TelnetConnectRequest } from "../../types/session";

type AuthMethod = "password" | "keyFile" | "agent";

export type QuickConnection =
  | { protocol: "ssh"; request: SshConnectRequest }
  | { protocol: "telnet"; request: TelnetConnectRequest };

export const defaultPort: Record<Protocol, string> = { ssh: "22", telnet: "23" };

export interface ConnectionDraft {
  protocol: Protocol;
  host: string;
  port: string;
  username: string;
  method: AuthMethod;
  keyPath: string;
  legacy: boolean;
}

export const emptyDraft: ConnectionDraft = {
  protocol: "ssh",
  host: "",
  port: "22",
  username: "",
  method: "password",
  keyPath: "~/.ssh/id_ed25519",
  legacy: false,
};

interface ConnectionFormProps {
  /** Non-secret fields to prefill (e.g. after a failed attempt). */
  initial: ConnectionDraft;
  error?: string;
  onConnect: (connection: QuickConnection, draft: ConnectionDraft) => void;
}

export function ConnectionForm({ initial, error, onConnect }: ConnectionFormProps) {
  const [draft, setDraft] = useState(initial);
  // Secrets live in their own state and are never copied into the draft.
  const [password, setPassword] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [validation, setValidation] = useState<string | null>(null);

  const update = (field: Exclude<keyof ConnectionDraft, "legacy" | "protocol">) => (value: string) =>
    setDraft((d) => ({ ...d, [field]: value }));

  // Switching protocol also switches the port, unless the user typed a custom one.
  const setProtocol = (protocol: Protocol) =>
    setDraft((d) => ({
      ...d,
      protocol,
      port: d.port === defaultPort[d.protocol] ? defaultPort[protocol] : d.port,
    }));
  const ssh = draft.protocol === "ssh";

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    const port = Number(draft.port);
    if (!draft.host.trim()) return setValidation("Indique l'hôte.");
    if (!Number.isInteger(port) || port < 1 || port > 65535)
      return setValidation("Le port doit être un entier entre 1 et 65535.");
    if (!ssh) {
      setValidation(null);
      onConnect({ protocol: "telnet", request: { host: draft.host.trim(), port } }, draft);
      return;
    }
    if (!draft.username.trim()) return setValidation("Indique le nom d'utilisateur.");
    if (draft.method === "keyFile" && !draft.keyPath.trim())
      return setValidation("Indique le chemin de la clé privée.");
    setValidation(null);

    onConnect(
      {
        protocol: "ssh",
        request: {
          host: draft.host.trim(),
          port,
          username: draft.username.trim(),
          auth:
            draft.method === "password"
              ? { method: "password", password }
              : draft.method === "keyFile"
                ? { method: "keyFile", path: draft.keyPath.trim(), passphrase }
                : { method: "agent" },
          legacy: draft.legacy,
        },
      },
      draft,
    );
    setPassword("");
    setPassphrase("");
  }

  const message = validation ?? error;

  return (
    <form className="connect-form" onSubmit={handleSubmit}>
      <h1>Connexion rapide</h1>
      <p className="hint">Sans enregistrer de profil. Pour une connexion réutilisable, crée un profil.</p>

      {message && (
        <p className="form-error" role="alert">
          {message}
        </p>
      )}

      <fieldset className="auth-choice">
        <legend>Protocole</legend>
        <label>
          <input type="radio" checked={ssh} onChange={() => setProtocol("ssh")} />
          SSH
        </label>
        <label>
          <input type="radio" checked={!ssh} onChange={() => setProtocol("telnet")} />
          Telnet
        </label>
      </fieldset>
      {!ssh && (
        <p className="hint warning-text">
          Telnet n'est pas chiffré : tout ce qui est tapé, mots de passe compris, circule en clair.
          À réserver aux consoles de labo (GNS3, EVE-NG) et aux réseaux de confiance.
        </p>
      )}

      <div className="field-row">
        <label className="field grow">
          <span>Hôte</span>
          <input
            autoFocus
            value={draft.host}
            onChange={(e) => update("host")(e.target.value)}
            placeholder="192.168.1.10 ou routeur.lab"
            spellCheck={false}
            autoCapitalize="off"
          />
        </label>
        <label className="field port">
          <span>Port</span>
          <input
            value={draft.port}
            onChange={(e) => update("port")(e.target.value)}
            inputMode="numeric"
          />
        </label>
      </div>

      {ssh && (
        <>
          <label className="field">
            <span>Utilisateur</span>
            <input
              value={draft.username}
              onChange={(e) => update("username")(e.target.value)}
              spellCheck={false}
              autoCapitalize="off"
            />
          </label>

          <fieldset className="auth-choice">
            <legend>Authentification</legend>
            <label>
              <input
                type="radio"
                checked={draft.method === "password"}
                onChange={() => update("method")("password")}
              />
              Mot de passe
            </label>
            <label>
              <input
                type="radio"
                checked={draft.method === "keyFile"}
                onChange={() => update("method")("keyFile")}
              />
              Clé privée
            </label>
            <label>
              <input
                type="radio"
                checked={draft.method === "agent"}
                onChange={() => update("method")("agent")}
              />
              Agent SSH
            </label>
          </fieldset>

          {draft.method === "password" ? (
            <label className="field">
              <span>Mot de passe</span>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                autoComplete="off"
              />
            </label>
          ) : draft.method === "keyFile" ? (
            <>
              <label className="field">
                <span>Fichier de clé privée</span>
                <input
                  value={draft.keyPath}
                  onChange={(e) => update("keyPath")(e.target.value)}
                  spellCheck={false}
                />
              </label>
              <label className="field">
                <span>Passphrase (si la clé est protégée)</span>
                <input
                  type="password"
                  value={passphrase}
                  onChange={(e) => setPassphrase(e.target.value)}
                  autoComplete="off"
                />
              </label>
        </>
      ) : null}

      <label className="checkbox">
        <input
          type="checkbox"
          checked={draft.legacy}
          onChange={(e) => setDraft((d) => ({ ...d, legacy: e.target.checked }))}
        />
        Mode compatibilité équipements anciens
      </label>
        </>
      )}

      <button type="submit" className="primary">
        Se connecter
      </button>
    </form>
  );
}
