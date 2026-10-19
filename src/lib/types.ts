export type OutputMode = "shared" | "exclusive";
export type ReplayGainMode = "off" | "track" | "album";
export type RepeatMode = "off" | "all" | "one";
export type PlayState = "stopped" | "playing" | "paused";

export interface Track {
  id: number;
  path: string;
  title: string;
  artist: string;
  album: string;
  albumArtist: string;
  genre: string;
  year?: number;
  trackNo?: number;
  discNo?: number;
  duration: number;
  sampleRate?: number;
  bitDepth?: number;
  channels?: number;
  bitrate?: number;
  codec: string;
  fileSize: number;
  mtime: number;
  coverId?: string;
  rgTrackGain?: number;
  rgTrackPeak?: number;
  rgAlbumGain?: number;
  rgAlbumPeak?: number;
  addedAt: number;
  coverHidden?: boolean;
  /** Artwork the user picked for this song in the app. */
  customCover?: boolean;
  /** ISRC from the tags (valid values only). */
  isrc?: string;
  /** MusicBrainz recording id from the tags. */
  mbid?: string;
}

export interface SourceFormat {
  codec: string;
  sampleRate: number;
  bitsPerSample: number | null;
  channels: number;
  bitrateKbps: number | null;
  lossless: boolean;
  float: boolean;
}

export interface OutputFormat {
  mode: OutputMode;
  deviceId: string | null;
  deviceName: string;
  sampleRate: number;
  channels: number;
  bits: number;
  containerBits: number;
  float: boolean;
}

export interface PlayerStatus {
  state: PlayState;
  position: number;
  duration: number;
  currentUid: number | null;
  track: Track | null;
  source: SourceFormat | null;
  output: OutputFormat | null;
  outputMode: OutputMode;
  bitPerfect: boolean;
  resampled: boolean;
  volume: number;
  muted: boolean;
  replayGainDb: number | null;
  eqActive: boolean;
  underruns: number;
}

export interface QueueItem {
  uid: number;
  track: Track;
}

export interface QueueSnapshot {
  items: QueueItem[];
  currentUid: number | null;
  shuffle: boolean;
  repeat: RepeatMode;
}

export type FilterType = "peaking" | "lowShelf" | "highShelf" | "lowPass" | "highPass";

export interface EqBand {
  enabled: boolean;
  filter: FilterType;
  freq: number;
  gain: number;
  q: number;
}

export interface EqSettings {
  enabled: boolean;
  preamp: number;
  autoHeadroom: boolean;
  bands: EqBand[];
  presetName: string | null;
}

export interface EqPreset {
  name: string;
  eq: EqSettings;
}

export interface AutoEqImport {
  eq: EqSettings;
  name: string;
  warnings: string[];
}

export interface Settings {
  outputMode: OutputMode;
  deviceId: string | null;
  volume: number;
  replayGain: ReplayGainMode;
  replayGainPreamp: number;
  theme: "dark" | "light" | "system";
  closeToTray: boolean;
  watchFolders: boolean;
  eq: EqSettings;
}

export interface LibraryFolder {
  id: number;
  path: string;
  trackCount: number;
}

export interface ScanProgress {
  phase: "walking" | "reading" | "artwork" | "saving";
  done: number;
  total: number;
  folder: string;
}

export interface ScanSummary {
  added: number;
  updated: number;
  removed: number;
  total: number;
  errors: number;
  elapsedMs: number;
}

export interface Playlist {
  id: number;
  name: string;
  createdAt: number;
  updatedAt: number;
  trackCount: number;
  description: string;
  coverId: string | null;
  previewCovers: string[];
  /** Import source ("m3u", "xspf", …); null for playlists made in the app. */
  source: string | null;
  sourceId: string | null;
}

export interface PlaylistEntry {
  track: Track;
  missing: boolean;
}

export interface PlaylistDetail {
  playlist: Playlist;
  entries: PlaylistEntry[];
}

/** A track as an external source describes it (not necessarily a local file). */
export interface TrackRef {
  title: string;
  artists: string[];
  album: string;
  durationMs: number | null;
  isrc: string | null;
  mbid: string | null;
  serviceIds: Record<string, string>;
  location: string | null;
}

export type MatchTier = "decision" | "path" | "pathSuffix" | "isrc" | "mbid" | "fuzzy";
export type MatchReason =
  | "none"
  | "noCandidate"
  | "ambiguous"
  | "durationMismatch"
  | "versionMismatch"
  | "drift"
  | "lowConfidence"
  | "markedMissing"
  | "skipped";

export interface MatchCandidate {
  /** 0 for a file that isn't in the library. */
  trackId: number;
  path: string;
  title: string;
  artist: string;
  /** Seconds. */
  duration: number;
  /** 0..1 */
  score: number;
  /** Signal summary: "title same · artist same · Δ 0.4 s". */
  why: string;
}

export interface MatchResult {
  refKey: string;
  accepted: MatchCandidate | null;
  tier: MatchTier | null;
  reason: MatchReason;
  explanation: string;
  candidates: MatchCandidate[];
}

export interface ReviewItem {
  /** 1-based positions in the playlist. */
  positions: number[];
  reference: TrackRef;
  result: MatchResult;
}

export interface PlaylistReview {
  pending: ReviewItem[];
  decided: ReviewItem[];
  matched: number;
  unmatched: number;
}

export type MatchDecision = "accept" | "missing" | "skip" | "clear";

export interface ImportResult {
  id: number;
  name: string;
  total: number;
  /** Stored entries whose file isn't on this PC. */
  missing: number;
  /** Streams and web links left out. */
  skipped: number;
  /** An earlier import of the same file was updated instead of adding a new playlist. */
  updated: boolean;
  /** Entries matched to a local file, by matcher tier ("path", "path-suffix", "isrc", "mbid", "fuzzy", "decision"). */
  matched: Record<string, number>;
  /** Entries with a candidate that needs a human decision. */
  review: number;
}

export interface ExportSummary {
  playlists: number;
  presets: number;
  artwork: number;
  bytes: number;
}

export interface RestoreSummary {
  playlists: number;
  presets: number;
  artwork: number;
  foldersAdded: string[];
  foldersMissing: string[];
  relocated: number;
  unresolved: number;
  ui: Record<string, string> | null;
  settings: Settings;
}

export interface DeviceInfo {
  id: string;
  name: string;
  isDefault: boolean;
}
