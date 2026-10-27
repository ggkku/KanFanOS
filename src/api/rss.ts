import { invoke } from "@tauri-apps/api/core";
import type {
  ParsedReleaseTitle,
  RssDownloadRoot,
  RssEntry,
  RssFeed,
  RssFollow,
  RssMatch,
  RssRefreshResult,
} from "../types/rss";

export function getRssDownloadRoot(): Promise<RssDownloadRoot> {
  return invoke<RssDownloadRoot>("rss_download_root");
}

export function parseReleaseTitle(text: string): Promise<ParsedReleaseTitle> {
  return invoke<ParsedReleaseTitle>("rss_parse_release_title", { text });
}

export function listRssFeeds(): Promise<RssFeed[]> {
  return invoke<RssFeed[]>("rss_list_feeds");
}

export function listRssFollows(): Promise<RssFollow[]> {
  return invoke<RssFollow[]>("rss_list_follows");
}

export function listRssMatches(
  limit?: number,
  followIds?: number[],
): Promise<RssMatch[]> {
  return invoke<RssMatch[]>("rss_list_matches", { limit, followIds });
}

export function listRssEntries(
  feedId: number,
  limit?: number,
): Promise<RssEntry[]> {
  return invoke<RssEntry[]>("rss_list_entries", { feedId, limit });
}

export function addRssFeed(url: string): Promise<RssFeed> {
  return invoke<RssFeed>("rss_add_feed", { url });
}

export function refreshRssFeed(feedId: number): Promise<RssRefreshResult> {
  return invoke<RssRefreshResult>("rss_refresh_feed", { feedId });
}

export function refreshAllRss(): Promise<RssRefreshResult[]> {
  return invoke<RssRefreshResult[]>("rss_refresh_all");
}

export function deleteRssFeed(feedId: number): Promise<void> {
  return invoke<void>("rss_delete_feed", { feedId });
}

export function addRssFollow(input: {
  feedId: number;
  seriesId?: number | null;
  title: string;
  aliases: string;
  preferredGroups?: string;
  subtitlePackaging?: string;
  resolution?: string;
  savePath?: string | null;
  autoDownload: boolean;
}): Promise<RssFollow> {
  return invoke<RssFollow>("rss_add_follow", {
    feedId: input.feedId,
    seriesId: input.seriesId ?? null,
    title: input.title,
    aliases: input.aliases,
    preferredGroups: input.preferredGroups ?? "",
    subtitlePackaging: input.subtitlePackaging ?? "",
    resolution: input.resolution ?? "1080p",
    savePath: input.savePath ?? null,
    autoDownload: input.autoDownload,
  });
}

export function updateRssFollow(input: {
  followId: number;
  title: string;
  aliases: string;
  preferredGroups?: string;
  subtitlePackaging?: string;
  resolution?: string;
  savePath?: string | null;
  autoDownload: boolean;
  status?: string;
}): Promise<void> {
  return invoke<void>("rss_update_follow", {
    followId: input.followId,
    title: input.title,
    aliases: input.aliases,
    preferredGroups: input.preferredGroups ?? "",
    subtitlePackaging: input.subtitlePackaging ?? "",
    resolution: input.resolution ?? "1080p",
    savePath: input.savePath ?? null,
    autoDownload: input.autoDownload,
    status: input.status ?? "active",
  });
}

export function deleteRssFollow(followId: number): Promise<void> {
  return invoke<void>("rss_delete_follow", { followId });
}

export function downloadRssMatch(matchId: number): Promise<void> {
  return invoke<void>("rss_download_match", { matchId });
}
