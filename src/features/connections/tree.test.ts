import { describe, expect, it } from "vitest";
import { defaultSerial, type Profile } from "../../types/profiles";
import { buildTree, flattenFolders, isInsideFolder } from "./tree";

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
  sessionLog: "off",
  serial: defaultSerial,
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

describe("isInsideFolder", () => {
  const folders = [
    { id: 1, name: "Labo", parentId: null },
    { id: 2, name: "Cœur", parentId: 1 },
    { id: 3, name: "R1-R4", parentId: 2 },
    { id: 4, name: "Prod", parentId: null },
  ];

  it("detects the folder itself and its descendants", () => {
    expect(isInsideFolder(folders, 1, 1)).toBe(true);
    expect(isInsideFolder(folders, 3, 1)).toBe(true);
    expect(isInsideFolder(folders, 4, 1)).toBe(false);
    expect(isInsideFolder(folders, 1, 3)).toBe(false);
  });

  it("stops on a looping hierarchy", () => {
    const loop = [
      { id: 1, name: "a", parentId: 2 },
      { id: 2, name: "b", parentId: 1 },
    ];
    expect(isInsideFolder(loop, 1, 99)).toBe(false);
  });
});
