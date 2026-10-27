import { Clock, Globe, Loader2, Rss, Search, Settings, X } from "lucide-react";
import type { LucideIcon } from "lucide-react";
// 直接使用 tauri icon 生成的大尺寸图标，换图标后无需再手动拷一份。
import appIcon from "../../../src-tauri/icons/128x128@2x.png";
import type { CategorySummary } from "../../types/library";
import type { View } from "../../types/navigation";

interface SidebarProps {
  view: View;
  categories: CategorySummary[];
  scanning: boolean;
  syncing: boolean;
  query: string;
  onQueryChange: (query: string) => void;
  onNavigate: (view: View) => void;
}

const buttonBase =
  "flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors";
const buttonActive = "bg-neutral-800 text-neutral-100";
const buttonIdle =
  "text-neutral-400 hover:bg-neutral-800/60 hover:text-neutral-200";

interface NavButtonProps {
  icon: LucideIcon;
  label: string;
  active: boolean;
  onClick: () => void;
}

function NavButton({ icon: Icon, label, active, onClick }: NavButtonProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`${buttonBase} ${active ? buttonActive : buttonIdle}`}
    >
      <Icon size={17} strokeWidth={active ? 2.2 : 1.8} />
      {label}
    </button>
  );
}

export function Sidebar({
  view,
  categories,
  scanning,
  syncing,
  query,
  onQueryChange,
  onNavigate,
}: SidebarProps) {
  return (
    <aside className="flex w-60 shrink-0 flex-col border-r border-neutral-800 bg-neutral-900/60">
      <div className="flex items-center gap-3 px-5 py-5">
        <img
          src={appIcon}
          alt=""
          className="h-9 w-9 shrink-0 object-contain"
        />
        <div className="text-sm font-semibold tracking-wide">KanFanOS</div>
      </div>

      <div className="px-3 pb-3">
        <div className="relative">
          <Search
            size={13}
            className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-neutral-500"
          />
          <input
            value={query}
            onChange={(event) => onQueryChange(event.target.value)}
            placeholder="搜索番剧或剧集"
            className="w-full rounded-lg border border-neutral-800 bg-neutral-950 py-1.5 pl-8 pr-7 text-xs text-neutral-200 placeholder:text-neutral-600 focus:border-indigo-500/60 focus:outline-none"
          />
          {query && (
            <button
              type="button"
              onClick={() => onQueryChange("")}
              className="absolute right-2 top-1/2 -translate-y-1/2 text-neutral-500 transition-colors hover:text-neutral-300"
            >
              <X size={12} />
            </button>
          )}
        </div>
      </div>

      <div className="flex flex-col gap-0.5 px-3">
        <NavButton
          icon={Clock}
          label="最近观看"
          active={view.kind === "recent"}
          onClick={() => onNavigate({ kind: "recent" })}
        />
        <NavButton
          icon={Rss}
          label="追番"
          active={view.kind === "rss"}
          onClick={() => onNavigate({ kind: "rss" })}
        />
        <NavButton
          icon={Globe}
          label="资源"
          active={view.kind === "resources"}
          onClick={() => onNavigate({ kind: "resources" })}
        />
      </div>

      <div className="mt-4 flex min-h-0 flex-1 flex-col">
        <div className="px-5 pb-2 text-[10px] uppercase tracking-widest text-neutral-600">
          档期
        </div>

        <nav className="min-h-0 flex-1 overflow-y-auto px-3 pb-3">
          {categories.length === 0 ? (
            <p className="px-3 py-2 text-xs leading-relaxed text-neutral-600">
              扫描完成后，这里会按年份月份列出番剧。
            </p>
          ) : (
            <div className="flex flex-col gap-0.5">
              {categories.map((category) => {
                const active =
                  view.kind === "category" &&
                  view.category === category.key;
                return (
                  <button
                    key={category.key}
                    type="button"
                    onClick={() =>
                      onNavigate({
                        kind: "category",
                        category: category.key,
                      })
                    }
                    className={`group flex w-full items-center justify-between gap-2 rounded-lg px-3 py-2 text-sm transition-colors ${
                      active ? buttonActive : buttonIdle
                    }`}
                  >
                    <span className="truncate">{category.label}</span>
                    <span
                      className={`shrink-0 text-[11px] tabular-nums ${
                        active
                          ? "text-neutral-500"
                          : "text-neutral-600 group-hover:text-neutral-500"
                      }`}
                    >
                      {category.count}
                    </span>
                  </button>
                );
              })}
            </div>
          )}
        </nav>
      </div>

      <div className="border-t border-neutral-800 px-3 py-3">
        {(scanning || syncing) && (
          <div className="mb-2 flex items-center gap-2 px-3 text-[11px] text-indigo-400">
            <Loader2 size={12} className="animate-spin" />
            {scanning ? "正在扫描…" : "自动同步中…"}
          </div>
        )}
        <NavButton
          icon={Settings}
          label="设置"
          active={view.kind === "settings"}
          onClick={() => onNavigate({ kind: "settings" })}
        />
      </div>
    </aside>
  );
}
