import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/api";
import type { Media } from "../lib/types";

/** Custom photos, GIFs and videos the app has stored, by id. */
interface MediaStore {
  byId: Map<string, Media>;
  load: () => Promise<void>;
}

export const useMedia = create<MediaStore>((set) => ({
  byId: new Map(),
  load: async () => set({ byId: new Map((await api.getMedia()).map((m) => [m.id, m])) }),
}));

/** URL of a stored file (served with Range support, so video can loop and seek). */
export function mediaUrl(id: string): string {
  return `http://media.localhost/${id}`;
}

/** The stored GIF or video for a motion id, if it's known and can move. */
export function useMotion(motionId: string | null | undefined): Media | null {
  const m = useMedia((s) => (motionId ? s.byId.get(motionId) : undefined));
  return m && m.kind !== "image" ? m : null;
}

let initialized = false;
export async function initMedia() {
  if (initialized) return;
  initialized = true;
  await listen("media:changed", () => useMedia.getState().load());
  await useMedia.getState().load();
}
