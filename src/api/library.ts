import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppSettings,
  CacheCleanup,
  CacheStats,
  CategorySummary,
  EpisodeSummary,
  LibraryStats,
  LibrarySummary,
  MediaProgress,
  MediaStats,
  RecentItem,
  ScanProgress,
  ScanSummary,
  SearchPage,
  SeriesPage,
  SeriesSort,
  SyncEvent,
} from "../types/library";

export function getLibraryStats(): Promise<LibraryStats> {
  return invoke<LibraryStats>("get_library_stats");
}

export function getSettings(): Promise<AppSettings> {
  return invoke<AppSettings>("get_settings");
}

export function setLibraryRoot(path: string): Promise<LibrarySummary> {
  return invoke<LibrarySummary>("set_library_root", { path });
}

export function setAutoWatch(enabled: boolean): Promise<AppSettings> {
  return invoke<AppSettings>("set_auto_watch", { enabled });
}

export function setThumbnailWidth(width: number): Promise<AppSettings> {
  return invoke<AppSettings>("set_thumbnail_width", { width });
}

export function getCacheStats(): Promise<CacheStats> {
  return invoke<CacheStats>("get_cache_stats");
}

export function clearThumbnailCache(): Promise<CacheCleanup> {
  return invoke<CacheCleanup>("clear_thumbnail_cache");
}

export function listCategories(
  libraryId: number,
): Promise<CategorySummary[]> {
  return invoke<CategorySummary[]>("list_categories", { libraryId });
}

export function listSeries(
  libraryId: number,
  category: string,
  sort: SeriesSort,
  offset: number,
  limit: number,
): Promise<SeriesPage> {
  return invoke<SeriesPage>("list_series", {
    libraryId,
    category,
    sort,
    offset,
    limit,
  });
}

export function searchSeries(
  libraryId: number,
  query: string,
  offset: number,
  limit: number,
): Promise<SearchPage> {
  return invoke<SearchPage>("search_series", {
    libraryId,
    query,
    offset,
    limit,
  });
}

export function listEpisodes(seriesId: number): Promise<EpisodeSummary[]> {
  return invoke<EpisodeSummary[]>("list_episodes", { seriesId });
}

export function scanLibrary(libraryId: number): Promise<ScanSummary> {
  return invoke<ScanSummary>("scan_library", { libraryId });
}

export function cancelScan(): Promise<boolean> {
  return invoke<boolean>("cancel_scan");
}

export function onScanProgress(
  handler: (progress: ScanProgress) => void,
): Promise<UnlistenFn> {
  return listen<ScanProgress>("scan-progress", (event) =>
    handler(event.payload),
  );
}

export function startMediaProcessing(libraryId: number): Promise<MediaStats> {
  return invoke<MediaStats>("start_media_processing", { libraryId });
}

export function cancelMediaProcessing(): Promise<boolean> {
  return invoke<boolean>("cancel_media_processing");
}

export function getMediaStats(libraryId: number): Promise<MediaStats> {
  return invoke<MediaStats>("get_media_stats", { libraryId });
}

export function onMediaProgress(
  handler: (progress: MediaProgress) => void,
): Promise<UnlistenFn> {
  return listen<MediaProgress>("media-progress", (event) =>
    handler(event.payload),
  );
}

export function playEpisode(episodeId: number): Promise<void> {
  return invoke<void>("open_episode", { episodeId });
}

export function listRecent(
  libraryId: number,
  limit: number,
): Promise<RecentItem[]> {
  return invoke<RecentItem[]>("list_recent", { libraryId, limit });
}

export function onLibrarySync(
  handler: (event: SyncEvent) => void,
): Promise<UnlistenFn> {
  return listen<SyncEvent>("library-sync", (event) =>
    handler(event.payload),
  );
}
