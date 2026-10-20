import { Clock, Loader2, Play, RefreshCw } from "lucide-react";
import { EmptyState } from "../../components/common/EmptyState";
import { thumbnailUrl } from "../../lib/thumbnails";
import type { RecentItem } from "../../types/library";

interface RecentPageProps {
  items: RecentItem[];
  loading: boolean;
  dataRoot: string | null;
  onPlay: (item: RecentItem) => void;
  onRefresh: () => void;
}

function formatOpenedAt(value: number): string {
  const diff = Date.now() - value;
  if (diff < 60_000) return "刚刚";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`;
  return new Date(value).toLocaleDateString("zh-CN");
}

export function RecentPage({
  items,
  loading,
  dataRoot,
  onPlay,
  onRefresh,
}: RecentPageProps) {
  return (
    <div className="flex flex-1 flex-col gap-6">
      <header className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold">最近观看</h1>
          <p className="mt-1 text-sm text-neutral-400">
            按打开时间排序，点击继续播放。
          </p>
        </div>
        <button
          type="button"
          onClick={onRefresh}
          disabled={loading}
          className="flex shrink-0 items-center gap-2 rounded-lg border border-neutral-800 bg-neutral-900 px-3 py-1.5 text-xs text-neutral-300 transition-colors hover:bg-neutral-800 disabled:opacity-60"
        >
          <RefreshCw size={13} className={loading ? "animate-spin" : ""} />
          刷新
        </button>
      </header>

      {loading ? (
        <div className="flex flex-1 items-center justify-center py-20 text-neutral-500">
          <Loader2 size={20} className="animate-spin" />
        </div>
      ) : items.length === 0 ? (
        <EmptyState
          icon={Clock}
          title="暂无观看记录"
          description="在番剧详情里点击播放按钮，观看记录会自动出现在这里。"
        />
      ) : (
        <section className="overflow-hidden rounded-xl border border-neutral-800 bg-neutral-900/30">
          {items.map((item) => {
            const url = thumbnailUrl(dataRoot, item.thumbnailKey);
            const percent =
              item.durationMs != null && item.durationMs > 0
                ? Math.min(
                    100,
                    Math.round(
                      (item.lastPositionMs / item.durationMs) * 100,
                    ),
                  )
                : 0;

            return (
              <div
                key={item.videoId}
                className="flex items-center gap-4 border-b border-neutral-800/70 px-4 py-3 last:border-b-0"
              >
                <div className="h-14 w-24 shrink-0 overflow-hidden rounded bg-neutral-800">
                  {url ? (
                    <img
                      src={url}
                      alt=""
                      loading="lazy"
                      className="h-full w-full object-cover"
                    />
                  ) : (
                    <div className="flex h-full w-full items-center justify-center text-neutral-700">
                      <Clock size={16} />
                    </div>
                  )}
                </div>

                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm text-neutral-200">
                    {item.seriesName ?? item.filename}
                  </div>
                  <div className="truncate text-[11px] text-neutral-500">
                    {item.seriesName
                      ? `${item.category ?? ""} · ${item.filename}`
                      : item.filename}
                  </div>

                  {item.completed ? (
                    <div className="mt-1 text-[10px] text-emerald-500">
                      已看完
                    </div>
                  ) : (
                    percent > 0 && (
                      <div className="mt-1 flex items-center gap-2">
                        <div className="h-0.5 w-20 rounded bg-neutral-800">
                          <div
                            className="h-full rounded bg-indigo-500"
                            style={{ width: `${percent}%` }}
                          />
                        </div>
                        <span className="text-[10px] tabular-nums text-neutral-500">
                          {percent}%
                        </span>
                      </div>
                    )
                  )}
                </div>

                <div className="flex shrink-0 flex-col items-end gap-1.5">
                  <span className="text-[11px] text-neutral-500">
                    {formatOpenedAt(item.lastOpenedAt)}
                  </span>
                  <button
                    type="button"
                    onClick={() => onPlay(item)}
                    className="flex items-center gap-1.5 rounded-lg bg-neutral-800 px-2.5 py-1.5 text-[11px] text-neutral-200 transition-colors hover:bg-indigo-600 hover:text-white"
                  >
                    <Play size={11} fill="currentColor" />
                    {item.completed
                      ? "重新播放"
                      : item.lastPositionMs > 0
                        ? "继续播放"
                        : "播放"}
                  </button>
                </div>
              </div>
            );
          })}
        </section>
      )}
    </div>
  );
}
