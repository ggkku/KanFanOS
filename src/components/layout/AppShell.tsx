import type { ReactNode, RefObject } from "react";
import { Sidebar } from "./Sidebar";
import type { CategorySummary } from "../../types/library";
import type { View } from "../../types/navigation";

interface AppShellProps {
  view: View;
  categories: CategorySummary[];
  scanning: boolean;
  syncing: boolean;
  query: string;
  onQueryChange: (query: string) => void;
  onNavigate: (view: View) => void;
  /** 主滚动容器，用于从选集页返回时恢复位置。 */
  scrollRef?: RefObject<HTMLElement | null>;
  /** 资源页需要铺满宽度，不套 `max-w-6xl`。 */
  wide?: boolean;
  children: ReactNode;
}

export function AppShell({
  view,
  categories,
  scanning,
  syncing,
  query,
  onQueryChange,
  onNavigate,
  scrollRef,
  wide = false,
  children,
}: AppShellProps) {
  return (
    <div className="flex h-full bg-neutral-950 text-neutral-100">
      <Sidebar
        view={view}
        categories={categories}
        scanning={scanning}
        syncing={syncing}
        query={query}
        onQueryChange={onQueryChange}
        onNavigate={onNavigate}
      />
      <main ref={scrollRef} className="flex-1 overflow-y-auto overflow-x-hidden">
        <div
          className={
            wide
              ? "mx-auto flex min-h-full w-full flex-col gap-5 px-6 py-6"
              : "mx-auto flex min-h-full max-w-6xl flex-col gap-5 px-8 py-8"
          }
        >
          {children}
        </div>
      </main>
    </div>
  );
}
