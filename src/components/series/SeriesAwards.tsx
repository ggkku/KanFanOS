import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { Check, Trophy, X } from "lucide-react";

interface SeriesAwardsProps {
  awards: string[];
  busy: boolean;
  /// 每次变化时打开输入框；由配置弹窗里的「添加奖项」触发。
  openSignal: number;
  onAdd: (name: string) => void;
}

/// 选集页右上角的奖项区：每条奖项一行，数量多时内部滚动。
/// 删除奖项在配置弹窗里操作，这里只展示。
export function SeriesAwards({
  awards,
  busy,
  openSignal,
  onAdd,
}: SeriesAwardsProps) {
  const [adding, setAdding] = useState(false);
  const [draft, setDraft] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (openSignal > 0) setAdding(true);
  }, [openSignal]);

  useEffect(() => {
    if (adding) inputRef.current?.focus();
  }, [adding, openSignal]);

  const cancel = () => {
    setAdding(false);
    setDraft("");
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const name = draft.trim();
    if (!name || busy) return;

    onAdd(name);
    setDraft("");
    setAdding(false);
  };

  return (
    <div className="flex w-full flex-col items-end gap-1.5">
      {adding && (
        <form onSubmit={submit} className="flex w-full items-center gap-1">
          <input
            ref={inputRef}
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") cancel();
            }}
            maxLength={60}
            placeholder="输入奖项名称"
            className="min-w-0 flex-1 rounded-lg border border-neutral-800 bg-neutral-900 px-2.5 py-1.5 text-[11px] text-neutral-100 outline-none transition-colors placeholder:text-neutral-600 focus:border-indigo-600"
          />
          <button
            type="submit"
            aria-label="添加奖项"
            disabled={!draft.trim() || busy}
            className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-indigo-600 text-white transition-colors hover:bg-indigo-500 disabled:cursor-not-allowed disabled:opacity-40"
          >
            <Check size={13} />
          </button>
          <button
            type="button"
            aria-label="取消添加"
            onClick={cancel}
            className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg text-neutral-500 transition-colors hover:bg-neutral-800 hover:text-neutral-300"
          >
            <X size={13} />
          </button>
        </form>
      )}

      {awards.length > 0 && (
        <div
          className={`flex w-full flex-col gap-1 overflow-y-auto ${
            adding ? "max-h-14" : "max-h-[92px]"
          }`}
        >
          {awards.map((award) => (
            <div
              key={award}
              className="flex items-center gap-2 rounded-lg border border-amber-900/40 bg-amber-950/20 px-2.5 py-1"
            >
              <Trophy size={12} className="shrink-0 text-amber-400" />
              <span className="min-w-0 flex-1 truncate text-xs text-amber-200">
                {award}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
