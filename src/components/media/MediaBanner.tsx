import { Loader2, X } from "lucide-react";
import type { MediaProgress } from "../../types/library";

interface MediaBannerProps {
  progress: MediaProgress;
  onCancel: () => void;
}

export function MediaBanner({ progress, onCancel }: MediaBannerProps) {
  const percent =
    progress.total > 0
      ? Math.min(100, (progress.processed / progress.total) * 100)
      : 0;

  return (
    <div className="rounded-lg border border-sky-900/60 bg-sky-950/30 px-4 py-3">
      <div className="flex items-center justify-between gap-4">
        <div className="flex min-w-0 items-center gap-2 text-xs text-sky-300">
          <Loader2 size={13} className="shrink-0 animate-spin" />
          <span className="shrink-0">正在提取元数据与缩略图</span>
          <span className="shrink-0 tabular-nums text-sky-400/80">
            {progress.processed.toLocaleString()} / {progress.total.toLocaleString()}
          </span>
          {progress.failed > 0 && (
            <span className="shrink-0 text-amber-400/80">
              · {progress.failed} 个失败
            </span>
          )}
        </div>
        <button
          type="button"
          onClick={onCancel}
          className="flex shrink-0 items-center gap-1 rounded-md border border-sky-900/60 px-2 py-1 text-[11px] text-sky-300 transition-colors hover:bg-sky-900/40"
        >
          <X size={11} />
          取消
        </button>
      </div>

      <div className="mt-2 h-1 overflow-hidden rounded-full bg-sky-950">
        <div
          className="h-full rounded-full bg-sky-500 transition-[width] duration-300"
          style={{ width: `${percent}%` }}
        />
      </div>
    </div>
  );
}
