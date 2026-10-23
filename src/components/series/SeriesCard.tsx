import { useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { Trophy } from "lucide-react";
import { getSeriesAwards } from "../../api/library";
import type { SeriesSummary } from "../../types/library";
import {
  RATING_LABELS,
  ratingColor,
  ratingGlowColor,
} from "./SeriesRating";

interface SeriesCardProps {
  series: SeriesSummary;
  coverUrl: string | null;
  subtitle?: string | null;
  /// 打开时把封面的屏幕位置传出去，用于「封面飞入」过渡。
  onOpen: (coverRect: DOMRect | null) => void;
}

export function SeriesCard({
  series,
  coverUrl,
  subtitle,
  onOpen,
}: SeriesCardProps) {
  const [imageFailed, setImageFailed] = useState(false);
  const [awards, setAwards] = useState<string[]>([]);
  const [awardsLoaded, setAwardsLoaded] = useState(false);
  const coverRef = useRef<HTMLDivElement>(null);
  const showImage = coverUrl != null && !imageFailed;

  // 奖项只在鼠标移入时按需读取，避免列表一次性读取所有番剧的配置。
  useEffect(() => {
    setAwards([]);
    setAwardsLoaded(false);
  }, [series.id]);

  const loadAwards = () => {
    if (awardsLoaded) return;

    setAwardsLoaded(true);
    getSeriesAwards(series.id)
      .then(setAwards)
      .catch(() => setAwards([]));
  };

  const hasMeta =
    series.rating != null || series.watchCount > 0 || awards.length > 0;
  // 悬停外发光的颜色跟随评分；没有评分就不发光。
  const glow =
    series.rating != null ? ratingGlowColor(series.rating) : null;

  return (
    <button
      type="button"
      onMouseEnter={loadAwards}
      onFocus={loadAwards}
      onClick={() =>
        onOpen(coverRef.current?.getBoundingClientRect() ?? null)
      }
      className="series-card group relative flex min-w-0 flex-col text-left hover:z-10"
    >
      <div
        ref={coverRef}
        style={
          glow
            ? ({ "--card-glow": glow } as CSSProperties)
            : undefined
        }
        className={`relative aspect-[3/4] overflow-hidden rounded-xl border border-neutral-800 bg-gradient-to-br from-neutral-800 via-neutral-900 to-neutral-950 transition-shadow duration-300 ${
          glow
            ? "group-hover:shadow-[0_0_4px_1px_var(--card-glow),0_0_10px_2px_var(--card-glow),0_0_24px_4px_var(--card-glow)]"
            : ""
        }`}
      >
        {showImage ? (
          <img
            src={coverUrl}
            alt=""
            loading="lazy"
            onError={() => setImageFailed(true)}
            className="h-full w-full object-cover transition-transform duration-300 group-hover:scale-[1.03]"
          />
        ) : (
          <div className="flex h-full w-full items-center justify-center">
            <span className="text-4xl font-semibold text-neutral-700 transition-colors group-hover:text-neutral-600">
              {series.name.slice(0, 1)}
            </span>
          </div>
        )}

        <span className="absolute right-2 top-2 rounded-md bg-black/60 px-1.5 py-0.5 text-[10px] tabular-nums text-neutral-200 backdrop-blur">
          {series.episodeCount} 集
        </span>

        {hasMeta && (
          <div className="pointer-events-none absolute inset-x-0 bottom-0 translate-y-1.5 bg-gradient-to-t from-black/95 via-black/70 to-transparent px-2.5 pb-2 pt-10 opacity-0 transition-all duration-200 group-hover:translate-y-0 group-hover:opacity-100 group-focus-visible:translate-y-0 group-focus-visible:opacity-100">
            <div className="flex flex-wrap items-center gap-1.5">
              {series.rating != null && (
                <span className="inline-flex items-baseline gap-1 rounded-md bg-white/10 px-1.5 py-0.5 text-[10px] text-neutral-200 backdrop-blur">
                  <span
                    className={`text-sm font-semibold tabular-nums ${ratingColor(series.rating)}`}
                  >
                    {series.rating}
                  </span>
                  {RATING_LABELS[series.rating]}
                </span>
              )}

              {series.watchCount > 0 && (
                <span className="rounded-md bg-white/10 px-1.5 py-0.5 text-[10px] text-neutral-200 backdrop-blur">
                  看过 {series.watchCount} 遍
                </span>
              )}
            </div>

            {awards.length > 0 && (
              <div className="mt-1 space-y-0.5">
                {awards.slice(0, 2).map((award) => (
                  <div
                    key={award}
                    className="flex items-center gap-1 text-[10px] text-amber-300"
                  >
                    <Trophy size={9} className="shrink-0" />
                    <span className="truncate">{award}</span>
                  </div>
                ))}
                {awards.length > 2 && (
                  <div className="text-[9px] text-neutral-400">
                    +{awards.length - 2} 个奖项
                  </div>
                )}
              </div>
            )}
          </div>
        )}
      </div>

      <div className="mt-2 line-clamp-2 text-sm leading-snug text-neutral-200 transition-colors group-hover:text-white">
        {series.name}
      </div>

      {subtitle && (
        <div className="mt-0.5 truncate text-[11px] text-neutral-500">
          {subtitle}
        </div>
      )}
    </button>
  );
}
