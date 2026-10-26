import { Loader2, X } from "lucide-react";
import type { ScanProgress } from "../../types/library";

interface ScanBannerProps {
  progress: ScanProgress;
  onCancel: () => void;
}

export function ScanBanner({ progress, onCancel }: ScanBannerProps) {
  return (
    <div className="rounded-lg border border-indigo-900/60 bg-indigo-950/95 px-4 py-3 shadow-2xl backdrop-blur-md">
      <div className="flex items-center justify-between gap-4">
        <div className="flex min-w-0 items-center gap-2 text-xs text-indigo-300">
          <Loader2 size={13} className="shrink-0 animate-spin" />
          <span className="shrink-0">正在扫描</span>
          <span className="shrink-0 tabular-nums text-indigo-400/80">
            {progress.seriesFound} 部番剧 · {progress.filesFound} 个视频
          </span>
          {progress.errors > 0 && (
            <span className="shrink-0 text-amber-400/80">
              · {progress.errors} 个错误
            </span>
          )}
          <span className="shrink-0 tabular-nums text-indigo-400/60">
            {(progress.elapsedMs / 1000).toFixed(1)}s
          </span>
        </div>
        <button
          type="button"
          onClick={onCancel}
          className="flex shrink-0 items-center gap-1 rounded-md border border-indigo-900/60 px-2 py-1 text-[11px] text-indigo-300 transition-colors hover:bg-indigo-900/40"
        >
          <X size={11} />
          取消
        </button>
      </div>

      <div className="mt-2 h-0.5 overflow-hidden rounded-full bg-indigo-950">
        <div className="h-full w-1/2 animate-pulse rounded-full bg-indigo-500" />
      </div>

      {progress.currentPath && (
        <div className="mt-2 truncate font-mono text-[10px] text-indigo-400/50">
          {progress.currentPath}
        </div>
      )}
    </div>
  );
}
