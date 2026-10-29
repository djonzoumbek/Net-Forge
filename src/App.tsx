import { useCallback, useMemo, useState } from "react";
import { ConfirmDialog } from "./components/dialogs/ConfirmDialog";
import { HostKeyChangedDialog } from "./components/dialogs/HostKeyChangedDialog";
import { HostKeyDialog } from "./components/dialogs/HostKeyDialog";
import { SecretPrompt } from "./components/dialogs/SecretPrompt";
import { TextPromptDialog } from "./components/dialogs/TextPromptDialog";
import { StatusBar, type SessionStatus } from "./components/layout/StatusBar";
import { ConnectionForm, emptyDraft, type ConnectionDraft } from "./features/connections/ConnectionForm";
import { ProfileEditor } from "./features/connections/ProfileEditor";
import { ProfileTree } from "./features/connections/ProfileTree";
import { buildTree } from "./features/connections/tree";
import { useLibrary } from "./features/connections/useLibrary";
import { TerminalView } from "./features/terminal/TerminalView";
import * as profiles from "./services/profileService";
import {
  answerHostKey,
  closeSession,
  profileOpener,
  replaceHostKey,
  sshOpener,
  toAppError,
  type SessionOpener,
} from "./services/sessionService";
import type { Folder, Profile, ProfileInput } from "./types/profiles";
import type { HostKeyInfo, HostKeyMismatch, SessionEvent, SessionId, SshConnectRequest } from "./types/session";
import "./styles/app.css";

interface SessionTarget {
  label: string;
  open: SessionOpener;
  profile?: Profile;
  /** Secret typed at connection time, to save in the keyring once logged in. */
  secretToSave?: string;
}

type View =
  | { kind: "home"; error?: string }
  // `attempt` is the TerminalView key: incrementing it reconnects.
  | { kind: "session"; target: SessionTarget; attempt: number };

type Dialog =
  | { kind: "profile"; profile?: Profile; folderId: number | null }
  | { kind: "newFolder"; parentId: number | null }
  | { kind: "renameFolder"; folder: Folder }
  | { kind: "deleteProfile"; profile: Profile }
  | { kind: "deleteFolder"; folder: Folder }
  | { kind: "secret"; profile: Profile; secretKind: "password" | "passphrase" }
  | { kind: "replaceSession"; target: SessionTarget };

const endpoint = (p: { username: string; host: string; port: number }) => `${p.username}@${p.host}:${p.port}`;

function App() {
  const { library, error: libraryError, reload } = useLibrary();
  const tree = useMemo(() => buildTree(library), [library]);

  const [view, setView] = useState<View>({ kind: "home" });
  const [draft, setDraft] = useState<ConnectionDraft>(emptyDraft);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [status, setStatus] = useState<SessionStatus>({ kind: "connecting" });
  const [sessionId, setSessionId] = useState<SessionId | null>(null);
  const [hostKey, setHostKey] = useState<HostKeyInfo | null>(null);
  const [mismatch, setMismatch] = useState<HostKeyMismatch | null>(null);

  const sessionLive = view.kind === "session" && status.kind !== "closed";

  function startSession(target: SessionTarget) {
    setStatus({ kind: "connecting" });
    setSessionId(null);
    setHostKey(null);
    setMismatch(null);
    setView((current) => ({
      kind: "session",
      target,
      attempt: current.kind === "session" ? current.attempt + 1 : 0,
    }));
  }

  /** Phase 3 has a single session: ask before replacing a live one (tabs: phase 4). */
  function requestSession(target: SessionTarget) {
    if (sessionLive) setDialog({ kind: "replaceSession", target });
    else startSession(target);
  }

  function connectProfile(profile: Profile, secret?: string, save = false) {
    if (secret === undefined && profile.authMethod === "password" && !profile.hasSavedSecret) {
      setDialog({ kind: "secret", profile, secretKind: "password" });
      return;
    }
    requestSession({
      label: `${profile.name} — ${endpoint(profile)}`,
      open: profileOpener(profile.id, secret),
      profile,
      secretToSave: save ? secret : undefined,
    });
  }

  function quickConnect(request: SshConnectRequest, nextDraft: ConnectionDraft) {
    setDraft(nextDraft);
    requestSession({ label: endpoint(request), open: sshOpener(request) });
  }

  const target = view.kind === "session" ? view.target : null;

  const handleEvent = useCallback(
    (event: SessionEvent) => {
      switch (event.type) {
        case "hostKeyPrompt":
          setHostKey(event.info);
          setStatus({ kind: "hostKey" });
          break;
        case "hostKeyChanged":
          setMismatch(event.mismatch);
          setStatus({ kind: "closed", reason: "Connexion bloquée : clé d'hôte modifiée.", byRemote: true });
          break;
        case "connected":
          setStatus({ kind: "connected", info: event.info });
          if (target?.profile && target.secretToSave) {
            profiles
              .saveProfileSecret(target.profile.id, target.secretToSave)
              .then(reload)
              .catch((e) => console.warn("secret not saved", toAppError(e).message));
          }
          break;
        case "failed": {
          setHostKey(null);
          const profile = target?.profile;
          // A protected key or a missing saved password: ask and retry.
          if (profile && (event.code === "passphrase_required" || event.code === "password_required")) {
            setView({ kind: "home" });
            setDialog({
              kind: "secret",
              profile,
              secretKind: event.code === "passphrase_required" ? "passphrase" : "password",
            });
          } else {
            setView({ kind: "home", error: event.message });
          }
          break;
        }
        case "closed":
          setHostKey(null);
          setStatus({ kind: "closed", reason: event.reason, byRemote: event.byRemote });
          break;
      }
    },
    [target, reload],
  );

  function handleHostKeyAnswer(accept: boolean) {
    setHostKey(null);
    setStatus({ kind: "connecting" });
    if (sessionId !== null) void answerHostKey(sessionId, accept).catch(() => {});
  }

  async function handleReplaceKey() {
    if (!mismatch || !target) return;
    try {
      await replaceHostKey(mismatch.host, mismatch.port, mismatch.newFingerprint);
      startSession(target);
    } catch (e) {
      setMismatch(null);
      setView({ kind: "home", error: toAppError(e).message });
    }
  }

  async function saveProfile(input: ProfileInput, existing?: Profile) {
    if (existing) await profiles.updateProfile(existing.id, input);
    else await profiles.createProfile(input);
    setDialog(null);
    await reload();
  }

  /** Runs a library change, then reloads; errors are shown on the home view. */
  async function mutate(action: () => Promise<unknown>) {
    setDialog(null);
    try {
      await action();
    } catch (e) {
      setView({ kind: "home", error: toAppError(e).message });
    }
    await reload();
  }

  function renderDialog() {
    if (!dialog) return null;
    const close = () => setDialog(null);
    switch (dialog.kind) {
      case "profile":
        return (
          <ProfileEditor
            profile={dialog.profile}
            defaultFolderId={dialog.folderId}
            tree={tree}
            onSave={(input) => saveProfile(input, dialog.profile)}
            onCancel={close}
          />
        );
      case "newFolder":
        return (
          <TextPromptDialog
            title="Nouveau dossier"
            label="Nom"
            submitLabel="Créer"
            onSubmit={(name) => mutate(() => profiles.createFolder(name, dialog.parentId))}
            onCancel={close}
          />
        );
      case "renameFolder":
        return (
          <TextPromptDialog
            title="Renommer le dossier"
            label="Nom"
            initial={dialog.folder.name}
            submitLabel="Renommer"
            onSubmit={(name) => mutate(() => profiles.renameFolder(dialog.folder.id, name))}
            onCancel={close}
          />
        );
      case "deleteProfile":
        return (
          <ConfirmDialog
            title="Supprimer le profil"
            message={`Supprimer « ${dialog.profile.name} » ? Le mot de passe éventuellement enregistré sera aussi retiré du trousseau.`}
            confirmLabel="Supprimer"
            onConfirm={() => mutate(() => profiles.deleteProfile(dialog.profile.id))}
            onCancel={close}
          />
        );
      case "deleteFolder":
        return (
          <ConfirmDialog
            title="Supprimer le dossier"
            message={`Supprimer « ${dialog.folder.name} » et ses sous-dossiers ? Les profils qu'ils contiennent ne sont pas supprimés : ils reviennent à la racine.`}
            confirmLabel="Supprimer"
            onConfirm={() => mutate(() => profiles.deleteFolder(dialog.folder.id))}
            onCancel={close}
          />
        );
      case "secret":
        return (
          <SecretPrompt
            kind={dialog.secretKind}
            target={dialog.profile.name}
            canSave
            onSubmit={(secret, save) => {
              setDialog(null);
              connectProfile(dialog.profile, secret, save);
            }}
            onCancel={close}
          />
        );
      case "replaceSession":
        return (
          <ConfirmDialog
            title="Remplacer la session en cours ?"
            message="Une seule session à la fois pour l'instant (les onglets arrivent en phase 4). La session actuelle sera fermée."
            confirmLabel="Fermer et se connecter"
            onConfirm={() => {
              setDialog(null);
              startSession(dialog.target);
            }}
            onCancel={close}
          />
        );
    }
  }

  return (
    <main className={`app ${sidebarOpen ? "" : "sidebar-hidden"}`}>
      {sidebarOpen && (
        <ProfileTree
          tree={tree}
          error={libraryError}
          onQuickConnect={() => setView({ kind: "home" })}
          onConnect={(profile) => connectProfile(profile)}
          onNewProfile={(folderId) => setDialog({ kind: "profile", folderId })}
          onEditProfile={(profile) => setDialog({ kind: "profile", profile, folderId: profile.folderId })}
          onDeleteProfile={(profile) => setDialog({ kind: "deleteProfile", profile })}
          onNewFolder={(parentId) => setDialog({ kind: "newFolder", parentId })}
          onRenameFolder={(folder) => setDialog({ kind: "renameFolder", folder })}
          onDeleteFolder={(folder) => setDialog({ kind: "deleteFolder", folder })}
        />
      )}

      <section className="workspace">
        {view.kind === "home" ? (
          <div className="home">
            {!sidebarOpen && (
              <button className="icon-button floating" onClick={() => setSidebarOpen(true)} aria-label="Profils">
                ☰
              </button>
            )}
            <ConnectionForm initial={draft} error={view.error} onConnect={quickConnect} />
          </div>
        ) : (
          <>
            <StatusBar
              label={view.target.label}
              status={status}
              sidebarOpen={sidebarOpen}
              onToggleSidebar={() => setSidebarOpen((open) => !open)}
              onDisconnect={() => {
                if (sessionId !== null) void closeSession(sessionId).catch(() => {});
              }}
              onBack={() => setView({ kind: "home" })}
            />
            {status.kind === "closed" && (
              <div className={`banner ${status.byRemote ? "warning" : ""}`} role="status">
                {status.reason}
                {status.byRemote && !mismatch && (
                  <button className="link" onClick={() => startSession(view.target)}>
                    Se reconnecter
                  </button>
                )}
              </div>
            )}
            <TerminalView
              key={view.attempt}
              open={view.target.open}
              onEvent={handleEvent}
              onSessionId={setSessionId}
            />
          </>
        )}
      </section>

      {hostKey && <HostKeyDialog info={hostKey} onAnswer={handleHostKeyAnswer} />}
      {mismatch && (
        <HostKeyChangedDialog
          mismatch={mismatch}
          onReplace={handleReplaceKey}
          onCancel={() => setMismatch(null)}
        />
      )}
      {renderDialog()}
    </main>
  );
}

export default App;
