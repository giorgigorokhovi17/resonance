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
  /** GIF or video the user picked as artwork (`coverId` is then its still poster). */
  motionId?: string;
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
  /** Seconds of crossfade between songs; 0 = off (gapless). */
  crossfade: number;
  theme: "dark" | "light" | "system";
  closeToTray: boolean;
  watchFolders: boolean;
  /** The Dynamic Island. */
  island: boolean;
  islandOptions: IslandOptions;
  eq: EqSettings;
  appearance: Appearance;
  soulseek: SoulseekSettings;
  youtube: { enabled: boolean };
}

export type IslandPosition =
  | "topLeft"
  | "topCenter"
  | "topRight"
  | "middleLeft"
  | "middleRight"
  | "bottomLeft"
  | "bottomCenter"
  | "bottomRight"
  | "custom";

export interface IslandOptions {
  /** "custom" is where it was dragged to (`x`, `y`: the collapsed island's top-left, screen px). */
  position: IslandPosition;
  x: number | null;
  y: number | null;
  locked: boolean;
  clickThrough: boolean;
  onlyWhenMinimized: boolean;
  showTitle: boolean;
  showArtist: boolean;
  expandOnHover: boolean;
  /** "cover", a palette id from lib/island.ts, or "custom" (`customColors`). */
  colors: string;
  customColors: string[];
}

export interface Appearance {
  preset: string;
  accent: string | null;
  accent2: string | null;
  coverAccent: boolean;
  pureBlack: boolean;
  ambient: boolean;
  ambientIntensity: number;
  backdropBlur: number;
  corners: "sharp" | "default" | "round";
  uiScale: number;
  /** Custom photo, GIF or video behind the app. */
  background: Backdrop | null;
  /** Where the custom background shows. */
  backgroundScope: "app" | "nowPlaying";
  /** Where GIF and video artwork moves. */
  animatedArt: AnimatedArt;
  /** Playlist page design. */
  playlistLayout: PlaylistLayout;
}

/** "classic": header above the songs; "split": cover, details and lyrics left, songs right. */
export type PlaylistLayout = "classic" | "split";

export interface LyricLine {
  /** Start in milliseconds, for timed (LRC) lyrics. */
  timeMs: number | null;
  text: string;
}

export interface Lyrics {
  lines: LyricLine[];
  /** Every line has a time. */
  synced: boolean;
  /** "lrc" (a file next to the song), "tags", or "lrclib" (found online). */
  source: string;
}

export interface LyricsSearch {
  lyrics: Lyrics | null;
  /** Why there are none, in plain words. */
  message: string | null;
}

export interface SleepStatus {
  mode: "off" | "minutes" | "endOfSong";
  /** Seconds left, for "minutes". */
  remaining: number | null;
}

export interface TopSong {
  path: string;
  title: string;
  artist: string;
  plays: number;
  seconds: number;
  /** The song in the library now, if it's still there. */
  track: Track | null;
}

export interface TopArtist {
  name: string;
  plays: number;
  seconds: number;
  songs: number;
}

export interface TopAlbum {
  album: string;
  artist: string;
  plays: number;
  seconds: number;
  track: Track | null;
}

export interface Stats {
  plays: number;
  seconds: number;
  songs: number;
  artists: number;
  albums: number;
  topSongs: TopSong[];
  topArtists: TopArtist[];
  topAlbums: TopAlbum[];
  /** Seconds per hour of the day, local time. */
  byHour: number[];
  /** Seconds per weekday, Monday first. */
  byWeekday: number[];
  /** [unix seconds at local midnight, seconds listened] */
  topDay: [number, number] | null;
  longestStreak: number;
  firstPlay: number | null;
}

export type AnimatedArt = "everywhere" | "nowPlaying" | "off";

export type MediaKind = "image" | "gif" | "webp-animated" | "video";

/** A custom photo, GIF or video stored in the app. */
export interface Media {
  id: string;
  kind: MediaKind;
  mime: string;
  width: number;
  height: number;
  durationMs: number | null;
  bytes: number;
  /** Still image shown wherever the file isn't played. */
  posterId: string | null;
}

/** A photo, GIF or video behind a playlist page or the app, with a scrim on top. */
export interface Backdrop {
  mediaId: string;
  fit: "cover" | "contain";
  /** Scrim strength 0..0.8; null uses the mode's default (55% dark, 35% light). */
  dim: number | null;
  /** px, 0..40 */
  blur: number;
}

export type MediaPurpose = "cover" | "background";

/** What the backend made of a chosen file. */
export type StagedMedia =
  | { type: "cover"; coverId: string }
  | { type: "ready"; media: Media }
  | { type: "pending"; id: string; kind: MediaKind; mime: string; bytes: number };

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
  /** GIF or video cover (`coverId` is its poster). */
  motionId: string | null;
  background: Backdrop | null;
  previewCovers: string[];
  /** Import source ("m3u", "xspf", …); null for playlists made in the app. */
  source: string | null;
  sourceId: string | null;
}

export interface PlaylistEntry {
  track: Track;
  missing: boolean;
  /** The imported reference this entry came from (for review and Soulseek search). */
  refKey: string | null;
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
  /** Entries with nothing like them in the library. */
  missing: ReviewItem[];
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
  media: number;
  /** Custom GIFs, videos or photos left out because they're too large. */
  mediaSkipped: number;
  bytes: number;
}

export interface RestoreSummary {
  playlists: number;
  presets: number;
  artwork: number;
  media: number;
  /** Custom media that wasn't in the backup (too large) or couldn't be restored, one line each. */
  skippedMedia: string[];
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

// ---- Soulseek -------------------------------------------------------------------------------

export interface SoulseekSettings {
  /** null = a "Soulseek" folder inside the first library folder. */
  downloadFolder: string | null;
  listen: boolean;
  listenPort: number;
}

export interface SlskSettingsView extends SoulseekSettings {
  effectiveFolder: string | null;
  inLibrary: boolean;
}

export type SlskConnState = "signedOut" | "connecting" | "signedIn";

export interface SlskStatus {
  state: SlskConnState;
  username: string | null;
  remembered: boolean;
  error: string | null;
}

export interface SlskFile {
  path: string;
  name: string;
  size: number;
  format: string;
  bitrate: number | null;
  length: number | null;
  vbr: boolean;
  sampleRate: number | null;
  bitDepth: number | null;
}

export interface SlskFolder {
  path: string;
  files: SlskFile[];
}

export interface SlskUser {
  username: string;
  freeSlots: boolean;
  speed: number;
  folders: SlskFolder[];
}

export interface SlskSearchUpdate {
  id: number;
  query: string;
  users: SlskUser[];
  fileCount: number;
  done: boolean;
}

export type SlskJobState = "queued" | "downloading" | "done" | "failed" | "cancelled";

export interface SlskJob {
  id: number;
  username: string;
  remotePath: string;
  name: string;
  size: number;
  folder: string;
  state: SlskJobState;
  /** Imported playlist entry this download fills in when done. */
  linkKey: string | null;
  bytes: number;
  speed: number;
  error: string | null;
}

export interface SlskDownloadRequest {
  username: string;
  path: string;
  size: number;
  /** Link the finished file to this imported playlist entry. */
  linkKey?: string;
  /** File name for a YouTube download. */
  name?: string;
}

/** YouTube downloads (yt-dlp). `username` "YouTube" marks their files and jobs. */
export interface YtStatus {
  enabled: boolean;
  state: "missing" | "installing" | "ready" | "failed";
  version: string | null;
  error: string | null;
}

export const YOUTUBE = "YouTube";

/** Where missing songs are looked for. */
export type FindMode = "soulseekFirst" | "youTube";

/** A public link Undertone can import. */
export interface LinkInfo {
  service: "Spotify" | "YouTube" | "Apple Music";
  kind: "playlist" | "album";
}

export interface LinkImport extends ImportResult {
  service: LinkInfo["service"];
  /** Set when the service only gave part of the playlist. */
  note: string | null;
}

/** A Soulseek file judged acceptable for a missing song. */
export interface FoundFile extends Omit<SlskFile, "path"> {
  username: string;
  path: string;
  folder: string;
  freeSlots: boolean;
  speed: number;
  score: number;
  why: string;
}

export type FindState = "waiting" | "searching" | "found" | "notFound" | "failed" | "skipped";

export interface FindResult {
  refKey: string;
  /** The song as the playlist's source names it. */
  title: string;
  artists: string[];
  state: FindState;
  query: string;
  files: FoundFile[];
  error: string | null;
}

export interface FindProgress {
  playlistId: number;
  result: FindResult;
  done: number;
  total: number;
  running: boolean;
}
