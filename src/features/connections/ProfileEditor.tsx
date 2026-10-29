import { useState, type FormEvent } from "react";
import { Modal } from "../../components/dialogs/Modal";
import { toAppError } from "../../services/sessionService";
import type { AuthMethod, LogMode, Profile, ProfileInput, Protocol, SecretChange } from "../../types/profiles";
import { defaultPort } from "./ConnectionForm";
import { flattenFolders, type Tree } from "./tree";

interface ProfileEditorProps {
  /** Undefined: new profile. */
  profile?: Profile;
  defaultFolderId: number | null;
  tree: Tree;
  onSave: (input: ProfileInput) => Promise<void>;
  onCancel: () => void;
}

/** What to do with the saved secret of an existing profile. */
type SavedSecretAction = "keep" | "replace" | "clear";
type SshAuthMethod = Exclude<AuthMethod, "none">;

export function ProfileEditor({ profile, defaultFolderId, tree, onSave, onCancel }: ProfileEditorProps) {
  const [name, setName] = useState(profile?.name ?? "");
  const [folderId, setFolderId] = useState<number | null>(profile?.folderId ?? defaultFolderId);
  const [protocol, setProtocol] = useState<Protocol>(profile?.protocol ?? "ssh");
  const [host, setHost] = useState(profile?.host ?? "");
  const [port, setPort] = useState(String(profile?.port ?? 22));
  const [username, setUsername] = useState(profile?.username ?? "");
  const [authMethod, setAuthMethod] = useState<SshAuthMethod>(
    profile && profile.authMethod !== "none" ? profile.authMethod : "password",
  );
  const [keyPath, setKeyPath] = useState(profile?.keyPath ?? "~/.ssh/id_ed25519");
  const [legacyMode, setLegacyMode] = useState(profile?.legacyMode ?? false);
  const [keepaliveSecs, setKeepaliveSecs] = useState(String(profile?.keepaliveSecs ?? 30));
  const [autoReconnect, setAutoReconnect] = useState(profile?.autoReconnect ?? false);
  const [pagerAutoReply, setPagerAutoReply] = useState(profile?.pagerAutoReply ?? false);
  const [loginCommands, setLoginCommands] = useState(profile?.loginCommands ?? "");
  const [sessionLog, setSessionLog] = useState<LogMode>(profile?.sessionLog ?? "off");
  const [saveSecret, setSaveSecret] = useState(false);
  const [savedAction, setSavedAction] = useState<SavedSecretAction>("keep");
  const [secret, setSecret] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const hasSaved = profile?.hasSavedSecret ?? false;
  const ssh = protocol === "ssh";

  function changeProtocol(next: Protocol) {
    // Keep a custom port; swap only the default one.
    if (port === defaultPort[protocol]) setPort(defaultPort[next]);
    setProtocol(next);
  }
  const secretLabel = authMethod === "password" ? "mot de passe" : "passphrase";

  function secretChange(): SecretChange {
    if (!ssh || authMethod === "agent") return { action: "clear" };
    if (hasSaved) {
      if (savedAction === "clear") return { action: "clear" };
      if (savedAction === "replace" && secret) return { action: "set", value: secret };
      return { action: "keep" };
    }
    return saveSecret && secret ? { action: "set", value: secret } : { action: "keep" };
  }

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    const portNumber = Number(port);
    if (!Number.isInteger(portNumber) || portNumber < 1 || portNumber > 65535) {
      setError("Le port doit être un entier entre 1 et 65535.");
      return;
    }
    const keepalive = Number(keepaliveSecs);
    if (!Number.isInteger(keepalive) || keepalive < 0 || keepalive > 3600) {
      setError("Keepalive : nombre de secondes entre 0 (désactivé) et 3600.");
      return;
    }
    setSaving(true);
    try {
      await onSave({
        folderId,
        name,
        protocol,
        host,
        port: portNumber,
        username: ssh ? username : "",
        authMethod: ssh ? authMethod : "none",
        keyPath: ssh && authMethod === "keyFile" ? keyPath : null,
        legacyMode: ssh && legacyMode,
        secret: secretChange(),
        keepaliveSecs: keepalive,
        autoReconnect,
        pagerAutoReply,
        loginCommands,
        sessionLog,
      });
    } catch (e) {
      setError(toAppError(e).message);
      setSaving(false);
    }
  }

  const showSecretInput =
    ssh && authMethod !== "agent" && (hasSaved ? savedAction === "replace" : saveSecret);

  return (
    <Modal title={profile ? `Modifier « ${profile.name} »` : "Nouveau profil"} onCancel={onCancel} wide>
      <form className="dialog-form" onSubmit={handleSubmit}>
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        <div className="field-row">
          <label className="field grow">
            <span>Nom</span>
            <input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="R1 — labo GNS3" />
          </label>
          <label className="field grow">
            <span>Dossier</span>
            <select
              value={folderId ?? ""}
              onChange={(e) => setFolderId(e.target.value === "" ? null : Number(e.target.value))}
            >
              <option value="">(racine)</option>
              {flattenFolders(tree).map(({ folder, depth }) => (
                <option key={folder.id} value={folder.id}>
                  {"  ".repeat(depth) + folder.name}
                </option>
              ))}
            </select>
          </label>
        </div>

        <fieldset className="auth-choice">
          <legend>Protocole</legend>
          <label>
            <input type="radio" checked={ssh} onChange={() => changeProtocol("ssh")} />
            SSH
          </label>
          <label>
            <input type="radio" checked={!ssh} onChange={() => changeProtocol("telnet")} />
            Telnet
          </label>
        </fieldset>
        {!ssh && (
          <p className="hint warning-text">
            Telnet n'est pas chiffré. Aucun mot de passe n'est enregistré pour un profil Telnet :
            l'équipement le demande dans le terminal.
          </p>
        )}

        <div className="field-row">
          <label className="field grow">
            <span>Hôte</span>
            <input value={host} onChange={(e) => setHost(e.target.value)} spellCheck={false} />
          </label>
          <label className="field port">
            <span>Port</span>
            <input value={port} onChange={(e) => setPort(e.target.value)} inputMode="numeric" />
          </label>
        </div>

        {ssh && (
          <>
            <label className="field">
              <span>Utilisateur</span>
              <input value={username} onChange={(e) => setUsername(e.target.value)} spellCheck={false} />
            </label>

            <fieldset className="auth-choice">
              <legend>Authentification</legend>
              {(
                [
                  ["password", "Mot de passe"],
                  ["keyFile", "Clé privée"],
                  ["agent", "Agent SSH"],
                ] as const
              ).map(([value, label]) => (
                <label key={value}>
                  <input type="radio" checked={authMethod === value} onChange={() => setAuthMethod(value)} />
                  {label}
                </label>
              ))}
            </fieldset>

            {authMethod === "keyFile" && (
              <label className="field">
                <span>Fichier de clé privée</span>
                <input value={keyPath} onChange={(e) => setKeyPath(e.target.value)} spellCheck={false} />
              </label>
            )}

            {authMethod !== "agent" &&
              (hasSaved ? (
                <fieldset className="auth-choice">
                  <legend>Un {secretLabel} est enregistré dans le trousseau</legend>
                  <label>
                    <input type="radio" checked={savedAction === "keep"} onChange={() => setSavedAction("keep")} />
                    Le garder
                  </label>
                  <label>
                    <input
                      type="radio"
                      checked={savedAction === "replace"}
                      onChange={() => setSavedAction("replace")}
                    />
                    Le remplacer
                  </label>
                  <label>
                    <input type="radio" checked={savedAction === "clear"} onChange={() => setSavedAction("clear")} />
                    Le supprimer
                  </label>
                </fieldset>
              ) : (
                <label className="checkbox">
                  <input type="checkbox" checked={saveSecret} onChange={(e) => setSaveSecret(e.target.checked)} />
                  Enregistrer le {secretLabel} dans le trousseau système
                </label>
              ))}

            {showSecretInput && (
              <label className="field">
                <span>{authMethod === "password" ? "Mot de passe" : "Passphrase"}</span>
                <input
                  type="password"
                  value={secret}
                  onChange={(e) => setSecret(e.target.value)}
                  autoComplete="off"
                />
              </label>
            )}

            <label className="checkbox">
              <input type="checkbox" checked={legacyMode} onChange={(e) => setLegacyMode(e.target.checked)} />
              Mode compatibilité équipements anciens
            </label>
            {legacyMode && (
              <p className="hint warning-text">
                Autorise des algorithmes obsolètes (diffie-hellman-group1/14-sha1, ssh-rsa, aes-cbc,
                3des-cbc, hmac-sha1) pour ce profil uniquement. Les algorithmes modernes restent
                prioritaires. À réserver aux vieux Cisco IOS / Huawei VRP.
              </p>
            )}
          </>
        )}

        <fieldset className="section">
          <legend>Session</legend>
          <div className="field-row">
            <label className="field port">
              <span>Keepalive (s)</span>
              <input
                value={keepaliveSecs}
                onChange={(e) => setKeepaliveSecs(e.target.value)}
                inputMode="numeric"
                title="0 = désactivé"
              />
            </label>
            <p className="hint grow">
              Garde la session ouverte à travers pare-feu et NAT, et détecte une connexion
              coupée. 0 pour désactiver.
            </p>
          </div>
          <label className="checkbox">
            <input type="checkbox" checked={autoReconnect} onChange={(e) => setAutoReconnect(e.target.checked)} />
            Reconnexion automatique si la connexion est perdue (jamais après <code>exit</code>)
          </label>
          <label className="checkbox">
            <input type="checkbox" checked={pagerAutoReply} onChange={(e) => setPagerAutoReply(e.target.checked)} />
            Répondre automatiquement au pager (<code>--More--</code>, <code>---- More ----</code>)
          </label>
          <label className="field">
            <span>Commandes envoyées à la première invite (une par ligne)</span>
            <textarea
              rows={3}
              value={loginCommands}
              onChange={(e) => setLoginCommands(e.target.value)}
              spellCheck={false}
              placeholder="terminal length 0"
            />
          </label>
          <div className="presets">
            <span className="hint">Désactiver la pagination :</span>
            <button type="button" onClick={() => setLoginCommands("terminal length 0")}>
              Cisco
            </button>
            <button type="button" onClick={() => setLoginCommands("screen-length 0 temporary")}>
              Huawei
            </button>
          </div>
          <p className="hint warning-text">
            Ces commandes sont enregistrées en clair : n'y mets jamais de mot de passe.
          </p>
        </fieldset>

        <fieldset className="section">
          <legend>Journal de session</legend>
          <label className="field">
            <span>Enregistrer la sortie du terminal dans un fichier horodaté</span>
            <select value={sessionLog} onChange={(e) => setSessionLog(e.target.value as LogMode)}>
              <option value="off">Désactivé</option>
              <option value="text">Texte brut (séquences d'échappement retirées)</option>
              <option value="raw">Octets bruts (rejouable avec cat)</option>
            </select>
          </label>
          {sessionLog !== "off" && (
            <p className="hint warning-text">
              Le journal contient tout ce que l'équipement affiche, donc parfois des secrets
              (configurations avec mots de passe ou clés, saisies renvoyées en écho). Fichier
              lisible par toi seul (droits 600), dans le dossier choisi dans les réglages.
            </p>
          )}
        </fieldset>

        <div className="dialog-actions">
          <button type="button" onClick={onCancel}>
            Annuler
          </button>
          <button type="submit" className="primary" disabled={saving}>
            Enregistrer
          </button>
        </div>
      </form>
    </Modal>
  );
}
