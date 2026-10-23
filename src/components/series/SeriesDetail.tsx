import { useEffect, useState } from "react";
import {
  ArrowLeft,
  ChevronLeft,
  ChevronRight,
  Clapperboard,
  Film,
  Play,
  Settings2,
} from "lucide-react";
import {
  getSeriesConfig,
  setSeriesAwards,
  setSeriesRating,
  setSeriesWatchCount,
} from "../../api/library";
import { errorMessage } from "../../lib/errors";
import { thumbnailUrl } from "../../lib/thumbnails";
import type {
  EpisodeSummary,
  SeriesConfigView,
  SeriesSummary,
} from "../../types/library";
import { SeriesAwards } from "./SeriesAwards";
import { SeriesConfigDialog } from "./SeriesConfigDialog";
import { SeriesRating } from "./SeriesRating";

interface SeriesDetailProps {
  series: SeriesSummary;
  episodes: EpisodeSummary[];
  dataRoot: string | null;
  coverUrl: string | null;
  extraCount: number;
  onBack: () => void;
  onPlay: (episode: EpisodeSummary) => void;
  onOpenExtras: () => void;
  onNavigateSeries: (seriesId: number) => void;
  onChanged: () => Promise<void>;
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
  extraCount,
  onBack,
  onPlay,
  onOpenExtras,
  onNavigateSeries,
  onChanged,
}: SeriesDetailProps) {
  const [busy, setBusy] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [config, setConfig] = useState<SeriesConfigView | null>(null);
  const [configOpen, setConfigOpen] = useState(false);
  const [rating, setRating] = useState<number | null>(series.rating);
  const [ratingBusy, setRatingBusy] = useState(false);
  const [awards, setAwards] = useState<string[]>([]);
  const [awardsBusy, setAwardsBusy] = useState(false);
  const [awardSignal, setAwardSignal] = useState(0);

  // 切换番剧或外部刷新后同步评分。
  useEffect(() => {
    setRating(series.rating);
  }, [series.id, series.rating]);

  // 配置里的上一季 / 下一季决定工具栏按钮是否出现。
  useEffect(() => {
    let disposed = false;
    setConfig(null);
    setAwards([]);

    getSeriesConfig(series.id)
      .then((view) => {
        if (disposed) return;
        setConfig(view);
        setRating(view.rating);
        setAwards(view.awards);
      })
      .catch((err: unknown) => {
        if (!disposed) setActionError(errorMessage(err));
      });

    return () => {
      disposed = true;
    };
  }, [series.id]);

  const runAction = async (name: string, action: () => Promise<unknown>) => {
    if (busy) return;
    setBusy(name);
    setActionError(null);
    try {
      await action();
      await onChanged();
    } catch (err) {
      setActionError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  };

  const actionButton =
    "rounded-lg border border-neutral-800 bg-neutral-900 px-2.5 py-1.5 text-[11px] text-neutral-300 transition-colors hover:bg-neutral-800 disabled:cursor-not-allowed disabled:opacity-40";

  const changeRating = async (next: number | null) => {
    if (ratingBusy) return;

    setRatingBusy(true);
    setActionError(null);
    try {
      const saved = await setSeriesRating(series.id, next);
      setRating(saved);
      await onChanged();
    } catch (err) {
      setActionError(errorMessage(err));
    } finally {
      setRatingBusy(false);
    }
  };

  const changeAwards = async (next: string[]) => {
    if (awardsBusy) return;

    setAwardsBusy(true);
    setActionError(null);
    try {
      const saved = await setSeriesAwards(series.id, next);
      setAwards(saved);
    } catch (err) {
      setActionError(errorMessage(err));
    } finally {
      setAwardsBusy(false);
    }
  };

  const addAward = (name: string) => {
    const trimmed = name.trim();
    if (!trimmed || awards.includes(trimmed)) return;
    void changeAwards([...awards, trimmed]);
  };

  // 配置弹窗里删除奖项；失败时让弹窗显示错误。
  const removeAward = async (name: string) => {
    const saved = await setSeriesAwards(
      series.id,
      awards.filter((award) => award !== name),
    );
    setAwards(saved);
  };

  return (
    <div className="relative flex flex-1 flex-col gap-6">
      <button
        type="button"
        onClick={onBack}
        className="flex w-fit items-center gap-1.5 text-xs text-neutral-400 transition-colors hover:text-neutral-200"
      >
        <ArrowLeft size={14} />
        返回
      </button>

      {/* 右上角；绝对定位，不占用返回行的高度，避免下移封面。 */}
      <button
        type="button"
        onClick={() => setConfigOpen(true)}
        className={`${actionButton} absolute right-0 top-0 flex items-center gap-1.5`}
      >
        <Settings2 size={12} />
        配置
      </button>

      <header className="flex items-start gap-5">
        {coverUrl ? (
          <img
            data-series-cover
            src={coverUrl}
            alt=""
            className="h-40 w-30 shrink-0 rounded-lg border border-neutral-800 object-cover"
          />
        ) : (
          <div
            data-series-cover
            className="flex h-40 w-30 shrink-0 items-center justify-center rounded-lg border border-neutral-800 bg-gradient-to-br from-neutral-800 via-neutral-900 to-neutral-950"
          >
            <span className="text-4xl font-semibold text-neutral-700">
              {series.name.slice(0, 1)}
            </span>
          </div>
        )}

        <div className="min-w-0 flex-1">
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

          <div className="mt-3 flex flex-wrap items-center gap-2">
            {config?.previous && (
              <button
                type="button"
                onClick={() => onNavigateSeries(config.previous!.id)}
                title={`上一季：${config.previous.name}`}
                className={`${actionButton} flex items-center gap-1`}
              >
                <ChevronLeft size={12} />
                上一季
              </button>
            )}

            {config?.next && (
              <button
                type="button"
                onClick={() => onNavigateSeries(config.next!.id)}
                title={`下一季：${config.next.name}`}
                className={`${actionButton} flex items-center gap-1`}
              >
                下一季
                <ChevronRight size={12} />
              </button>
            )}

            <div className="flex items-center gap-1.5 rounded-lg border border-neutral-800 bg-neutral-900 px-2.5 py-1.5 text-[11px] text-neutral-400">
              <span>看过</span>
              <button
                type="button"
                onClick={() =>
                  void runAction("count", () =>
                    setSeriesWatchCount(
                      series.id,
                      Math.max(0, series.watchCount - 1),
                    ),
                  )
                }
                disabled={busy != null || series.watchCount <= 0}
                className="flex h-5 w-5 items-center justify-center rounded text-neutral-300 transition-colors hover:bg-neutral-800 disabled:cursor-not-allowed disabled:opacity-30"
                title="减少一遍"
              >
                −
              </button>
              <span className="min-w-4 text-center tabular-nums text-neutral-200">
                {series.watchCount}
              </span>
              <button
                type="button"
                onClick={() =>
                  void runAction("count", () =>
                    setSeriesWatchCount(
                      series.id,
                      series.watchCount + 1,
                    ),
                  )
                }
                disabled={busy != null}
                className="flex h-5 w-5 items-center justify-center rounded text-neutral-300 transition-colors hover:bg-neutral-800 disabled:cursor-not-allowed disabled:opacity-30"
                title="增加一遍"
              >
                +
              </button>
              <span>遍</span>
            </div>

            {extraCount > 0 && (
              <button
                type="button"
                onClick={onOpenExtras}
                className="flex items-center gap-1.5 rounded-lg border border-indigo-900/60 bg-indigo-950/30 px-2.5 py-1.5 text-[11px] text-indigo-300 transition-colors hover:bg-indigo-950/60"
              >
                <Clapperboard size={12} />
                其他视频 · {extraCount}
              </button>
            )}

          </div>

          {actionError && (
            <p className="mt-2 text-[11px] text-red-400">{actionError}</p>
          )}
        </div>

        {/* 评分和奖项固定在右侧，不占用封面 / 选集列表的纵向空间。 */}
        <div className="flex w-56 shrink-0 flex-col items-end gap-2">
          <SeriesRating
            value={rating}
            disabled={ratingBusy}
            onChange={(next) => void changeRating(next)}
          />
          <SeriesAwards
            awards={awards}
            busy={awardsBusy}
            openSignal={awardSignal}
            onAdd={addAward}
          />
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

      {configOpen && config && (
        <SeriesConfigDialog
          series={series}
          config={config}
          onClose={() => setConfigOpen(false)}
          onSaved={(updated) => {
            setConfig(updated);
            setRating(updated.rating);
            setAwards(updated.awards);
            setConfigOpen(false);
          }}
          onAddAward={() => {
            setConfigOpen(false);
            setAwardSignal((value) => value + 1);
          }}
          awards={awards}
          onRemoveAward={removeAward}
          onChanged={onChanged}
        />
      )}
    </div>
  );
}
