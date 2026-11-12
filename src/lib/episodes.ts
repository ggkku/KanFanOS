import type { EpisodeSummary } from "../types/library";

const collator = new Intl.Collator("zh-CN", {
  numeric: true,
  sensitivity: "base",
});

/// 优先按文件名解析出的集数排序（同一集按版本 v1、v2…）；
/// 解析不到集数的文件排在后面，仍按文件名自然排序。
export function sortEpisodes(rows: EpisodeSummary[]): EpisodeSummary[] {
  return [...rows].sort((a, b) => {
    const aEpisode = a.episodeNumber;
    const bEpisode = b.episodeNumber;
    if (aEpisode != null && bEpisode != null) {
      if (aEpisode !== bEpisode) return aEpisode - bEpisode;
      if (a.episodeVersion !== b.episodeVersion) {
        return a.episodeVersion - b.episodeVersion;
      }
    } else if (aEpisode != null) {
      return -1;
    } else if (bEpisode != null) {
      return 1;
    }
    return collator.compare(a.filename, b.filename);
  });
}
