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

export interface AppSettings {
  libraryRoot: string | null;
  libraryId: number | null;
  dataRoot: string;
  autoWatch: boolean;
  thumbnailWidth: number;
}

export interface CacheStats {
  files: number;
  bytes: number;
}

export interface CacheCleanup {
  removedFiles: number;
  removedBytes: number;
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
