import { useEffect, useRef, useState } from "react";
import { Star } from "lucide-react";

/// 0-10 分对应的四字评价。
export const RATING_LABELS = [
  "粪中之粪",
  "惨不忍睹",
  "乏善可陈",
  "勉强能看",
  "平平无奇",
  "中规中矩",
  "可圈可点",
  "值得一看",
  "回味悠长",
  "惊世之作",
  "神中之神",
];

interface SeriesRatingProps {
  value: number | null;
  disabled?: boolean;
  onChange: (value: number | null) => void;
}

/// 分数越高越暖/亮：红 → 橙 → 琥珀 → 绿 → 蓝 → 紫。
export function ratingColor(score: number): string {
  if (score <= 2) return "text-rose-400";
  if (score <= 4) return "text-orange-400";
  if (score <= 6) return "text-amber-300";
  if (score <= 8) return "text-emerald-400";
  if (score <= 9) return "text-sky-300";
  return "text-violet-300";
}

/// 与 ratingColor 对应的外发光颜色，供选番卡片 hover 使用。
/// 双层发光叠加后会比单层更亮，所以 alpha 保持在 0.5-0.6。
export function ratingGlowColor(score: number): string {
  if (score <= 2) return "rgba(251, 113, 133, 0.6)";
  if (score <= 4) return "rgba(251, 146, 60, 0.6)";
  if (score <= 6) return "rgba(252, 211, 77, 0.55)";
  if (score <= 8) return "rgba(52, 211, 153, 0.55)";
  if (score <= 9) return "rgba(125, 211, 252, 0.55)";
  return "rgba(196, 181, 253, 0.6)";
}

function labelFor(score: number): string {
  return RATING_LABELS[score] ?? "未评分";
}

/// 选集页右上角的个人评分：默认显示分数和评语，点开选择 0-10。
export function SeriesRating({
  value,
  disabled = false,
  onChange,
}: SeriesRatingProps) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;

    const onPointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };

    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  const pick = (score: number | null) => {
    if (disabled) return;
    onChange(score);
    setOpen(false);
  };

  return (
    <div ref={rootRef} className="relative w-full shrink-0">
      <button
        type="button"
        onClick={() => setOpen((current) => !current)}
        disabled={disabled}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label="我的评分"
        className="group flex w-full items-center gap-3 rounded-xl border border-neutral-800 bg-neutral-900/60 px-3.5 py-2 text-left transition-colors hover:border-neutral-700 hover:bg-neutral-900 disabled:cursor-not-allowed disabled:opacity-50"
      >
        <div
          className={`flex h-10 w-10 items-center justify-center rounded-lg border border-neutral-800 bg-neutral-950 text-lg font-semibold tabular-nums ${
            value == null ? "text-neutral-600" : ratingColor(value)
          }`}
        >
          {value ?? "–"}
        </div>

        <div className="min-w-0">
          <div className="flex items-center gap-1 text-[10px] tracking-wider text-neutral-500">
            <Star
              size={10}
              className={value == null ? "text-neutral-600" : "text-amber-400"}
              fill={value == null ? "none" : "currentColor"}
            />
            我的评分
          </div>
          <div
            className={`mt-0.5 text-xs ${
              value == null ? "text-neutral-500" : "text-neutral-200"
            }`}
          >
            {value == null ? "点击评分" : labelFor(value)}
          </div>
          <div className="mt-1 flex gap-0.5">
            {RATING_LABELS.map((_, index) => (
              <span
                key={index}
                className={`h-0.5 w-1.5 rounded-full ${
                  value != null && index <= value
                    ? "bg-indigo-400"
                    : "bg-neutral-800"
                }`}
              />
            ))}
          </div>
        </div>
      </button>

      {open && (
        <div
          role="dialog"
          aria-label="选择评分"
          className="absolute right-0 top-full z-30 mt-2 w-80 rounded-xl border border-neutral-800 bg-neutral-900 p-2 shadow-2xl"
        >
          <div className="px-1.5 pb-1.5 pt-0.5 text-[11px] text-neutral-500">
            给这部番打分
          </div>

          <div className="grid grid-cols-4 gap-1">
            {RATING_LABELS.map((label, score) => {
              const active = value === score;

              return (
                <button
                  key={score}
                  type="button"
                  onClick={() => pick(score)}
                  disabled={disabled}
                  className={`flex flex-col items-center gap-0.5 rounded-lg border px-1 py-2 transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${
                    active
                      ? "border-indigo-500 bg-indigo-500/10"
                      : "border-transparent hover:border-neutral-700 hover:bg-neutral-800"
                  }`}
                >
                  <span
                    className={`text-sm font-semibold tabular-nums ${ratingColor(score)}`}
                  >
                    {score}
                  </span>
                  <span
                    className={`text-[10px] ${
                      active ? "text-neutral-200" : "text-neutral-400"
                    }`}
                  >
                    {label}
                  </span>
                </button>
              );
            })}
          </div>

          {value != null && (
            <div className="mt-1 border-t border-neutral-800 pt-1.5">
              <button
                type="button"
                onClick={() => pick(null)}
                disabled={disabled}
                className="w-full rounded-lg px-2 py-1.5 text-left text-[11px] text-neutral-500 transition-colors hover:bg-neutral-800 hover:text-red-300 disabled:opacity-50"
              >
                清除评分
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
