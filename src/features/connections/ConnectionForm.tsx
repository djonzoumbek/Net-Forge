import { useState, type FormEvent } from "react";
import type { SshConnectRequest } from "../../types/session";

type AuthMethod = "password" | "keyFile";

export interface ConnectionDraft {
  host: string;
  port: string;
  username: string;
  method: AuthMethod;
  keyPath: string;
}

export const emptyDraft: ConnectionDraft = {
  host: "",
  port: "22",
  username: "",
  method: "password",
  keyPath: "~/.ssh/id_ed25519",
};

interface ConnectionFormProps {
  /** Non-secret fields to prefill (e.g. after a failed attempt). */
  initial: ConnectionDraft;
  error?: string;
  onConnect: (request: SshConnectRequest, draft: ConnectionDraft) => void;
}

export function ConnectionForm({ initial, error, onConnect }: ConnectionFormProps) {
  const [draft, setDraft] = useState(initial);
  // Secrets live in their own state and are never copied into the draft.
  const [password, setPassword] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [validation, setValidation] = useState<string | null>(null);

  const update = (field: keyof ConnectionDraft) => (value: string) =>
    setDraft((d) => ({ ...d, [field]: value }));

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    const port = Number(draft.port);
    if (!draft.host.trim()) return setValidation("Indique l'hôte.");
    if (!Number.isInteger(port) || port < 1 || port > 65535)
      return setValidation("Le port doit être un entier entre 1 et 65535.");
    if (!draft.username.trim()) return setValidation("Indique le nom d'utilisateur.");
    if (draft.method === "keyFile" && !draft.keyPath.trim())
      return setValidation("Indique le chemin de la clé privée.");
    setValidation(null);

    onConnect(
      {
        host: draft.host.trim(),
        port,
        username: draft.username.trim(),
        auth:
          draft.method === "password"
            ? { method: "password", password }
            : { method: "keyFile", path: draft.keyPath.trim(), passphrase },
      },
      draft,
    );
    setPassword("");
    setPassphrase("");
  }

  const message = validation ?? error;

  return (
    <form className="connect-form" onSubmit={handleSubmit}>
      <h1>Nouvelle connexion SSH</h1>

      {message && (
        <p className="form-error" role="alert">
          {message}
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
      ) : (
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
      )}

      <button type="submit" className="primary">
        Se connecter
      </button>
    </form>
  );
}
