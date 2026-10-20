import type { ReactNode } from "react";
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
      <main className="flex-1 overflow-y-auto">
        <div className="mx-auto flex min-h-full max-w-6xl flex-col gap-5 px-8 py-8">
          {children}
        </div>
      </main>
    </div>
  );
}
