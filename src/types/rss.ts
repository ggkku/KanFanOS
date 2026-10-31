export interface RssFeed {
  id: number;
  url: string;
  title: string;
  site: string;
  lastFetchAt: number | null;
  lastError: string | null;
  createdAt: number;
}

export interface RssFollow {
  id: number;
  feedId: number;
  feedTitle: string;
  seriesId: number | null;
  title: string;
  aliases: string[];
  preferredGroups: string[];
  subtitlePackaging: string;
  resolution: string;
  savePath: string | null;
  savePathExists: boolean;
  autoDownload: boolean;
  lastEpisode: number | null;
  status: string;
  createdAt: number;
}

export interface RssEntry {
  id: number;
  feedId: number;
  title: string;
  link: string;
  downloadUrl: string;
  publishedAt: number;
  firstSeenAt: number;
}

export interface RssMatch {
  id: number;
  entryId: number;
  followId: number;
  followTitle: string;
  entryTitle: string;
  releaseGroup: string | null;
  subtitlePackaging: string | null;
  link: string;
  downloadUrl: string;
  publishedAt: number;
  episode: number | null;
  confidence: number;
  status: string;
  autoDownloaded: boolean;
  error: string | null;
  createdAt: number;
}

export interface RssRefreshResult {
  feedId: number;
  addedEntries: number;
  addedMatches: number;
  downloaded: number;
}

export interface RssDownloadRoot {
  path: string;
  exists: boolean;
}

export interface ParsedReleaseTitle {
  raw: string;
  group: string | null;
  title: string | null;
  titleCandidates: string[];
  episode: number | null;
  resolution: string | null;
  source: string | null;
  subtitle: string | null;
  subtitlePackaging: string | null;
  version: number | null;
  codec: string | null;
  container: string | null;
  confidence: number;
}
