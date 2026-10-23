export interface LibraryStats {
  videoCount: number;
  libraryCount: number;
  databasePath: string;
  dataRoot: string;
}

export interface LibrarySummary {
  id: number;
  rootPath: string;
  displayName: string;
  enabled: boolean;
  lastScanAt: number | null;
  videoCount: number;
  createdAt: number;
}

export type PageTransition = "fade" | "slide";
export type SeriesTransition = "rise" | "zoom" | "slide" | "flip";
export type WindowMode = "windowed" | "borderless";

export interface AppSettings {
  libraryRoot: string | null;
  libraryId: number | null;
  dataRoot: string;
  autoWatch: boolean;
  autoStart: boolean;
  thumbnailWidth: number;
  pageTransition: PageTransition;
  seriesTransition: SeriesTransition;
  windowMode: WindowMode;
  toolsReady: boolean;
}

export interface CacheStats {
  files: number;
  bytes: number;
}

export interface CacheCleanup {
  removedFiles: number;
  removedBytes: number;
}

/// 批量检查/修复 kanfanos.toml 的结果。
export interface ConfigRepairSummary {
  checked: number;
  created: number;
  updated: number;
  namesCleared: number;
  invalid: number;
  errors: number;
}

export interface CategorySummary {
  key: string;
  label: string;
  count: number;
}

export type SeriesSort = "name" | "episodes" | "updated";

export interface SeriesSummary {
  id: number;
  libraryId: number;
  category: string;
  name: string;
  path: string;
  episodeCount: number;
  coverKey: string | null;
  coverPath: string | null;
  coverVersion: number | null;
  watchCount: number;
  /// 用户评分 0-10；null 表示未评分。
  rating: number | null;
}

export interface SeriesPage {
  items: SeriesSummary[];
  total: number;
}

export interface SearchItem extends SeriesSummary {
  matchedEpisodes: number;
  matchPreview: string | null;
}

export interface SearchPage {
  items: SearchItem[];
  total: number;
}

/// 「上一季 / 下一季」的目标番剧。
export interface SeriesOption {
  id: number;
  name: string;
  category: string;
  path: string;
}

/// 选集页配置弹窗的当前值。
export interface SeriesConfigView {
  name: string;
  aliases: string[];
  rating: number | null;
  awards: string[];
  previous: SeriesOption | null;
  next: SeriesOption | null;
}

/// 自动同步反向季链接时会覆盖的目标现有链接。
export interface SeasonLinkConflict {
  target: SeriesOption;
  /// 需要修改目标哪一侧：`next` 对应本番的「上一季」，`previous` 对应「下一季」。
  direction: "next" | "previous";
  current: SeriesOption | null;
}

export interface EpisodeSummary {
  id: number;
  filename: string;
  path: string;
  fileSize: number;
  durationMs: number | null;
  width: number | null;
  height: number | null;
  thumbnailKey: string | null;
  lastPositionMs: number | null;
  completed: boolean;
}

export interface RecentItem {
  videoId: number;
  seriesId: number | null;
  seriesName: string | null;
  category: string | null;
  filename: string;
  path: string;
  durationMs: number | null;
  thumbnailKey: string | null;
  lastOpenedAt: number;
  lastPositionMs: number;
  completed: boolean;
}

export interface PlayerRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface SubtitleTrack {
  id: number;
  title: string;
  lang: string;
}

export interface AudioTrack {
  id: number;
  title: string;
  lang: string;
}

export interface PlayerState {
  episodeId: number;
  positionMs: number;
  durationMs: number;
  paused: boolean;
  volume: number;
  muted: boolean;
  speed: number;
  ended: boolean;
  subtitles: SubtitleTrack[];
  subtitleId: number | null;
  subtitlesVisible: boolean;
  audioTracks: AudioTrack[];
  audioId: number | null;
}

export interface PlayerStarted {
  episodeId: number;
  durationMs: number;
  startPositionMs: number;
}

export interface PlayerError {
  episodeId: number;
  message: string;
}

export interface MediaStats {
  pending: number;
  processing: number;
  ready: number;
  failed: number;
}

export interface MediaProgress {
  libraryId: number;
  phase: "processing" | "completed" | "cancelled";
  processed: number;
  failed: number;
  total: number;
}

export interface SyncEvent {
  libraryId: number;
  phase: "watching" | "syncing" | "completed" | "offline" | "failed";
  seriesFound: number | null;
  filesFound: number | null;
  missing: number | null;
}

export interface ScanProgress {
  libraryId: number;
  phase: "scanning" | "completed" | "cancelled";
  seriesFound: number;
  filesFound: number;
  errors: number;
  currentPath: string | null;
  elapsedMs: number;
}

export interface ScanSummary {
  libraryId: number;
  seriesFound: number;
  filesFound: number;
  filesAdded: number;
  errors: number;
  missing: number;
  cancelled: boolean;
  elapsedMs: number;
}
