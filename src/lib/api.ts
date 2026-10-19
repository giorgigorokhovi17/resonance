import { invoke } from "@tauri-apps/api/core";
import type {
  AutoEqImport,
  DeviceInfo,
  EqPreset,
  EqSettings,
  ExportSummary,
  ImportResult,
  LibraryFolder,
  MatchDecision,
  Playlist,
  PlaylistDetail,
  PlaylistReview,
  OutputMode,
  PlayerStatus,
  QueueSnapshot,
  RepeatMode,
  RestoreSummary,
  Settings,
  Track,
} from "./types";

export const api = {
  // playback
  getStatus: () => invoke<PlayerStatus>("get_status"),
  playFiles: (paths: string[]) => invoke<void>("play_files", { paths }),
  togglePlay: () => invoke<void>("toggle_play"),
  play: () => invoke<void>("play"),
  pause: () => invoke<void>("pause"),
  stop: () => invoke<void>("stop"),
  next: () => invoke<void>("next_track"),
  previous: () => invoke<void>("previous_track"),
  seek: (position: number) => invoke<void>("seek", { position }),
  setVolume: (volume: number) => invoke<void>("set_volume", { volume }),
  setMuted: (muted: boolean) => invoke<void>("set_muted", { muted }),

  // library
  getLibraryFolders: () => invoke<LibraryFolder[]>("get_library_folders"),
  addLibraryFolder: (path: string) => invoke<LibraryFolder[]>("add_library_folder", { path }),
  removeLibraryFolder: (id: number) => invoke<LibraryFolder[]>("remove_library_folder", { id }),
  rescanLibrary: (force = false) => invoke<void>("rescan_library", { force }),
  isScanning: () => invoke<boolean>("is_scanning"),
  getTracks: () => invoke<Track[]>("get_tracks"),
  playTracks: (ids: number[], start = 0) => invoke<void>("play_tracks", { ids, start }),
  enqueueTracks: (ids: number[], next: boolean) => invoke<number>("enqueue_tracks", { ids, next }),

  // output
  listDevices: () => invoke<DeviceInfo[]>("list_devices"),
  setOutput: (mode: OutputMode, deviceId: string | null) => invoke<Settings>("set_output", { mode, deviceId }),

  playPaths: (paths: string[], start = 0) => invoke<void>("play_paths", { paths, start }),

  // queue
  getQueue: () => invoke<QueueSnapshot>("get_queue"),
  playQueueItem: (uid: number) => invoke<void>("play_queue_item", { uid }),
  removeFromQueue: (uids: number[]) => invoke<void>("remove_from_queue", { uids }),
  moveQueueItem: (from: number, to: number) => invoke<void>("move_queue_item", { from, to }),
  clearQueue: () => invoke<void>("clear_queue"),
  saveQueueAsPlaylist: (name: string) => invoke<number>("save_queue_as_playlist", { name }),

  // playlists
  getPlaylists: () => invoke<Playlist[]>("get_playlists"),
  createPlaylist: (name: string, ids: number[] = []) => invoke<number>("create_playlist", { name, ids }),
  renamePlaylist: (id: number, name: string) => invoke<void>("rename_playlist", { id, name }),
  deletePlaylist: (id: number) => invoke<void>("delete_playlist", { id }),
  getPlaylist: (id: number) => invoke<PlaylistDetail>("get_playlist", { id }),
  addToPlaylist: (id: number, ids: number[]) => invoke<number>("add_to_playlist", { id, ids }),
  setPlaylistPaths: (id: number, paths: string[]) => invoke<void>("set_playlist_paths", { id, paths }),
  /** M3U/M3U8, PLS, XSPF or WPL. Re-importing the same file updates its playlist. */
  importPlaylist: (path: string) => invoke<ImportResult>("import_playlist", { path }),
  /** XSPF when `path` ends in .xspf, extended M3U8 otherwise. */
  exportPlaylist: (id: number, path: string) => invoke<number>("export_playlist", { id, path }),
  /** CSV of imported entries not matched to a local file, with reasons. Returns the row count. */
  exportUnmatchedReport: (id: number, path: string) => invoke<number>("export_unmatched_report", { id, path }),
  getPlaylistReview: (id: number) => invoke<PlaylistReview>("get_playlist_review", { id }),
  /** Applies to every playlist with this reference. Returns how many playlists changed. */
  decideMatch: (key: string, decision: MatchDecision, path: string | null = null) =>
    invoke<number>("decide_match", { key, decision, path }),
  updatePlaylist: (id: number, name: string, description: string) =>
    invoke<void>("update_playlist", { id, name, description }),
  importCoverImage: (path: string) => invoke<string>("import_cover_image", { path }),
  setPlaylistCover: (id: number, coverId: string | null) => invoke<void>("set_playlist_cover", { id, coverId }),
  setTracksCoverHidden: (ids: number[], hidden: boolean) => invoke<void>("set_tracks_cover_hidden", { ids, hidden }),
  setTracksCover: (ids: number[], coverId: string | null) => invoke<void>("set_tracks_cover", { ids, coverId }),
  setShuffle: (shuffle: boolean) => invoke<void>("set_shuffle", { shuffle }),
  setRepeat: (repeat: RepeatMode) => invoke<void>("set_repeat", { repeat }),

  // equalizer
  setEq: (eq: EqSettings) => invoke<EqSettings>("set_eq", { eq }),
  getEqPresets: () => invoke<EqPreset[]>("get_eq_presets"),
  saveEqPreset: (name: string, eq: EqSettings) => invoke<EqPreset[]>("save_eq_preset", { name, eq }),
  deleteEqPreset: (name: string) => invoke<EqPreset[]>("delete_eq_preset", { name }),
  importAutoEq: (path: string) => invoke<AutoEqImport>("import_autoeq", { path }),

  // backup
  exportBackup: (path: string, ui: Record<string, string>) => invoke<ExportSummary>("export_backup", { path, ui }),
  importBackup: (path: string) => invoke<RestoreSummary>("import_backup", { path }),

  // misc
  openUrl: (url: string) => invoke<void>("open_url", { url }),

  // settings
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (patch: Partial<Pick<Settings, "theme" | "closeToTray" | "watchFolders" | "replayGain" | "replayGainPreamp" | "appearance">>) =>
    invoke<Settings>("update_settings", { patch }),
};
