import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { Check, Loader2, X } from "lucide-react";
import { listSeriesVideos } from "../../api/library";
import type { SeriesVideo } from "../../types/library";
import { errorMessage } from "../../lib/errors";

interface SeriesEpisodePickerProps {
  seriesId: number;
  /// 当前保存的选集路径（相对番剧目录）。
  selected: string[];
  onConfirm: (paths: string[]) => void;
  onClose: () => void;
}

function normalize(path: string): string {
  return path
    .replace(/\\/g, "/")
    .replace(/^\.\//, "")
    .trim()
    .toLowerCase();
}

/// 选集选择器：勾选的视频进入选集页，其余进入「其它视频」。
export function SeriesEpisodePicker({
  seriesId,
  selected,
  onConfirm,
  onClose,
}: SeriesEpisodePickerProps) {
  const [videos, setVideos] = useState<SeriesVideo[]>([]);
  const [checked, setChecked] = useState<Set<string>>(
    () => new Set(selected.map(normalize)),
  );
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;

    void listSeriesVideos(seriesId)
      .then((items) => {
        if (!disposed) setVideos(items);
      })
      .catch((reason) => {
        if (!disposed) setError(errorMessage(reason));
      })
      .finally(() => {
        if (!disposed) setLoading(false);
      });

    return () => {
      disposed = true;
    };
  }, [seriesId]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  const toggle = (relativePath: string) => {
    const key = normalize(relativePath);
    setChecked((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const setAll = (value: boolean) => {
    setChecked(
      value
        ? new Set(videos.map((video) => normalize(video.relativePath)))
        : new Set(),
    );
  };

  const confirm = () => {
    const paths = videos
      .filter((video) => checked.has(normalize(video.relativePath)))
      .map((video) => video.relativePath);
    onConfirm(paths);
  };

  return createPortal(
    <div
      className="fixed inset-0 z-[80] flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="选择选集视频"
        className="flex max-h-[82vh] w-full max-w-2xl flex-col rounded-2xl border border-neutral-800 bg-neutral-950 shadow-2xl"
      >
        <header className="flex shrink-0 items-center justify-between border-b border-neutral-800 px-5 py-3.5">
          <div>
            <h2 className="text-sm font-medium text-neutral-100">
              选择选集视频
            </h2>
            <p className="mt-0.5 text-[11px] text-neutral-500">
              勾选的视频进入选集页，其余进入「其它视频」；文件本身不会被移动。
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="关闭"
            className="flex h-7 w-7 items-center justify-center rounded-lg text-neutral-500 transition-colors hover:bg-neutral-800 hover:text-neutral-200"
          >
            <X size={15} />
          </button>
        </header>

        <div className="flex shrink-0 items-center justify-between border-b border-neutral-800/70 px-5 py-2 text-[11px] text-neutral-500">
          <span className="tabular-nums">
            共 {videos.length} 个视频，已选 {checked.size} 个
          </span>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => setAll(true)}
              className="rounded border border-neutral-700 px-2 py-0.5 text-neutral-300 transition-colors hover:bg-neutral-800"
            >
              全选
            </button>
            <button
              type="button"
              onClick={() => setAll(false)}
              className="rounded border border-neutral-700 px-2 py-0.5 text-neutral-300 transition-colors hover:bg-neutral-800"
            >
              全不选
            </button>
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-3">
          {loading ? (
            <div className="flex items-center justify-center gap-2 py-10 text-xs text-neutral-500">
              <Loader2 size={14} className="animate-spin" />
              正在读取视频…
            </div>
          ) : error ? (
            <p className="py-6 text-center text-xs text-rose-300">{error}</p>
          ) : videos.length === 0 ? (
            <p className="py-6 text-center text-xs text-neutral-600">
              这个番剧目录下没有扫描到视频。
            </p>
          ) : (
            <div className="flex flex-col gap-1">
              {videos.map((video) => {
                const isChecked = checked.has(normalize(video.relativePath));
                return (
                  <button
                    key={video.path}
                    type="button"
                    onClick={() => toggle(video.relativePath)}
                    className={`flex items-center gap-3 rounded-lg border px-3 py-2 text-left text-xs transition-colors ${
                      isChecked
                        ? "border-indigo-500/40 bg-indigo-500/10"
                        : "border-neutral-800 bg-neutral-900/40 hover:bg-neutral-900"
                    }`}
                  >
                    <span
                      className={`flex h-4 w-4 shrink-0 items-center justify-center rounded border ${
                        isChecked
                          ? "border-indigo-500 bg-indigo-600 text-white"
                          : "border-neutral-700"
                      }`}
                    >
                      {isChecked && <Check size={11} />}
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-neutral-200">
                        {video.filename}
                      </span>
                      {video.relativePath !== video.filename && (
                        <span className="block truncate text-[10px] text-neutral-500">
                          {video.relativePath}
                        </span>
                      )}
                    </span>
                    <span className="shrink-0 text-[10px] text-neutral-600">
                      {video.kind === "main" ? "选集" : "其它"}
                    </span>
                  </button>
                );
              })}
            </div>
          )}
        </div>

        <footer className="flex shrink-0 items-center justify-end gap-2 border-t border-neutral-800 px-5 py-3">
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg border border-neutral-800 bg-neutral-900 px-3.5 py-1.5 text-xs text-neutral-300 transition-colors hover:bg-neutral-800"
          >
            取消
          </button>
          <button
            type="button"
            onClick={confirm}
            disabled={loading}
            className="rounded-lg bg-indigo-600 px-3.5 py-1.5 text-xs font-medium text-white transition-colors hover:bg-indigo-500 disabled:opacity-40"
          >
            确定
          </button>
        </footer>
      </div>
    </div>,
    document.body,
  );
}
