import { useRef, useState } from "react";
import type { SeriesSummary } from "../../types/library";

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
  const coverRef = useRef<HTMLDivElement>(null);
  const showImage = coverUrl != null && !imageFailed;

  return (
    <button
      type="button"
      onClick={() =>
        onOpen(coverRef.current?.getBoundingClientRect() ?? null)
      }
      className="group flex min-w-0 flex-col text-left"
    >
      <div
        ref={coverRef}
        className="relative aspect-[3/4] overflow-hidden rounded-xl border border-neutral-800 bg-gradient-to-br from-neutral-800 via-neutral-900 to-neutral-950 transition-colors group-hover:border-indigo-500/50"
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
