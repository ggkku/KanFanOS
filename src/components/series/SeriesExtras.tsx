import { useEffect, useMemo, useState } from "react";
import { ArrowLeft, Film, Play } from "lucide-react";
import {
  EXTRA_KINDS,
  UNKNOWN_EXTRA_KIND,
  extraKindKey,
} from "../../lib/episodes";
import { thumbnailUrl } from "../../lib/thumbnails";
import type { EpisodeSummary, SeriesSummary } from "../../types/library";

interface SeriesExtrasProps {
  series: SeriesSummary;
  episodes: EpisodeSummary[];
  dataRoot: string | null;
  coverUrl: string | null;
  onBack: () => void;
  /// 播放时传入当前筛选后的列表，保证上一集/下一集不会跳到被筛掉的视频。
  onPlay: (episode: EpisodeSummary, episodes: EpisodeSummary[]) => void;
}

interface FilterButtonProps {
  active: boolean;
  label: string;
  count: number;
  onClick: () => void;
}

function FilterButton({ active, label, count, onClick }: FilterButtonProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      className={`flex items-center gap-1.5 rounded-lg border px-2.5 py-1 text-[11px] transition-colors ${
        active
          ? "border-indigo-500/70 bg-indigo-500/15 text-indigo-200"
          : "border-neutral-800 bg-neutral-900/40 text-neutral-400 hover:border-neutral-700 hover:text-neutral-200"
      }`}
    >
      {label}
      <span
        className={`rounded px-1 text-[10px] ${
          active
            ? "bg-indigo-500/20 text-indigo-200"
            : "bg-neutral-800 text-neutral-500"
        }`}
      >
        {count}
      </span>
    </button>
  );
}

function formatDuration(ms: number | null): string {
  if (ms == null || ms <= 0) return "";
  const total = Math.round(ms / 1000);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
  }
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

/// 番剧子目录里的 CM/PV/特典等。故意不显示左侧集数序号。
export function SeriesExtras({
  series,
  episodes,
  dataRoot,
  coverUrl,
  onBack,
  onPlay,
}: SeriesExtrasProps) {
  const [selectedKind, setSelectedKind] = useState<string | null>(null);

  const groups = useMemo(() => {
    const counts = new Map<string, number>();
    for (const episode of episodes) {
      const key = extraKindKey(episode.extraKind);
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }

    const known: { key: string; label: string; count: number }[] = EXTRA_KINDS
      .filter((kind) => counts.has(kind))
      .map((kind) => ({
        key: kind as string,
        label: kind as string,
        count: counts.get(kind) ?? 0,
      }));

    if (counts.has(UNKNOWN_EXTRA_KIND)) {
      known.push({
        key: UNKNOWN_EXTRA_KIND,
        label: "未分类",
        count: counts.get(UNKNOWN_EXTRA_KIND) ?? 0,
      });
    }
    return known;
  }, [episodes]);

  // 换番剧时回到「全部」；当前筛选类型消失时也自动回退。
  useEffect(() => {
    setSelectedKind(null);
  }, [series.id]);

  useEffect(() => {
    if (
      selectedKind != null &&
      !groups.some((group) => group.key === selectedKind)
    ) {
      setSelectedKind(null);
    }
  }, [groups, selectedKind]);

  const visibleEpisodes = useMemo(() => {
    if (selectedKind == null) return episodes;
    return episodes.filter(
      (episode) => extraKindKey(episode.extraKind) === selectedKind,
    );
  }, [episodes, selectedKind]);

  return (
    <div className="flex flex-1 flex-col gap-6">
      <button
        type="button"
        onClick={onBack}
        className="flex w-fit items-center gap-1.5 text-xs text-neutral-400 transition-colors hover:text-neutral-200"
      >
        <ArrowLeft size={14} />
        返回选集
      </button>

      <header className="flex items-start gap-5">
        {coverUrl ? (
          <img
            src={coverUrl}
            alt=""
            className="h-40 w-30 shrink-0 rounded-lg border border-neutral-800 object-cover"
          />
        ) : (
          <div className="flex h-40 w-30 shrink-0 items-center justify-center overflow-hidden rounded-lg border border-neutral-800 bg-gradient-to-br from-neutral-800 via-neutral-900 to-neutral-950">
            <span className="text-4xl font-semibold text-neutral-700">
              {series.name.slice(0, 1)}
            </span>
          </div>
        )}

        <div className="min-w-0 flex-1">
          <h1 className="text-2xl font-semibold">{series.name}</h1>
          <p className="mt-1 text-sm text-neutral-400">
            其他视频 · {episodes.length} 个
          </p>
          <p
            data-selectable
            className="mt-1 break-all font-mono text-[11px] text-neutral-600"
          >
            {series.path}
          </p>

          {groups.length > 1 && (
            <div
              role="group"
              aria-label="按类型筛选其他视频"
              className="mt-3 flex flex-wrap items-center gap-2"
            >
              <FilterButton
                active={selectedKind == null}
                label="全部"
                count={episodes.length}
                onClick={() => setSelectedKind(null)}
              />
              {groups.map((group) => (
                <FilterButton
                  key={group.key}
                  active={selectedKind === group.key}
                  label={group.label}
                  count={group.count}
                  onClick={() => setSelectedKind(group.key)}
                />
              ))}
            </div>
          )}
        </div>
      </header>

      <section className="overflow-hidden rounded-xl border border-neutral-800 bg-neutral-900/30">
        {visibleEpisodes.length === 0 && (
          <div className="px-4 py-6 text-center text-xs text-neutral-500">
            这个分类下没有视频
          </div>
        )}
        {visibleEpisodes.map((episode) => {
          const url = thumbnailUrl(dataRoot, episode.thumbnailKey);
          const meta = formatDuration(episode.durationMs);

          return (
            <div
              key={episode.id}
              className="flex items-center gap-4 border-b border-neutral-800/70 px-4 py-3 last:border-b-0"
            >
              <div className="h-10 w-16 shrink-0 overflow-hidden rounded bg-neutral-800">
                {url ? (
                  <img
                    src={url}
                    alt=""
                    loading="lazy"
                    className="h-full w-full object-cover"
                  />
                ) : (
                  <div className="flex h-full w-full items-center justify-center text-neutral-700">
                    <Film size={14} />
                  </div>
                )}
              </div>

              <div className="min-w-0 flex-1">
                <div className="flex min-w-0 items-center gap-2">
                  <span className="truncate text-sm text-neutral-200">
                    {episode.filename}
                  </span>
                  {episode.extraKind && (
                    <span className="shrink-0 whitespace-nowrap rounded border border-indigo-900/60 bg-indigo-950/30 px-1.5 py-0.5 text-[10px] font-medium text-indigo-300">
                      {episode.extraKind}
                    </span>
                  )}
                </div>
                {meta && (
                  <div className="mt-0.5 text-[11px] text-neutral-500">
                    {meta}
                  </div>
                )}
              </div>

              <button
                type="button"
                onClick={() => onPlay(episode, visibleEpisodes)}
                title="用 MPV 播放"
                className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-neutral-800 text-neutral-300 transition-colors hover:bg-indigo-600 hover:text-white"
              >
                <Play size={13} fill="currentColor" />
              </button>
            </div>
          );
        })}
      </section>
    </div>
  );
}
