import { useState, type FormEvent } from "react";
import { Modal } from "../../components/dialogs/Modal";
import { toAppError } from "../../services/sessionService";
import {
  defaultSerial,
  type AuthMethod,
  type LogMode,
  type Profile,
  type ProfileInput,
  type Protocol,
  type SecretChange,
  type SerialSettings,
} from "../../types/profiles";
import type { ForwardSpec } from "../../types/forwards";
import { ForwardFields } from "../forwards/ForwardFields";
import { emptyForward, forwardError } from "../forwards/forwards";
import { SerialFields } from "../serial/SerialFields";
import { defaultPort, protocolLabels } from "./ConnectionForm";
import { flattenFolders, type Tree } from "./tree";

interface ProfileEditorProps {
  /** Undefined: new profile. */
  profile?: Profile;
  defaultFolderId: number | null;
  tree: Tree;
  /** Every profile: the SSH ones can be chosen as jump host. */
  profiles: Profile[];
  onSave: (input: ProfileInput) => Promise<void>;
  onCancel: () => void;
}

/** What to do with the saved secret of an existing profile. */
type SavedSecretAction = "keep" | "replace" | "clear";
type SshAuthMethod = Exclude<AuthMethod, "none">;

export function ProfileEditor({ profile, defaultFolderId, tree, profiles, onSave, onCancel }: ProfileEditorProps) {
  const [name, setName] = useState(profile?.name ?? "");
  const [folderId, setFolderId] = useState<number | null>(profile?.folderId ?? defaultFolderId);
  const [protocol, setProtocol] = useState<Protocol>(profile?.protocol ?? "ssh");
  const [host, setHost] = useState(profile?.protocol === "serial" ? "" : (profile?.host ?? ""));
  // Serial profiles keep the device path in `host`: separate state, so that
  // switching protocol back and forth does not mix a hostname and a device.
  const [device, setDevice] = useState(profile?.protocol === "serial" ? profile.host : "");
  const [serial, setSerial] = useState<SerialSettings>(profile?.serial ?? defaultSerial);
  const [port, setPort] = useState(profile && profile.protocol !== "serial" ? String(profile.port) : "22");
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
  const [portForwards, setPortForwards] = useState<ForwardSpec[]>(profile?.portForwards ?? []);
  const [jumpProfileId, setJumpProfileId] = useState<number | null>(profile?.jumpProfileId ?? null);
  const [saveSecret, setSaveSecret] = useState(false);
  const [savedAction, setSavedAction] = useState<SavedSecretAction>("keep");
  const [secret, setSecret] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const hasSaved = profile?.hasSavedSecret ?? false;
  const ssh = protocol === "ssh";
  const isSerial = protocol === "serial";

  function changeProtocol(next: Protocol) {
    // Keep a custom port; swap only the default one.
    if (next !== "serial" && (port === defaultPort[protocol] || port === "")) setPort(defaultPort[next]);
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
    const portNumber = isSerial ? 0 : Number(port);
    if (!isSerial && (!Number.isInteger(portNumber) || portNumber < 1 || portNumber > 65535)) {
      setError("Le port doit être un entier entre 1 et 65535.");
      return;
    }
    const keepalive = isSerial ? 0 : Number(keepaliveSecs);
    if (!Number.isInteger(keepalive) || keepalive < 0 || keepalive > 3600) {
      setError("Keepalive : nombre de secondes entre 0 (désactivé) et 3600.");
      return;
    }
    const badForward = ssh ? portForwards.map(forwardError).findIndex((e) => e !== null) : -1;
    if (badForward >= 0) {
      setError(`Redirection ${badForward + 1} : ${forwardError(portForwards[badForward])}`);
      return;
    }
    setSaving(true);
    try {
      await onSave({
        folderId,
        name,
        protocol,
        host: isSerial ? device : host,
        port: portNumber,
        username: ssh ? username : "",
        authMethod: ssh ? authMethod : "none",
        keyPath: ssh && authMethod === "keyFile" ? keyPath : null,
        legacyMode: ssh && legacyMode,
        secret: secretChange(),
        keepaliveSecs: keepalive,
        autoReconnect: autoReconnect && !isSerial,
        pagerAutoReply,
        loginCommands,
        sessionLog,
        serial,
        portForwards: ssh ? portForwards : [],
        jumpProfileId: ssh ? jumpProfileId : null,
      });
    } catch (e) {
      setError(toAppError(e).message);
      setSaving(false);
    }
  }

  // Loops and chains too long are refused by the backend when saving.
  const jumpCandidates = profiles.filter((p) => p.protocol === "ssh" && p.id !== profile?.id);
  const jumpProfile = jumpCandidates.find((p) => p.id === jumpProfileId);
  const jumpNeedsPassword = jumpProfile?.authMethod === "password" && !jumpProfile.hasSavedSecret;

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
          {protocolLabels.map(([value, label]) => (
            <label key={value}>
              <input type="radio" checked={protocol === value} onChange={() => changeProtocol(value)} />
              {label}
            </label>
          ))}
        </fieldset>
        {protocol === "telnet" && (
          <p className="hint warning-text">
            Telnet n'est pas chiffré. Aucun mot de passe n'est enregistré pour un profil Telnet :
            l'équipement le demande dans le terminal.
          </p>
        )}

        {isSerial ? (
          <SerialFields device={device} settings={serial} onDeviceChange={setDevice} onSettingsChange={setSerial} />
        ) : (
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
        )}

        {ssh && jumpCandidates.length > 0 && (
          <>
            <label className="field">
              <span>Hôte de rebond (bastion)</span>
              <select
                value={jumpProfileId ?? ""}
                onChange={(e) => setJumpProfileId(e.target.value === "" ? null : Number(e.target.value))}
              >
                <option value="">Aucun : connexion directe</option>
                {jumpCandidates.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name} ({p.username}@{p.host}:{p.port})
                  </option>
                ))}
              </select>
            </label>
            {jumpProfile && (
              <p className={`hint ${jumpNeedsPassword ? "warning-text" : ""}`}>
                {jumpNeedsPassword
                  ? `« ${jumpProfile.name} » n'a pas de mot de passe enregistré : enregistre-le dans ce profil (ou utilise une clé / l'agent SSH), sinon la connexion échouera.`
                  : `Connexion à travers « ${jumpProfile.name} », comme ssh -J : l'hôte et le port ci-dessus sont ceux vus depuis le rebond (ex. une adresse privée du labo). Chaque clé d'hôte est vérifiée.`}
              </p>
            )}
          </>
        )}

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
          {!isSerial && (
            <>
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
            </>
          )}
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

        {ssh && (
          <fieldset className="section">
            <legend>Redirections de port</legend>
            {portForwards.map((spec, index) => (
              <div key={index} className="forward-row">
                <ForwardFields
                  spec={spec}
                  onChange={(next) => setPortForwards((list) => list.map((s, i) => (i === index ? next : s)))}
                />
                <button
                  type="button"
                  className="link"
                  onClick={() => setPortForwards((list) => list.filter((_, i) => i !== index))}
                >
                  Retirer
                </button>
              </div>
            ))}
            <button type="button" onClick={() => setPortForwards((list) => [...list, emptyForward()])}>
              Ajouter une redirection
            </button>
            <p className="hint">
              Démarrées à chaque connexion. Local (-L) : un port de cette machine mène à une destination vue depuis
              le serveur SSH (ex. l'interface web d'un équipement du labo). SOCKS (-D) : un proxy pour faire passer
              un navigateur par le serveur. D'autres redirections peuvent être ajoutées pendant la session (bouton
              « Tunnels »).
            </p>
          </fieldset>
        )}

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
