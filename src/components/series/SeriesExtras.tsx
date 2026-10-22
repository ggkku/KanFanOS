import { ArrowLeft, Film, Play } from "lucide-react";
import { thumbnailUrl } from "../../lib/thumbnails";
import type { EpisodeSummary, SeriesSummary } from "../../types/library";

interface SeriesExtrasProps {
  series: SeriesSummary;
  episodes: EpisodeSummary[];
  dataRoot: string | null;
  coverUrl: string | null;
  onBack: () => void;
  onPlay: (episode: EpisodeSummary) => void;
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

        <div className="min-w-0">
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
        </div>
      </header>

      <section className="overflow-hidden rounded-xl border border-neutral-800 bg-neutral-900/30">
        {episodes.map((episode) => {
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
                <div className="truncate text-sm text-neutral-200">
                  {episode.filename}
                </div>
                {meta && (
                  <div className="mt-0.5 text-[11px] text-neutral-500">
                    {meta}
                  </div>
                )}
              </div>

              <button
                type="button"
                onClick={() => onPlay(episode)}
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
