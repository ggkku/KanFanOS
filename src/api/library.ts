import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppSettings,
  CacheCleanup,
  CacheStats,
  CategorySummary,
  ConfigRepairSummary,
  InvalidSeries,
  InvalidSeriesCleanup,
  EpisodeSummary,
  LibraryStats,
  LibrarySummary,
  MediaProgress,
  MediaStats,
  PageTransition,
  PlayerError,
  SeriesTransition,
  PlayerRect,
  PlayerStarted,
  PlayerState,
  RecentItem,
  ScanProgress,
  ScanSummary,
  WindowMode,
  SearchPage,
  SeasonLinkConflict,
  SeriesConfigView,
  SeriesNameConflict,
  SeriesOption,
  SeriesPage,
  SeriesSort,
  SeriesSummary,
  SeriesVideo,
  SeriesEpisodeIssues,
  SyncEvent,
  SystemAudioDevice,
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

export function setAutoStart(enabled: boolean): Promise<AppSettings> {
  return invoke<AppSettings>("set_auto_start", { enabled });
}

export function setThumbnailWidth(width: number): Promise<AppSettings> {
  return invoke<AppSettings>("set_thumbnail_width", { width });
}

export function setPageTransition(
  transition: PageTransition,
): Promise<AppSettings> {
  return invoke<AppSettings>("set_page_transition", { transition });
}

export function setSeriesTransition(
  transition: SeriesTransition,
): Promise<AppSettings> {
  return invoke<AppSettings>("set_series_transition", { transition });
}

export function setWindowMode(mode: WindowMode): Promise<AppSettings> {
  return invoke<AppSettings>("set_window_mode", { mode });
}

export function minimizeToTray(): Promise<void> {
  return invoke<void>("minimize_to_tray");
}

/** 退出前并行关闭内置 qBittorrent，不等待进程退出。 */
export function beginExit(): Promise<void> {
  return invoke<void>("begin_exit");
}

export function quitApp(): Promise<void> {
  return invoke<void>("quit_app");
}

export function getCacheStats(): Promise<CacheStats> {
  return invoke<CacheStats>("get_cache_stats");
}

export function clearThumbnailCache(): Promise<CacheCleanup> {
  return invoke<CacheCleanup>("clear_thumbnail_cache");
}

export function repairSeriesConfigs(
  libraryId: number,
): Promise<ConfigRepairSummary> {
  return invoke<ConfigRepairSummary>("repair_series_configs", { libraryId });
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

export function getSeriesNameConflicts(
  libraryId: number,
): Promise<SeriesNameConflict[]> {
  return invoke<SeriesNameConflict[]>("series_name_conflicts", { libraryId });
}

export function listInvalidSeries(
  libraryId: number,
): Promise<InvalidSeries[]> {
  return invoke<InvalidSeries[]>("list_invalid_series", { libraryId });
}

export function cleanupInvalidSeries(
  libraryId: number,
): Promise<InvalidSeriesCleanup> {
  return invoke<InvalidSeriesCleanup>("cleanup_invalid_series", { libraryId });
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

export function listExtraEpisodes(
  seriesId: number,
): Promise<EpisodeSummary[]> {
  return invoke<EpisodeSummary[]>("list_extra_episodes", { seriesId });
}

/** 按文件名分析整个媒体库的重复集数与中间缺集。 */
export function listSeriesEpisodeIssues(
  libraryId: number,
): Promise<SeriesEpisodeIssues[]> {
  return invoke<SeriesEpisodeIssues[]>("series_episode_issues", { libraryId });
}

export function markSeriesWatched(seriesId: number): Promise<number> {
  return invoke<number>("mark_series_watched", { seriesId });
}

export function clearSeriesHistory(seriesId: number): Promise<number> {
  return invoke<number>("clear_series_history", { seriesId });
}

export function setSeriesWatchCount(
  seriesId: number,
  count: number,
): Promise<number> {
  return invoke<number>("set_series_watch_count", { seriesId, count });
}

export function setSeriesRating(
  seriesId: number,
  rating: number | null,
): Promise<number | null> {
  return invoke<number | null>("set_series_rating", { seriesId, rating });
}

export function setSeriesAwards(
  seriesId: number,
  awards: string[],
): Promise<string[]> {
  return invoke<string[]>("set_series_awards", { seriesId, awards });
}

export function getSeriesAwards(seriesId: number): Promise<string[]> {
  return invoke<string[]>("get_series_awards", { seriesId });
}

export function getSeriesConfig(
  seriesId: number,
): Promise<SeriesConfigView> {
  return invoke<SeriesConfigView>("get_series_config", { seriesId });
}

/// 番剧目录下扫描到的全部视频（供选集选择器使用）。
export function listSeriesVideos(seriesId: number): Promise<SeriesVideo[]> {
  return invoke<SeriesVideo[]>("list_series_videos", { seriesId });
}

export function searchSeriesOptions(
  libraryId: number,
  query: string,
  limit: number,
): Promise<SeriesOption[]> {
  return invoke<SeriesOption[]>("search_series_options", {
    libraryId,
    query,
    limit,
  });
}

export function checkSeasonLinks(
  seriesId: number,
  previousId: number | null,
  nextId: number | null,
): Promise<SeasonLinkConflict[]> {
  return invoke<SeasonLinkConflict[]>("check_season_links", {
    seriesId,
    previousId,
    nextId,
  });
}

export function updateSeriesConfig(
  seriesId: number,
  name: string,
  aliases: string[],
  previousId: number | null,
  nextId: number | null,
  overwriteInverse: boolean,
  episodeMode: string,
  episodes: string[],
): Promise<SeriesConfigView> {
  return invoke<SeriesConfigView>("update_series_config", {
    seriesId,
    name,
    aliases,
    previousId,
    nextId,
    overwriteInverse,
    episodeMode,
    episodes,
  });
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

export function playerSetSubtitle(subtitleId: number | null): Promise<void> {
  return invoke<void>("player_set_subtitle", { subtitleId });
}

export function playerSetSubDelay(seconds: number): Promise<void> {
  return invoke<void>("player_set_sub_delay", { seconds });
}

export function playerSetAudio(audioId: number): Promise<void> {
  return invoke<void>("player_set_audio", { audioId });
}

export function playerSetAudioDevice(device: string): Promise<void> {
  return invoke<void>("player_set_audio_device", { device });
}

export function listAudioOutputDevices(): Promise<SystemAudioDevice[]> {
  return invoke<SystemAudioDevice[]>("list_audio_output_devices");
}

/** 切换系统默认播放设备，并让正在播放的 mpv 跟随。 */
export function setAudioOutputDevice(deviceId: string): Promise<void> {
  return invoke<void>("set_audio_output_device", { deviceId });
}

export interface SystemVolume {
  level: number;
  muted: boolean;
}

export function getSystemVolume(): Promise<SystemVolume> {
  return invoke<SystemVolume>("get_system_volume");
}

export function setSystemVolume(level: number): Promise<void> {
  return invoke<void>("set_system_volume", { level });
}

export function setSystemMuted(muted: boolean): Promise<void> {
  return invoke<void>("set_system_muted", { muted });
}

export function playerPreviewFrame(
  episodeId: number,
  positionMs: number,
): Promise<string> {
  return invoke<string>("player_preview_frame", { episodeId, positionMs });
}

export function showPlayerPreview(
  image: string,
  clientX: number,
  clientY: number,
): Promise<void> {
  return invoke<void>("show_player_preview", { image, clientX, clientY });
}

export function movePlayerPreview(
  clientX: number,
  clientY: number,
): Promise<void> {
  return invoke<void>("move_player_preview", { clientX, clientY });
}

export function hidePlayerPreview(): Promise<void> {
  return invoke<void>("hide_player_preview");
}

export function setPlayerCursorVisible(visible: boolean): Promise<void> {
  return invoke<void>("set_player_cursor_visible", { visible });
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

export function onPlayerMouseActivity(
  handler: () => void,
): Promise<UnlistenFn> {
  return listen("player-mouse-activity", () => handler());
}

export function onPlayerClick(handler: () => void): Promise<UnlistenFn> {
  return listen("player-click", () => handler());
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
