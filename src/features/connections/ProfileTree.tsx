import { useState, type PointerEvent } from "react";
import { usePointerDrag, type DragState } from "../../hooks/usePointerDrag";
import type { Folder, Profile } from "../../types/profiles";
import { flattenFolders, isInsideFolder, type FolderNode, type Tree } from "./tree";

export interface ProfileTreeActions {
  onConnect: (profile: Profile) => void;
  onNewProfile: (folderId: number | null) => void;
  onEditProfile: (profile: Profile) => void;
  onDeleteProfile: (profile: Profile) => void;
  onNewFolder: (parentId: number | null) => void;
  onRenameFolder: (folder: Folder) => void;
  onDeleteFolder: (folder: Folder) => void;
  /** Drag and drop; `null`: the root. */
  onMoveProfile: (profile: Profile, folderId: number | null) => void;
  onMoveFolder: (folder: Folder, parentId: number | null) => void;
}

type DragItem = { kind: "profile"; profile: Profile } | { kind: "folder"; folder: Folder };
/** Destination folder; `null`: the root. */
type TreeDrop = { folderId: number | null };

/** Shared by every row of the tree. */
interface TreeDrag {
  drag: DragState<DragItem, TreeDrop> | null;
  startDrag: (event: PointerEvent, item: DragItem) => void;
}

interface ProfileTreeProps extends ProfileTreeActions {
  tree: Tree;
  error: string | null;
  onQuickConnect: () => void;
  onSettings: () => void;
}

export function ProfileTree({ tree, error, onQuickConnect, onSettings, ...actions }: ProfileTreeProps) {
  const [collapsed, setCollapsed] = useState<Set<number>>(new Set());
  const toggle = (id: number) =>
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const empty = tree.folders.length === 0 && tree.profiles.length === 0;
  const allFolders = flattenFolders(tree).map(({ folder }) => folder);

  const dnd: TreeDrag = usePointerDrag<DragItem, TreeDrop>({
    findDrop: (element, item) => {
      const zone = element?.closest<HTMLElement>("[data-drop-folder]");
      if (!zone) return null;
      const folderId = zone.dataset.dropFolder === "root" ? null : Number(zone.dataset.dropFolder);
      if (item.kind === "profile") return folderId === item.profile.folderId ? null : { folderId };
      const { folder } = item;
      if (folderId === folder.parentId) return null;
      if (folderId !== null && isInsideFolder(allFolders, folderId, folder.id)) return null;
      return { folderId };
    },
    onDrop: (item, { folderId }) => {
      if (item.kind === "profile") actions.onMoveProfile(item.profile, folderId);
      else actions.onMoveFolder(item.folder, folderId);
    },
  });
  const rootTarget = dnd.drag?.drop?.folderId === null;

  return (
    <nav className="sidebar" aria-label="Profils">
      <div className="sidebar-header" data-tauri-drag-region>
        <span className="brand" data-tauri-drag-region>
          NetForge
        </span>
      </div>
      <div className="sidebar-actions">
        <button onClick={() => actions.onNewProfile(null)}>+ Profil</button>
        <button onClick={() => actions.onNewFolder(null)}>+ Dossier</button>
        <button onClick={onQuickConnect} title="Connexion sans enregistrer de profil">
          Rapide
        </button>
        <button onClick={onSettings} title="Réglages (Ctrl+,)" aria-label="Réglages">
          ⚙
        </button>
      </div>
      {error && <p className="form-error">{error}</p>}
      {empty && <p className="sidebar-empty">Aucun profil. Crée-en un avec « + Profil ».</p>}
      <ul className={`tree ${rootTarget ? "drop-target" : ""}`} role="tree" data-drop-folder="root">
        {tree.folders.map((node) => (
          <FolderItem
            key={node.folder.id}
            node={node}
            collapsed={collapsed}
            onToggle={toggle}
            dnd={dnd}
            {...actions}
          />
        ))}
        {tree.profiles.map((profile) => (
          <ProfileItem key={profile.id} profile={profile} dnd={dnd} {...actions} />
        ))}
      </ul>
      {dnd.drag && (
        <div className="drag-ghost" style={{ left: dnd.drag.x + 12, top: dnd.drag.y + 8 }}>
          {dnd.drag.item.kind === "profile" ? dnd.drag.item.profile.name : `📁 ${dnd.drag.item.folder.name}`}
        </div>
      )}
    </nav>
  );
}

interface FolderItemProps extends ProfileTreeActions {
  node: FolderNode;
  collapsed: Set<number>;
  onToggle: (id: number) => void;
  dnd: TreeDrag;
}

function FolderItem({ node, collapsed, onToggle, dnd, ...actions }: FolderItemProps) {
  const { folder } = node;
  const open = !collapsed.has(folder.id);
  const target = dnd.drag?.drop?.folderId === folder.id;
  const dragged = dnd.drag?.item.kind === "folder" && dnd.drag.item.folder.id === folder.id;
  return (
    <li role="treeitem" aria-expanded={open} data-drop-folder={folder.id}>
      <div className={`tree-row folder ${target ? "drop-target" : ""} ${dragged ? "dragged" : ""}`}>
        <button
          className="tree-label"
          onClick={() => onToggle(folder.id)}
          onPointerDown={(e) => dnd.startDrag(e, { kind: "folder", folder })}
          title="Glisser pour déplacer le dossier"
        >
          <span className="chevron" aria-hidden>
            {open ? "▾" : "▸"}
          </span>
          {folder.name}
        </button>
        <span className="row-actions">
          <button title="Nouveau profil dans ce dossier" onClick={() => actions.onNewProfile(folder.id)}>
            +
          </button>
          <button title="Nouveau sous-dossier" onClick={() => actions.onNewFolder(folder.id)}>
            ⊞
          </button>
          <button title="Renommer" onClick={() => actions.onRenameFolder(folder)}>
            ✎
          </button>
          <button title="Supprimer le dossier" onClick={() => actions.onDeleteFolder(folder)}>
            ✕
          </button>
        </span>
      </div>
      {open && (
        <ul role="group">
          {node.folders.map((child) => (
            <FolderItem
              key={child.folder.id}
              node={child}
              collapsed={collapsed}
              onToggle={onToggle}
              dnd={dnd}
              {...actions}
            />
          ))}
          {node.profiles.map((profile) => (
            <ProfileItem key={profile.id} profile={profile} dnd={dnd} {...actions} />
          ))}
        </ul>
      )}
    </li>
  );
}

function ProfileItem({ profile, dnd, ...actions }: ProfileTreeActions & { profile: Profile; dnd: TreeDrag }) {
  const dragged = dnd.drag?.item.kind === "profile" && dnd.drag.item.profile.id === profile.id;
  return (
    <li role="treeitem">
      <div className={`tree-row profile ${dragged ? "dragged" : ""}`}>
        <button
          className="tree-label"
          onPointerDown={(e) => dnd.startDrag(e, { kind: "profile", profile })}
          title={`${profile.protocol === "telnet" ? "telnet://" : `${profile.username}@`}${profile.host}:${profile.port} — double-clic pour se connecter, glisser vers un dossier pour le ranger`}
          onDoubleClick={() => actions.onConnect(profile)}
          onKeyDown={(e) => {
            if (e.key === "Enter") actions.onConnect(profile);
          }}
        >
          {profile.name}
          {profile.protocol === "telnet" && <span className="badge danger small">telnet</span>}
          {profile.legacyMode && <span className="badge legacy small">legacy</span>}
        </button>
        <span className="row-actions">
          <button title="Se connecter" onClick={() => actions.onConnect(profile)}>
            ▶
          </button>
          <button title="Modifier" onClick={() => actions.onEditProfile(profile)}>
            ✎
          </button>
          <button title="Supprimer" onClick={() => actions.onDeleteProfile(profile)}>
            ✕
          </button>
        </span>
      </div>
    </li>
  );
}
