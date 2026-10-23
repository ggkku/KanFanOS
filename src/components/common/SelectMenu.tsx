import { useEffect, useRef, useState } from "react";
import { Check, ChevronDown } from "lucide-react";

interface SelectOption<T extends string | number> {
  value: T;
  label: string;
}

interface SelectMenuProps<T extends string | number> {
  value: T;
  options: SelectOption<T>[];
  onChange: (value: T) => void;
  disabled?: boolean;
  align?: "left" | "right";
  ariaLabel?: string;
}

/// 自定义下拉菜单，和排序按钮一致的圆角矩形样式。
export function SelectMenu<T extends string | number>({
  value,
  options,
  onChange,
  disabled = false,
  align = "left",
  ariaLabel,
}: SelectMenuProps<T>) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  const current = options.find((option) => option.value === value) ?? options[0];

  // 点击外部或按 Esc 收起。
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

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        className="flex items-center gap-2 rounded-lg border border-neutral-800 bg-neutral-900 px-3 py-1.5 text-xs text-neutral-300 transition-colors hover:bg-neutral-800 disabled:cursor-not-allowed disabled:opacity-40"
      >
        <span className="min-w-0 truncate">{current?.label}</span>
        <ChevronDown
          size={13}
          className={`shrink-0 transition-transform duration-200 ${
            open ? "rotate-180" : ""
          }`}
        />
      </button>

      {open && (
        <div
          role="listbox"
          className={`absolute top-full z-30 mt-2 max-h-64 w-max min-w-full max-w-xs overflow-y-auto rounded-xl border border-neutral-800 bg-neutral-900 p-1 shadow-2xl ${
            align === "right" ? "right-0" : "left-0"
          }`}
        >
          {options.map((option) => {
            const active = option.value === value;

            return (
              <button
                key={String(option.value)}
                type="button"
                role="option"
                aria-selected={active}
                onClick={() => {
                  onChange(option.value);
                  setOpen(false);
                }}
                className={`flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs transition-colors ${
                  active
                    ? "bg-indigo-500/15 text-indigo-300"
                    : "text-neutral-300 hover:bg-neutral-800"
                }`}
              >
                <Check
                  size={12}
                  className={`shrink-0 ${active ? "" : "opacity-0"}`}
                />
                <span className="min-w-0 truncate">{option.label}</span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
