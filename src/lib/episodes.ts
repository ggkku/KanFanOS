import type { EpisodeSummary } from "../types/library";

const collator = new Intl.Collator("zh-CN", {
  numeric: true,
  sensitivity: "base",
});

/// 按文件名自然排序，例如第2集排在第10集前面。
export function sortEpisodes(rows: EpisodeSummary[]): EpisodeSummary[] {
  return [...rows].sort((a, b) => collator.compare(a.filename, b.filename));
}
