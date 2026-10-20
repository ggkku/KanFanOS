import { ArrowLeft, Film, Play } from "lucide-react";
import { thumbnailUrl } from "../../lib/thumbnails";
import type { EpisodeSummary, SeriesSummary } from "../../types/library";

interface SeriesDetailProps {
  series: SeriesSummary;
  episodes: EpisodeSummary[];
  dataRoot: string | null;
  coverUrl: string | null;
  onBack: () => void;
  onPlay: (episode: EpisodeSummary) => void;
}

function formatBytes(bytes: number): string {
  if (bytes <= 0) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(
    Math.floor(Math.log(bytes) / Math.log(1024)),
    units.length - 1,
  );
  return `${(bytes / 1024 ** index).toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
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

export function SeriesDetail({
  series,
  episodes,
  dataRoot,
  coverUrl,
  onBack,
  onPlay,
}: SeriesDetailProps) {
  return (
    <div className="flex flex-1 flex-col gap-6">
      <button
        type="button"
        onClick={onBack}
        className="flex w-fit items-center gap-1.5 text-xs text-neutral-400 transition-colors hover:text-neutral-200"
      >
        <ArrowLeft size={14} />
        返回
      </button>

      <header className="flex items-start gap-5">
        {coverUrl && (
          <img
            src={coverUrl}
            alt=""
            className="h-40 w-28 shrink-0 rounded-lg border border-neutral-800 object-cover"
          />
        )}

        <div className="min-w-0">
          <h1 className="text-2xl font-semibold">{series.name}</h1>
          <p className="mt-1 text-sm text-neutral-400">
            {series.category} · {episodes.length} 集
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
        {episodes.length === 0 ? (
          <div className="px-5 py-8 text-center text-sm text-neutral-500">
            没有可显示的视频文件
          </div>
        ) : (
          episodes.map((episode, index) => {
            const url = thumbnailUrl(dataRoot, episode.thumbnailKey);
            const meta = [
              formatDuration(episode.durationMs),
              episode.width != null && episode.height != null
                ? `${episode.width}×${episode.height}`
                : "",
            ]
              .filter(Boolean)
              .join(" · ");

            const progress =
              episode.durationMs != null && episode.durationMs > 0
                ? Math.min(
                    100,
                    Math.round(
                      ((episode.lastPositionMs ?? 0) / episode.durationMs) *
                        100,
                    ),
                  )
                : 0;

            return (
              <div
                key={episode.id}
                className="flex items-center gap-4 border-b border-neutral-800/70 px-4 py-3 last:border-b-0"
              >
                <span className="w-8 shrink-0 text-right text-xs tabular-nums text-neutral-600">
                  {index + 1}
                </span>

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

                  {episode.completed ? (
                    <div className="mt-1 text-[10px] text-emerald-500">
                      已看完
                    </div>
                  ) : (
                    progress > 0 && (
                      <div className="mt-1 flex items-center gap-2">
                        <div className="h-0.5 w-20 rounded bg-neutral-800">
                          <div
                            className="h-full rounded bg-indigo-500"
                            style={{ width: `${progress}%` }}
                          />
                        </div>
                        <span className="text-[10px] tabular-nums text-neutral-500">
                          已看 {progress}%
                        </span>
                      </div>
                    )
                  )}
                </div>

                <span className="shrink-0 text-[11px] tabular-nums text-neutral-600">
                  {formatBytes(episode.fileSize)}
                </span>

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
          })
        )}
      </section>

      <p className="text-xs text-neutral-600">
        播放功能将在后续步骤接入；当前列表中的信息来自本地索引与 FFprobe。
      </p>
    </div>
  );
}
