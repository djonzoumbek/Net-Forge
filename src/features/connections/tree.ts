import type { Folder, Library, Profile } from "../../types/profiles";

export interface FolderNode {
  folder: Folder;
  folders: FolderNode[];
  profiles: Profile[];
}

export interface Tree {
  folders: FolderNode[];
  /** Profiles at the root (no folder). */
  profiles: Profile[];
}

/** Builds the folder hierarchy from the flat lists returned by the backend. */
export function buildTree({ folders, profiles }: Library): Tree {
  const nodes = new Map<number, FolderNode>(
    folders.map((folder) => [folder.id, { folder, folders: [], profiles: [] }]),
  );
  const root: Tree = { folders: [], profiles: [] };
  for (const node of nodes.values()) {
    const parent = node.folder.parentId === null ? undefined : nodes.get(node.folder.parentId);
    (parent ? parent.folders : root.folders).push(node);
  }
  for (const profile of profiles) {
    const parent = profile.folderId === null ? undefined : nodes.get(profile.folderId);
    (parent ? parent.profiles : root.profiles).push(profile);
  }
  return root;
}

/** Folders in display order with their depth, for indented `<select>` options. */
export function flattenFolders(tree: Tree): { folder: Folder; depth: number }[] {
  const out: { folder: Folder; depth: number }[] = [];
  const walk = (nodes: FolderNode[], depth: number) => {
    for (const node of nodes) {
      out.push({ folder: node.folder, depth });
      walk(node.folders, depth + 1);
    }
  };
  walk(tree.folders, 0);
  return out;
}
