// Typed wrappers around the profile / folder commands.
import { invoke } from "@tauri-apps/api/core";
import type { AppSettings } from "../types/settings";
import type { ButtonInput, CommandButton, Folder, Library, Profile, ProfileInput } from "../types/profiles";

export const listLibrary = () => invoke<Library>("library_list");

export const createProfile = (input: ProfileInput) => invoke<Profile>("profile_create", { input });

export const updateProfile = (id: number, input: ProfileInput) =>
  invoke<Profile>("profile_update", { id, input });

export const deleteProfile = (id: number) => invoke<void>("profile_delete", { id });

export const saveProfileSecret = (id: number, secret: string) =>
  invoke<void>("profile_save_secret", { id, secret });

export const createFolder = (name: string, parentId: number | null) =>
  invoke<Folder>("folder_create", { name, parentId });

export const renameFolder = (id: number, name: string) =>
  invoke<void>("folder_rename", { id, name });

export const deleteFolder = (id: number) => invoke<void>("folder_delete", { id });

export const moveProfile = (id: number, folderId: number | null) =>
  invoke<void>("profile_move", { id, folderId });

export const moveFolder = (id: number, parentId: number | null) =>
  invoke<void>("folder_move", { id, parentId });

export const listButtons = () => invoke<CommandButton[]>("buttons_list");

export const createButton = (input: ButtonInput) => invoke<CommandButton>("button_create", { input });

export const updateButton = (id: number, input: ButtonInput) =>
  invoke<CommandButton>("button_update", { id, input });

export const deleteButton = (id: number) => invoke<void>("button_delete", { id });

export const getSettings = () => invoke<AppSettings>("settings_get");

export const updateSettings = (settings: AppSettings) =>
  invoke<AppSettings>("settings_update", { settings });
