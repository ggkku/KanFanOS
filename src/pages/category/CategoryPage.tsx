import { Film, Loader2, RefreshCw } from "lucide-react";
import { EmptyState } from "../../components/common/EmptyState";
import { SelectMenu } from "../../components/common/SelectMenu";
import { SeriesCard } from "../../components/series/SeriesCard";
import { seriesCoverUrl } from "../../lib/thumbnails";
import type { SeriesSort, SeriesSummary } from "../../types/library";

const SORT_OPTIONS: { value: SeriesSort; label: string }[] = [
  { value: "name", label: "按名称" },
  { value: "episodes", label: "按集数" },
  { value: "updated", label: "按更新时间" },
];

interface CategoryPageProps {
  label: string;
  series: SeriesSummary[];
  total: number;
  loading: boolean;
  loadingMore: boolean;
  scanning: boolean;
  sort: SeriesSort;
  dataRoot: string | null;
  onSortChange: (sort: SeriesSort) => void;
  onOpenSeries: (series: SeriesSummary, coverRect: DOMRect | null) => void;
  onRescan: () => void;
  onLoadMore: () => void;
}

export function CategoryPage({
  label,
  series,
  total,
  loading,
  loadingMore,
  scanning,
  sort,
  dataRoot,
  onSortChange,
  onOpenSeries,
  onRescan,
  onLoadMore,
}: CategoryPageProps) {
  return (
    <div className="flex flex-1 flex-col gap-6">
      <header className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold">{label}</h1>
          <p className="mt-1 text-sm text-neutral-400">{total} 部番剧</p>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          <SelectMenu
            value={sort}
            options={SORT_OPTIONS}
            onChange={onSortChange}
            align="right"
            ariaLabel="排序方式"
          />

          <button
            type="button"
            onClick={onRescan}
            disabled={scanning}
            className="flex items-center gap-2 rounded-lg border border-neutral-800 bg-neutral-900 px-3 py-1.5 text-xs text-neutral-300 transition-colors hover:bg-neutral-800 disabled:opacity-60"
          >
            <RefreshCw size={13} className={scanning ? "animate-spin" : ""} />
            重新扫描
          </button>
        </div>
      </header>

      {loading ? (
        <div className="flex flex-1 items-center justify-center py-20 text-neutral-500">
          <Loader2 size={20} className="animate-spin" />
        </div>
      ) : series.length === 0 ? (
        <EmptyState
          icon={Film}
          title="这个档期还没有番剧"
          description="检查番剧目录结构后点击右上角「重新扫描」。视频文件需要放在番剧文件夹内。"
        />
      ) : (
        <>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-5">
            {series.map((item) => (
              <SeriesCard
                key={item.id}
                series={item}
                coverUrl={seriesCoverUrl(item, dataRoot)}
                onOpen={(rect) => onOpenSeries(item, rect)}
              />
            ))}
          </div>

          {series.length > 0 && series.length < total && (
            <div className="flex justify-center pt-2">
              <button
                type="button"
                onClick={onLoadMore}
                disabled={loadingMore}
                className="flex items-center gap-2 rounded-lg border border-neutral-800 bg-neutral-900 px-4 py-2 text-xs text-neutral-300 transition-colors hover:bg-neutral-800 disabled:opacity-60"
              >
                {loadingMore && (
                  <Loader2 size={13} className="animate-spin" />
                )}
                加载更多（{series.length} / {total}）
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
