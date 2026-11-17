import { isTauri } from "@tauri-apps/api/core";
import { commands, type Space, type SpacesState } from "./bindings";

export type { Space, SpacesState } from "./bindings";

const unwrap = <T>(r: { status: "ok"; data: T } | { status: "error"; error: unknown }): T => {
  if (r.status === "error") throw r.error;
  return r.data;
};

const preview: SpacesState = {
  activeId: "default",
  spaces: [{ id: "default", name: "Моё пространство", root: "" }],
};

export const listSpaces = (): Promise<SpacesState> => (isTauri() ? commands.spacesList() : Promise.resolve(preview));
export const selectSpace = async (id: string): Promise<SpacesState> => unwrap(await commands.spacesSelect(id));
export const createSpace = async (name: string, root: string): Promise<SpacesState> =>
  unwrap(await commands.spacesCreate(name, root));
export const renameSpace = async (id: string, name: string): Promise<SpacesState> =>
  unwrap(await commands.spacesRename(id, name));
export const deleteSpace = async (id: string): Promise<SpacesState> =>
  unwrap(await commands.spacesDelete(id));
