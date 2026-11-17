/** Local on-PC spaces (D-178). A select command restarts the native core. */
import { isTauri } from "@tauri-apps/api/core";
import { commands, type LocalSpace } from "./bindings";

export type { LocalSpace } from "./bindings";

const unwrap = <T>(result: { status: "ok"; data: T } | { status: "error"; error: unknown }): T => {
  if (result.status === "error") throw result.error;
  return result.data;
};

export const listSpaces = async (): Promise<LocalSpace[]> =>
  isTauri() ? commands.spacesList() : [{ id: "default", name: "", active: true }];

export const createSpace = async (name: string): Promise<LocalSpace> =>
  unwrap(await commands.spacesCreate(name));

export const renameSpace = async (id: string, name: string): Promise<void> => {
  unwrap(await commands.spacesRename(id, name));
};

export const removeSpace = async (id: string): Promise<void> => {
  unwrap(await commands.spacesRemove(id));
};

/** Native process relaunches on success; the returned promise does not normally resolve. */
export const selectSpace = async (id: string): Promise<void> => {
  unwrap(await commands.spacesSelect(id));
};
