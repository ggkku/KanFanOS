import { Loader2, Search } from "lucide-react";
import { EmptyState } from "../../components/common/EmptyState";
import { SeriesCard } from "../../components/series/SeriesCard";
import { seriesCoverUrl } from "../../lib/thumbnails";
import type { SearchItem } from "../../types/library";

interface SearchPageProps {
  query: string;
  items: SearchItem[];
  total: number;
  loading: boolean;
  loadingMore: boolean;
  onOpenSeries: (item: SearchItem, coverRect: DOMRect | null) => void;
  onLoadMore: () => void;
}

export function SearchPage({
  query,
  items,
  total,
  loading,
  loadingMore,
  onOpenSeries,
  onLoadMore,
}: SearchPageProps) {
  return (
    <div className="flex flex-1 flex-col gap-6">
      <header>
        <h1 className="text-2xl font-semibold">搜索结果</h1>
        <p className="mt-1 text-sm text-neutral-400">
          “{query}” · {total} 部番剧
        </p>
      </header>

      {loading && items.length === 0 ? (
        <div className="flex flex-1 items-center justify-center py-20 text-neutral-500">
          <Loader2 size={20} className="animate-spin" />
        </div>
      ) : total === 0 ? (
        <EmptyState
          icon={Search}
          title="没有找到匹配内容"
          description="可以尝试番剧名的一部分，或者剧集文件名中的关键词。"
        />
      ) : (
        <>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-5">
            {items.map((item) => (
              <SeriesCard
                key={item.id}
                series={item}
                coverUrl={seriesCoverUrl(item)}
                subtitle={
                  item.matchPreview
                    ? `${item.category} · ${item.matchPreview}`
                    : item.category
                }
                onOpen={(rect) => onOpenSeries(item, rect)}
              />
            ))}
          </div>

          {items.length < total && (
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
                加载更多（{items.length} / {total}）
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
