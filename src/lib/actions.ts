import { api } from "./api";
import type { Track } from "./types";
import { usePlayer } from "../store/player";

const toastErr = (e: unknown) => usePlayer.getState().pushToast("error", String(e));
const toastInfo = (m: string) => usePlayer.getState().pushToast("info", m);

/** Library tracks have positive ids; others (opened files) can't be addressed by id. */
const ids = (tracks: Track[]) => tracks.filter((t) => t.id > 0).map((t) => t.id);

export async function playTracks(tracks: Track[], start = 0) {
  try {
    await api.playTracks(ids(tracks), start);
  } catch (e) {
    toastErr(e);
  }
}

export async function playShuffled(tracks: Track[]) {
  if (!tracks.length) return;
  try {
    await api.setShuffle(true);
    await api.playTracks(ids(tracks), Math.floor(Math.random() * tracks.length));
  } catch (e) {
    toastErr(e);
  }
}

export async function playNext(tracks: Track[]) {
  try {
    const n = await api.enqueueTracks(ids(tracks), true);
    toastInfo(n === 1 ? `"${tracks[0].title}" plays next.` : `${n} tracks play next.`);
  } catch (e) {
    toastErr(e);
  }
}

export async function addToQueue(tracks: Track[]) {
  try {
    const n = await api.enqueueTracks(ids(tracks), false);
    toastInfo(n === 1 ? `Added "${tracks[0].title}" to the queue.` : `Added ${n} tracks to the queue.`);
  } catch (e) {
    toastErr(e);
  }
}
