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
  PlayerError,
  PlayerRect,
  PlayerStarted,
  PlayerState,
  RecentItem,
  ScanProgress,
  ScanSummary,
  SearchPage,
  SeriesPage,
  SeriesSort,
  SeriesSummary,
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

export function getSeries(seriesId: number): Promise<SeriesSummary> {
  return invoke<SeriesSummary>("get_series", { seriesId });
}

export function startPlayer(
  episodeId: number,
  rect: PlayerRect,
): Promise<PlayerStarted> {
  return invoke<PlayerStarted>("start_player", { episodeId, rect });
}

export function stopPlayer(): Promise<void> {
  return invoke<void>("stop_player");
}

export function resizePlayer(rect: PlayerRect): Promise<void> {
  return invoke<void>("resize_player", { rect });
}

export function playerSetPaused(paused: boolean): Promise<void> {
  return invoke<void>("player_set_paused", { paused });
}

export function playerSeek(positionMs: number): Promise<void> {
  return invoke<void>("player_seek", { positionMs });
}

export function playerSetVolume(volume: number): Promise<void> {
  return invoke<void>("player_set_volume", { volume });
}

export function playerSetMuted(muted: boolean): Promise<void> {
  return invoke<void>("player_set_muted", { muted });
}

export function playerSetSpeed(speed: number): Promise<void> {
  return invoke<void>("player_set_speed", { speed });
}

export function playerToggleSubtitles(): Promise<void> {
  return invoke<void>("player_toggle_subtitles");
}

export function setPlayerFullscreen(fullscreen: boolean): Promise<void> {
  return invoke<void>("set_player_fullscreen", { fullscreen });
}

export function onPlayerState(
  handler: (state: PlayerState) => void,
): Promise<UnlistenFn> {
  return listen<PlayerState>("player-state", (event) =>
    handler(event.payload),
  );
}

export function onPlayerClosed(
  handler: (episodeId: number) => void,
): Promise<UnlistenFn> {
  return listen<number>("player-closed", (event) =>
    handler(event.payload),
  );
}

export function onPlayerError(
  handler: (payload: PlayerError) => void,
): Promise<UnlistenFn> {
  return listen<PlayerError>("player-error", (event) =>
    handler(event.payload),
  );
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
