import type { LucideIcon } from "lucide-react";

interface EmptyStateProps {
  icon: LucideIcon;
  title: string;
  description: string;
}

export function EmptyState({ icon: Icon, title, description }: EmptyStateProps) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center rounded-xl border border-dashed border-neutral-800 bg-neutral-900/30 px-6 py-16 text-center">
      <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-full bg-neutral-800/80 text-neutral-400">
        <Icon size={22} />
      </div>
      <p className="text-sm font-medium text-neutral-200">{title}</p>
      <p className="mt-1 max-w-sm text-xs leading-relaxed text-neutral-500">
        {description}
      </p>
    </div>
  );
}
