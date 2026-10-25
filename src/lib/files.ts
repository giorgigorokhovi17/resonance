import { open, save } from "@tauri-apps/plugin-dialog";
import { api } from "./api";
import { isMobile } from "./platform";

export interface FileFilter {
  name: string;
  extensions: string[];
}

/**
 * Pick one or more files to import (playlists, backups, AutoEQ). Desktop opens the Tauri file
 * dialog; Android opens the system document picker, copies the picked documents into the app's
 * cache, and returns those paths so the backend can read them like regular files.
 */
export async function pickFiles(opts: {
  title: string;
  multiple?: boolean;
  filters: FileFilter[];
}): Promise<string[]> {
  if (isMobile) {
    return api.pickFiles(!!opts.multiple);
  }
  const picked = await open({
    title: opts.title,
    multiple: !!opts.multiple,
    filters: opts.filters,
  });
  if (!picked) return [];
  return Array.isArray(picked) ? picked : [picked];
}

/**
 * Save an exported file (playlist, unmatched report, backup). Desktop asks where to save first and
 * writes directly there; Android writes to a temporary file in the app's cache and then opens the
 * system "Save as" screen to copy it out. Returns null when the user cancels.
 */
export async function saveFile<T>(
  opts: { title: string; name: string; filters: FileFilter[] },
  write: (path: string) => Promise<T>,
): Promise<{ path: string; result: T } | null> {
  if (!isMobile) {
    const path = await save({
      title: opts.title,
      defaultPath: opts.name,
      filters: opts.filters,
    });
    if (!path) return null;
    return { path, result: await write(path) };
  }
  const temp = await api.exportPath(opts.name);
  const result = await write(temp);
  const saved = await api.exportFile(temp, opts.name);
  return saved ? { path: opts.name, result } : null;
}
