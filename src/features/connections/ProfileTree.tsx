import { useState } from "react";
import type { Folder, Profile } from "../../types/profiles";
import type { FolderNode, Tree } from "./tree";

export interface ProfileTreeActions {
  onConnect: (profile: Profile) => void;
  onNewProfile: (folderId: number | null) => void;
  onEditProfile: (profile: Profile) => void;
  onDeleteProfile: (profile: Profile) => void;
  onNewFolder: (parentId: number | null) => void;
  onRenameFolder: (folder: Folder) => void;
  onDeleteFolder: (folder: Folder) => void;
}

interface ProfileTreeProps extends ProfileTreeActions {
  tree: Tree;
  error: string | null;
  onQuickConnect: () => void;
}

export function ProfileTree({ tree, error, onQuickConnect, ...actions }: ProfileTreeProps) {
  const [collapsed, setCollapsed] = useState<Set<number>>(new Set());
  const toggle = (id: number) =>
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const empty = tree.folders.length === 0 && tree.profiles.length === 0;

  return (
    <nav className="sidebar" aria-label="Profils">
      <div className="sidebar-header">
        <span className="brand">NetForge</span>
      </div>
      <div className="sidebar-actions">
        <button onClick={() => actions.onNewProfile(null)}>+ Profil</button>
        <button onClick={() => actions.onNewFolder(null)}>+ Dossier</button>
        <button onClick={onQuickConnect} title="Connexion sans enregistrer de profil">
          Rapide
        </button>
      </div>
      {error && <p className="form-error">{error}</p>}
      {empty && <p className="sidebar-empty">Aucun profil. Crée-en un avec « + Profil ».</p>}
      <ul className="tree" role="tree">
        {tree.folders.map((node) => (
          <FolderItem key={node.folder.id} node={node} collapsed={collapsed} onToggle={toggle} {...actions} />
        ))}
        {tree.profiles.map((profile) => (
          <ProfileItem key={profile.id} profile={profile} {...actions} />
        ))}
      </ul>
    </nav>
  );
}

interface FolderItemProps extends ProfileTreeActions {
  node: FolderNode;
  collapsed: Set<number>;
  onToggle: (id: number) => void;
}

function FolderItem({ node, collapsed, onToggle, ...actions }: FolderItemProps) {
  const { folder } = node;
  const open = !collapsed.has(folder.id);
  return (
    <li role="treeitem" aria-expanded={open}>
      <div className="tree-row folder">
        <button className="tree-label" onClick={() => onToggle(folder.id)}>
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
            <FolderItem key={child.folder.id} node={child} collapsed={collapsed} onToggle={onToggle} {...actions} />
          ))}
          {node.profiles.map((profile) => (
            <ProfileItem key={profile.id} profile={profile} {...actions} />
          ))}
        </ul>
      )}
    </li>
  );
}

function ProfileItem({ profile, ...actions }: ProfileTreeActions & { profile: Profile }) {
  return (
    <li role="treeitem">
      <div className="tree-row profile">
        <button
          className="tree-label"
          title={`${profile.username}@${profile.host}:${profile.port} — double-clic pour se connecter`}
          onDoubleClick={() => actions.onConnect(profile)}
          onKeyDown={(e) => {
            if (e.key === "Enter") actions.onConnect(profile);
          }}
        >
          {profile.name}
          {profile.legacyMode && <span className="badge legacy">legacy</span>}
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
