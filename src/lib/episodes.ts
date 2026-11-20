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

/// 其他视频的类型顺序，同时用于播放排序和筛选按钮。
export const EXTRA_KINDS = [
  "OVA",
  "SP",
  "NCOP/NCED",
  "OP/ED",
  "PV/CM",
  "EXTRA",
] as const;

/// 没有识别出类型的其他视频。用独立字符串避免和「全部」的 null 混淆。
export const UNKNOWN_EXTRA_KIND = "__unknown__";

const EXTRA_KIND_RANK: Record<string, number> = Object.fromEntries(
  EXTRA_KINDS.map((kind, index) => [kind, index]),
);

const UNKNOWN_EXTRA_RANK = EXTRA_KINDS.length;

export function extraKindKey(kind: string | null): string {
  return kind ?? UNKNOWN_EXTRA_KIND;
}

function extraKindRank(kind: string | null): number {
  return EXTRA_KIND_RANK[extraKindKey(kind)] ?? UNKNOWN_EXTRA_RANK;
}

export function sortExtraEpisodes(rows: EpisodeSummary[]): EpisodeSummary[] {
  return [...rows].sort((a, b) => {
    const rank = extraKindRank(a.extraKind) - extraKindRank(b.extraKind);
    if (rank !== 0) return rank;
    return collator.compare(a.filename, b.filename);
  });
}
