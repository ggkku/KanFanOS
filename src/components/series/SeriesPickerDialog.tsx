import { useEffect, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { Check, Loader2, Search, X } from "lucide-react";
import { searchSeriesOptions } from "../../api/library";
import { errorMessage } from "../../lib/errors";
import type { SeriesOption } from "../../types/library";

interface SeriesPickerDialogProps {
  title: string;
  libraryId: number;
  /// 当前番剧自己不能作为季链接目标。
  excludeId: number;
  selectedId: number | null;
  /// 打开时预填的搜索词，通常是当前番剧名的前两个字。
  initialQuery?: string;
  onSelect: (option: SeriesOption | null) => void;
  onClose: () => void;
}

/// 单次最多返回的候选数。番剧可能上千部，靠关键词缩小范围。
const LIMIT = 200;
const DEBOUNCE_MS = 180;

/// 搜索式番剧选择器：点季链接字段后弹出。
export function SeriesPickerDialog({
  title,
  libraryId,
  excludeId,
  selectedId,
  initialQuery = "",
  onSelect,
  onClose,
}: SeriesPickerDialogProps) {
  const [query, setQuery] = useState(initialQuery);
  const [results, setResults] = useState<SeriesOption[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  // 0 是「无」，其余是 results[index - 1]。
  const [activeIndex, setActiveIndex] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let disposed = false;
    setLoading(true);
    setError(null);

    const trimmed = query.trim();
    const timer = window.setTimeout(
      () => {
        searchSeriesOptions(libraryId, trimmed, LIMIT)
          .then((items) => {
            if (disposed) return;
            const visible = items.filter((item) => item.id !== excludeId);
            setResults(visible);
            const selected = visible.findIndex((item) => item.id === selectedId);
            setActiveIndex(selected >= 0 ? selected + 1 : 0);
          })
          .catch((err: unknown) => {
            if (!disposed) setError(errorMessage(err));
          })
          .finally(() => {
            if (!disposed) setLoading(false);
          });
      },
      trimmed.length > 0 ? DEBOUNCE_MS : 0,
    );

    return () => {
      disposed = true;
      window.clearTimeout(timer);
    };
  }, [excludeId, libraryId, query, selectedId]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  // 键盘移动时让当前项保持在可见区域。
  useEffect(() => {
    listRef.current
      ?.querySelector<HTMLElement>(`[data-index="${activeIndex}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [activeIndex]);

  const pick = (index: number) => {
    // 「无」不依赖搜索结果，加载中也允许点，避免清链接要等请求回来。
    if (index > 0 && loading) return;
    onSelect(index === 0 ? null : (results[index - 1] ?? null));
  };

  const onInputKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActiveIndex((index) => Math.min(index + 1, results.length));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActiveIndex((index) => Math.max(index - 1, 0));
    } else if (event.key === "Enter") {
      event.preventDefault();
      pick(activeIndex);
    }
  };

  const rowClass = (active: boolean) =>
    `flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left transition-colors ${
      active ? "bg-indigo-500/15" : "hover:bg-neutral-800/60"
    }`;

  return createPortal(
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className="flex max-h-[80vh] w-full max-w-md flex-col rounded-2xl border border-neutral-800 bg-neutral-950 shadow-2xl"
      >
        <header className="flex items-center justify-between border-b border-neutral-800 px-4 py-3">
          <h3 className="text-sm font-medium text-neutral-100">{title}</h3>
          <button
            type="button"
            onClick={onClose}
            aria-label="关闭"
            className="flex h-7 w-7 items-center justify-center rounded-lg text-neutral-500 transition-colors hover:bg-neutral-800 hover:text-neutral-200"
          >
            <X size={15} />
          </button>
        </header>

        <div className="flex min-h-0 flex-1 flex-col px-4 py-3">
          <div className="relative">
            <Search
              size={13}
              className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-neutral-600"
            />
            <input
              autoFocus
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={onInputKeyDown}
              placeholder="搜索番剧名、别名或档期"
              className="w-full rounded-lg border border-neutral-800 bg-neutral-900 py-2 pl-9 pr-9 text-sm text-neutral-100 outline-none transition-colors placeholder:text-neutral-600 focus:border-indigo-600"
            />
            {loading && (
              <Loader2
                size={13}
                className="absolute right-3 top-1/2 -translate-y-1/2 animate-spin text-neutral-500"
              />
            )}
          </div>

          <div
            ref={listRef}
            className="mt-2 min-h-0 flex-1 overflow-y-auto"
          >
            <button
              type="button"
              data-index={0}
              onMouseEnter={() => setActiveIndex(0)}
              onClick={() => pick(0)}
              className={rowClass(activeIndex === 0)}
            >
              <span className="min-w-0 flex-1 truncate text-xs text-neutral-300">
                无（清除季链接）
              </span>
              {selectedId == null && (
                <Check size={12} className="shrink-0 text-indigo-400" />
              )}
            </button>

            {results.map((option, index) => {
              const itemIndex = index + 1;
              const active = itemIndex === activeIndex;

              return (
                <button
                  key={option.id}
                  type="button"
                  data-index={itemIndex}
                  onMouseEnter={() => setActiveIndex(itemIndex)}
                  onClick={() => pick(itemIndex)}
                  className={rowClass(active)}
                >
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-xs text-neutral-200">
                      {option.name}
                    </span>
                    <span className="mt-0.5 block truncate text-[10px] text-neutral-500">
                      {option.category}
                    </span>
                  </span>
                  {option.id === selectedId && (
                    <Check size={12} className="shrink-0 text-indigo-400" />
                  )}
                </button>
              );
            })}

            {!loading && results.length === 0 && (
              <p className="px-2 py-8 text-center text-xs text-neutral-500">
                没有匹配的番剧
              </p>
            )}

            {results.length >= LIMIT && (
              <p className="px-2 py-2 text-center text-[10px] text-neutral-600">
                只显示前 {LIMIT} 部，请输入关键词缩小范围
              </p>
            )}
          </div>

          {error && <p className="mt-2 text-[11px] text-red-400">{error}</p>}
        </div>

        <footer className="flex items-center justify-between border-t border-neutral-800 px-4 py-2.5 text-[10px] text-neutral-600">
          <span>↑↓ 选择 · Enter 确认 · Esc 关闭</span>
          <span className="tabular-nums">
            {results.length >= LIMIT ? `${LIMIT}+` : results.length} 个结果
          </span>
        </footer>
      </div>
    </div>,
    document.body,
  );
}
