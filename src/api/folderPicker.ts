// The OS folder picker for adding a project (Tauri dialog plugin). It can only
// choose a folder on this machine, so it is for the local host in the app alone.

import { open } from "@tauri-apps/plugin-dialog";

import { isTauri } from "./create";

/** Whether the native picker applies: inside the Tauri app, on the local host. */
export function canPickLocalFolder(isRemoteHost: boolean): boolean {
  return isTauri() && !isRemoteHost;
}

/** Opens the folder picker. `null` when the user cancels. */
export async function pickLocalFolder(title: string): Promise<string | null> {
  const picked = await open({ directory: true, multiple: false, title });
  return picked ?? null;
}
