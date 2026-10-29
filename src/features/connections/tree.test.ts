import { describe, expect, it } from "vitest";
import type { Profile } from "../../types/profiles";
import { buildTree, flattenFolders } from "./tree";

const profile = (id: number, folderId: number | null): Profile => ({
  id,
  folderId,
  name: `p${id}`,
  protocol: "ssh",
  host: "h",
  port: 22,
  username: "u",
  authMethod: "password",
  keyPath: null,
  hasSavedSecret: false,
  legacyMode: false,
  keepaliveSecs: 30,
  autoReconnect: false,
  pagerAutoReply: false,
  loginCommands: "",
});

describe("buildTree", () => {
  it("nests folders and places profiles, orphans going to the root", () => {
    const tree = buildTree({
      folders: [
        { id: 1, name: "Lab", parentId: null },
        { id: 2, name: "Cisco", parentId: 1 },
      ],
      profiles: [profile(10, 2), profile(11, null), profile(12, 99)],
    });
    expect(tree.folders).toHaveLength(1);
    expect(tree.folders[0].folders[0].profiles.map((p) => p.id)).toEqual([10]);
    expect(tree.profiles.map((p) => p.id)).toEqual([11, 12]);
    expect(flattenFolders(tree)).toEqual([
      { folder: { id: 1, name: "Lab", parentId: null }, depth: 0 },
      { folder: { id: 2, name: "Cisco", parentId: 1 }, depth: 1 },
    ]);
  });
});
