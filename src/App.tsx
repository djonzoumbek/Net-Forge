import { useEffect, useMemo, useReducer, useState } from "react";
import { ConfirmDialog } from "./components/dialogs/ConfirmDialog";
import { HostKeyChangedDialog } from "./components/dialogs/HostKeyChangedDialog";
import { HostKeyDialog } from "./components/dialogs/HostKeyDialog";
import { PasteConfirmDialog } from "./components/dialogs/PasteConfirmDialog";
import { SecretPrompt } from "./components/dialogs/SecretPrompt";
import { TextPromptDialog } from "./components/dialogs/TextPromptDialog";
import { TabBar } from "./components/layout/TabBar";
import { ResizeHandles } from "./components/layout/WindowFrame";
import { BroadcastDialog } from "./features/commands/BroadcastDialog";
import { ButtonEditor } from "./features/commands/ButtonEditor";
import { useButtons } from "./features/commands/useButtons";
import {
  ConnectionForm,
  emptyDraft,
  type ConnectionDraft,
  type QuickConnection,
} from "./features/connections/ConnectionForm";
import { ProfileEditor } from "./features/connections/ProfileEditor";
import { ProfileTree } from "./features/connections/ProfileTree";
import { buildTree } from "./features/connections/tree";
import { useLibrary } from "./features/connections/useLibrary";
import { SessionPanel } from "./features/sessions/SessionPanel";
import {
  initialTabs,
  reconnectDelay,
  tabsReducer,
  type SessionTarget,
  type Tab,
} from "./features/sessions/tabs";
import * as profiles from "./services/profileService";
import {
  answerHostKey,
  profileOpener,
  replaceHostKey,
  sendSequence,
  sshOpener,
  telnetOpener,
  toAppError,
} from "./services/sessionService";
import type { ButtonInput, CommandButton, Folder, Profile, ProfileInput } from "./types/profiles";
import type { SessionEvent } from "./types/session";
import "./styles/app.css";

type Dialog =
  | { kind: "profile"; profile?: Profile; folderId: number | null }
  | { kind: "newFolder"; parentId: number | null }
  | { kind: "renameFolder"; folder: Folder }
  | { kind: "deleteProfile"; profile: Profile }
  | { kind: "deleteFolder"; folder: Folder }
  // `tabKey`: retry in that tab instead of opening a new one.
  | { kind: "secret"; profile: Profile; secretKind: "password" | "passphrase"; tabKey?: number }
  | { kind: "button"; button?: CommandButton }
  | { kind: "broadcast"; fromKey: number }
  | { kind: "paste"; tab: Tab; text: string; paste: () => void };

/** Failures worth another automatic attempt (the device may come back). */
const NETWORK_FAILURES = new Set(["unreachable", "timeout", "host_not_found"]);

function describe(protocol: "ssh" | "telnet", host: string, port: number, username?: string): string {
  return protocol === "telnet" ? `telnet://${host}:${port}` : `${username}@${host}:${port}`;
}

function profileTarget(profile: Profile, secret?: string, save = false): SessionTarget {
  return {
    label: profile.name,
    title: `${profile.name} — ${describe(profile.protocol, profile.host, profile.port, profile.username)}`,
    protocol: profile.protocol,
    open: profileOpener(profile.id, secret),
    profile,
    secretToSave: save ? secret : undefined,
  };
}

function quickTarget(connection: QuickConnection): SessionTarget {
  const { host, port } = connection.request;
  if (connection.protocol === "telnet") {
    return {
      label: `${host}:${port}`,
      title: describe("telnet", host, port),
      protocol: "telnet",
      open: telnetOpener(connection.request),
    };
  }
  return {
    label: host,
    title: describe("ssh", host, port, connection.request.username),
    protocol: "ssh",
    open: sshOpener(connection.request),
  };
}

function App() {
  const { library, error: libraryError, reload } = useLibrary();
  const { buttons, error: buttonsError, reload: reloadButtons } = useButtons();
  const tree = useMemo(() => buildTree(library), [library]);
  const [state, dispatch] = useReducer(tabsReducer, initialTabs);
  const [draft, setDraft] = useState<ConnectionDraft>(emptyDraft);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(true);

  const activeTab = state.tabs.find((tab) => tab.key === state.active) ?? null;

  // Automatic reconnections: one timer per scheduled tab. Recreated when the tabs
  // change, with the remaining time.
  useEffect(() => {
    const timers = state.tabs
      .filter((tab) => tab.reconnectAt !== null)
      .map((tab) =>
        setTimeout(
          () => dispatch({ type: "retry", key: tab.key, auto: true }),
          Math.max(0, (tab.reconnectAt ?? 0) - Date.now()),
        ),
      );
    return () => timers.forEach(clearTimeout);
  }, [state.tabs]);

  // Ctrl+Tab / Ctrl+Shift+Tab / Ctrl+PageDown / Ctrl+PageUp: switch tabs.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!event.ctrlKey) return;
      const step =
        event.key === "Tab" ? (event.shiftKey ? -1 : 1)
        : event.key === "PageDown" ? 1
        : event.key === "PageUp" ? -1
        : 0;
      if (step !== 0) {
        event.preventDefault();
        dispatch({ type: "cycle", step });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  function connectProfile(profile: Profile) {
    const needsPassword =
      profile.protocol === "ssh" && profile.authMethod === "password" && !profile.hasSavedSecret;
    if (needsPassword) setDialog({ kind: "secret", profile, secretKind: "password" });
    else dispatch({ type: "open", target: profileTarget(profile) });
  }

  function quickConnect(connection: QuickConnection, nextDraft: ConnectionDraft) {
    setDraft(nextDraft);
    dispatch({ type: "open", target: quickTarget(connection) });
  }

  function scheduleReconnect(tab: Tab) {
    const delay = reconnectDelay(tab.autoRetries + 1);
    if (delay !== null) dispatch({ type: "scheduleReconnect", key: tab.key, at: Date.now() + delay * 1000 });
  }

  /** Side effects of session events (the reducer itself stays pure). */
  function handleEvent(tab: Tab, event: SessionEvent) {
    dispatch({ type: "event", key: tab.key, attempt: tab.attempt, event });
    // Auto-reconnect: only after a lost connection (never after `exit`), and while
    // the device stays unreachable during an ongoing reconnection cycle.
    if (tab.target.profile?.autoReconnect) {
      const lost = event.type === "closed" && event.lost;
      const stillDown = event.type === "failed" && tab.autoRetries > 0 && NETWORK_FAILURES.has(event.code);
      if (lost || stillDown) scheduleReconnect(tab);
    }
    const { profile, secretToSave } = tab.target;
    if (event.type === "connected" && profile && secretToSave) {
      profiles
        .saveProfileSecret(profile.id, secretToSave)
        .then(reload)
        .catch((e) => setActionError(`Mot de passe non enregistré : ${toAppError(e).message}`));
    }
    if (event.type === "failed" && profile) {
      if (event.code === "passphrase_required" || event.code === "password_required") {
        setDialog({
          kind: "secret",
          profile,
          secretKind: event.code === "passphrase_required" ? "passphrase" : "password",
          tabKey: tab.key,
        });
      }
    }
  }

  function retry(tab: Tab) {
    const { profile } = tab.target;
    // A profile is re-read from the library: it may have been edited since.
    const fresh = profile && library.profiles.find((p) => p.id === profile.id);
    if (fresh) {
      const needsPassword =
        fresh.protocol === "ssh" && fresh.authMethod === "password" && !fresh.hasSavedSecret;
      if (needsPassword && !tab.target.secretToSave) {
        setDialog({ kind: "secret", profile: fresh, secretKind: "password", tabKey: tab.key });
        return;
      }
    }
    dispatch({ type: "retry", key: tab.key, target: fresh ? profileTarget(fresh) : undefined });
  }

  function answerActiveHostKey(accept: boolean) {
    if (!activeTab || activeTab.sessionId === null) return;
    void answerHostKey(activeTab.sessionId, accept).catch(() => {});
    dispatch({ type: "hostKeyAnswered", key: activeTab.key });
  }

  async function replaceActiveHostKey() {
    if (activeTab?.status.kind !== "hostKeyChanged") return;
    const { mismatch } = activeTab.status;
    try {
      await replaceHostKey(mismatch.host, mismatch.port, mismatch.newFingerprint);
      dispatch({ type: "retry", key: activeTab.key });
    } catch (e) {
      dispatch({ type: "dismissMismatch", key: activeTab.key });
      setActionError(toAppError(e).message);
    }
  }

  async function saveProfile(input: ProfileInput, existing?: Profile) {
    if (existing) await profiles.updateProfile(existing.id, input);
    else await profiles.createProfile(input);
    setDialog(null);
    await reload();
  }

  function runButton(tab: Tab, button: CommandButton) {
    if (tab.sessionId === null) return;
    sendSequence(tab.sessionId, button.commands, button.delayMs).catch((e) =>
      setActionError(toAppError(e).message),
    );
  }

  function broadcast(keys: number[], text: string, delayMs: number) {
    setDialog(null);
    for (const tab of state.tabs.filter((t) => keys.includes(t.key))) {
      if (tab.sessionId === null || tab.status.kind !== "connected") continue;
      sendSequence(tab.sessionId, text, delayMs).catch((e) =>
        setActionError(`${tab.target.label} : ${toAppError(e).message}`),
      );
    }
  }

  async function saveButton(input: ButtonInput, existing?: CommandButton) {
    if (existing) await profiles.updateButton(existing.id, input);
    else await profiles.createButton(input);
    setDialog(null);
    await reloadButtons();
  }

  /** Runs a library change, then reloads; errors are shown in the sidebar. */
  async function mutate(action: () => Promise<unknown>) {
    setDialog(null);
    try {
      await action();
      setActionError(null);
    } catch (e) {
      setActionError(toAppError(e).message);
    }
    await reload();
  }

  function renderDialog() {
    if (!dialog) return null;
    const close = () => setDialog(null);
    switch (dialog.kind) {
      case "button":
        return (
          <ButtonEditor
            button={dialog.button}
            onSave={(input) => saveButton(input, dialog.button)}
            onDelete={async (button) => {
              await profiles.deleteButton(button.id);
              setDialog(null);
              await reloadButtons();
            }}
            onCancel={close}
          />
        );
      case "broadcast":
        return (
          <BroadcastDialog
            tabs={state.tabs.filter((t) => t.status.kind === "connected")}
            initiallySelected={dialog.fromKey}
            onSend={broadcast}
            onCancel={close}
          />
        );
      case "paste":
        return (
          <PasteConfirmDialog
            text={dialog.text}
            target={dialog.tab.target.title}
            onConfirm={() => {
              setDialog(null);
              dialog.paste();
            }}
            onCancel={close}
          />
        );
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
            message={`Supprimer « ${dialog.profile.name} » ? Le mot de passe éventuellement enregistré sera aussi retiré du trousseau. Les onglets ouverts restent ouverts.`}
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
              const target = profileTarget(dialog.profile, secret, save);
              if (dialog.tabKey !== undefined) {
                dispatch({ type: "retry", key: dialog.tabKey, target });
                dispatch({ type: "activate", key: dialog.tabKey });
              } else {
                dispatch({ type: "open", target });
              }
            }}
            onCancel={close}
          />
        );
    }
  }

  return (
    <main className={`app ${sidebarOpen ? "" : "sidebar-hidden"}`}>
      <ResizeHandles />
      {sidebarOpen && (
        <ProfileTree
          tree={tree}
          error={actionError ?? libraryError ?? buttonsError}
          onQuickConnect={() => dispatch({ type: "activate", key: null })}
          onConnect={connectProfile}
          onNewProfile={(folderId) => setDialog({ kind: "profile", folderId })}
          onEditProfile={(profile) => setDialog({ kind: "profile", profile, folderId: profile.folderId })}
          onDeleteProfile={(profile) => setDialog({ kind: "deleteProfile", profile })}
          onNewFolder={(parentId) => setDialog({ kind: "newFolder", parentId })}
          onRenameFolder={(folder) => setDialog({ kind: "renameFolder", folder })}
          onDeleteFolder={(folder) => setDialog({ kind: "deleteFolder", folder })}
        />
      )}

      <section className="workspace">
        <TabBar
          tabs={state.tabs}
          active={state.active}
          onActivate={(key) => dispatch({ type: "activate", key })}
          onClose={(key) => dispatch({ type: "close", key })}
        />
        <div className="panels">
          {state.active === null && (
            <div className="home">
              {!sidebarOpen && (
                <button className="icon-button floating" onClick={() => setSidebarOpen(true)} aria-label="Profils">
                  ☰
                </button>
              )}
              <ConnectionForm initial={draft} onConnect={quickConnect} />
            </div>
          )}
          {state.tabs.map((tab) => (
            <SessionPanel
              key={tab.key}
              tab={tab}
              active={tab.key === state.active}
              sidebarOpen={sidebarOpen}
              buttons={buttons}
              onToggleSidebar={() => setSidebarOpen((open) => !open)}
              onEvent={handleEvent}
              onCancelReconnect={(t) => dispatch({ type: "cancelReconnect", key: t.key })}
              onRunButton={runButton}
              onEditButton={(button) => setDialog({ kind: "button", button })}
              onBroadcast={(t) => setDialog({ kind: "broadcast", fromKey: t.key })}
              onPasteRequest={(t, text, paste) => setDialog({ kind: "paste", tab: t, text, paste })}
              onSessionId={(t, id) => dispatch({ type: "sessionId", key: t.key, attempt: t.attempt, id })}
              onRetry={retry}
              onClose={(t) => dispatch({ type: "close", key: t.key })}
              onEditProfile={(t) =>
                t.target.profile &&
                setDialog({ kind: "profile", profile: t.target.profile, folderId: t.target.profile.folderId })
              }
            />
          ))}
        </div>
      </section>

      {/* Host key questions are asked for the visible tab only; other tabs wait
          (their dot turns orange) so typing in one tab is never interrupted. */}
      {activeTab?.status.kind === "hostKey" && (
        <HostKeyDialog info={activeTab.status.info} onAnswer={answerActiveHostKey} />
      )}
      {activeTab?.status.kind === "hostKeyChanged" && (
        <HostKeyChangedDialog
          mismatch={activeTab.status.mismatch}
          onReplace={replaceActiveHostKey}
          onCancel={() => dispatch({ type: "dismissMismatch", key: activeTab.key })}
        />
      )}
      {renderDialog()}
    </main>
  );
}

export default App;
