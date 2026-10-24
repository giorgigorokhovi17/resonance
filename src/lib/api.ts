import { invoke } from "@tauri-apps/api/core";
import type {
  FindResult,
  LinkImport,
  LinkInfo,
  AutoEqImport,
  DeviceInfo,
  EqPreset,
  EqSettings,
  ExportSummary,
  ImportResult,
  LibraryFolder,
  Backdrop,
  Lyrics,
  LyricsSearch,
  Media,
  MediaPurpose,
  StagedMedia,
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
  SleepStatus,
  SlskDownloadRequest,
  Stats,
  SlskJob,
  SlskSettingsView,
  SlskStatus,
  SoulseekSettings,
  Track,
  FindMode,
  YtStatus,
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
  /** `motionId`: a GIF or video from commitMedia; `coverId` is then its poster. */
  setPlaylistCover: (id: number, coverId: string | null, motionId: string | null = null) =>
    invoke<void>("set_playlist_cover", { id, coverId, motionId }),
  setPlaylistBackground: (id: number, background: Backdrop | null) =>
    invoke<void>("set_playlist_background", { id, background }),
  setTracksCoverHidden: (ids: number[], hidden: boolean) => invoke<void>("set_tracks_cover_hidden", { ids, hidden }),
  setTracksCover: (ids: number[], coverId: string | null, motionId: string | null = null) =>
    invoke<void>("set_tracks_cover", { ids, coverId, motionId }),

  // custom photos, GIFs and videos (see lib/mediaImport.ts)
  /** Checks a file and copies it into the app. Refusals reject with a plain message. */
  stageMedia: (path: string, purpose: MediaPurpose) => invoke<StagedMedia>("stage_media", { path, purpose }),
  /** Finishes a pending GIF/video import; `poster` is its first frame (base64 JPEG). */
  commitMedia: (id: string, purpose: MediaPurpose, width: number, height: number, durationMs: number | null, poster: string) =>
    invoke<Media>("commit_media", { id, purpose, width, height, durationMs, poster }),
  discardMedia: (id: string) => invoke<void>("discard_media", { id }),
  getMedia: () => invoke<Media[]>("get_media"),
  /** From a .lrc file next to the song or its tags; null when it has none. */
  getLyrics: (path: string) => invoke<Lyrics | null>("get_lyrics", { path }),
  /** Looks the song up on LRCLIB and keeps what's found (the file isn't changed). */
  searchLyrics: (path: string) => invoke<LyricsSearch>("search_lyrics", { path }),
  removeFoundLyrics: (path: string) => invoke<void>("remove_found_lyrics", { path }),

  // listening stats
  /** Plays started in from..to (unix seconds). */
  getStats: (from: number, to: number) =>
    invoke<Stats>("get_stats", { from, to, tzOffsetMin: -new Date().getTimezoneOffset() }),
  clearPlayHistory: () => invoke<void>("clear_play_history"),

  // sleep timer
  getSleepTimer: () => invoke<SleepStatus>("get_sleep_timer"),
  /** `minutes` pauses after that long, `endOfSong` stops when this song ends; neither = off. */
  setSleepTimer: (minutes: number | null, endOfSong = false) => invoke<SleepStatus>("set_sleep_timer", { minutes, endOfSong }),
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

  // soulseek
  slskStatus: () => invoke<SlskStatus>("slsk_status"),
  slskSignIn: (username: string, password: string, remember: boolean) =>
    invoke<SlskStatus>("slsk_sign_in", { username, password, remember }),
  slskSignOut: (forget: boolean) => invoke<void>("slsk_sign_out", { forget }),
  slskReconnect: () => invoke<SlskStatus>("slsk_reconnect"),
  ytStatus: () => invoke<YtStatus>("yt_status"),
  ytSetEnabled: (enabled: boolean) => invoke<YtStatus>("yt_set_enabled", { enabled }),
  ytInstall: () => invoke<void>("yt_install"),
  slskSearch: (query: string) => invoke<number>("slsk_search", { query }),
  slskStopSearch: () => invoke<void>("slsk_stop_search"),
  slskDownloads: () => invoke<SlskJob[]>("slsk_downloads"),
  slskDownload: (items: SlskDownloadRequest[]) => invoke<number>("slsk_download", { items }),
  slskCancel: (id: number) => invoke<void>("slsk_cancel", { id }),
  slskRetry: (id: number) => invoke<void>("slsk_retry", { id }),
  slskClearFinished: () => invoke<void>("slsk_clear_finished"),
  slskSettings: () => invoke<SlskSettingsView>("slsk_settings"),
  slskUpdateSettings: (settings: SoulseekSettings) => invoke<SlskSettingsView>("slsk_update_settings", { settings }),

  // paste-a-link import + missing songs from Soulseek
  checkLink: (url: string) => invoke<LinkInfo | null>("check_link", { url }),
  /** Public Spotify / YouTube / Apple Music playlist or album link. Re-importing updates it. */
  importLink: (url: string) => invoke<LinkImport>("import_link", { url }),
  findMissing: (playlistId: number, keys: string[] | null = null, mode: FindMode = "soulseekFirst") =>
    invoke<number>("find_missing", { playlistId, keys, mode }),
  stopFind: (playlistId: number) => invoke<void>("stop_find", { playlistId }),
  /** Cancel one song's Soulseek search. */
  skipFind: (key: string) => invoke<void>("skip_find", { key }),
  findResults: (playlistId: number, keys: string[]) =>
    invoke<{ running: boolean; results: FindResult[] }>("find_results", { playlistId, keys }),
  downloadFound: (items: SlskDownloadRequest[]) => invoke<number>("download_found", { items }),

  // misc
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  showMainWindow: () => invoke<void>("show_main_window"),
  islandMainAway: () => invoke<boolean>("island_main_away"),
  islandSetBounds: (x: number, y: number, w: number, h: number) => invoke<void>("island_set_bounds", { x, y, w, h }),
  islandAltDown: () => invoke<boolean>("island_alt_down"),

  // settings
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (patch: Partial<Pick<Settings, "theme" | "closeToTray" | "watchFolders" | "island" | "islandOptions" | "replayGain" | "replayGainPreamp" | "crossfade" | "appearance">>) =>
    invoke<Settings>("update_settings", { patch }),
};
