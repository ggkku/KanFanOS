import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { FolderOpen } from "lucide-react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { errorMessage } from "../../lib/errors";
import type { SeriesSummary } from "../../types/library";

export interface SeriesContextMenuState {
  series: SeriesSummary;
  x: number;
  y: number;
}

interface SeriesContextMenuProps {
  menu: SeriesContextMenuState | null;
  onClose: () => void;
  onError: (message: string) => void;
}

export function SeriesContextMenu({
  menu,
  onClose,
  onError,
}: SeriesContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menu) return;
    const closeOutside = (event: PointerEvent) => {
      if (
        menuRef.current &&
        event.target instanceof Node &&
        !menuRef.current.contains(event.target)
      ) {
        onClose();
      }
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("pointerdown", closeOutside, true);
    document.addEventListener("keydown", closeOnEscape);
    window.addEventListener("scroll", onClose, true);
    window.addEventListener("resize", onClose);
    return () => {
      document.removeEventListener("pointerdown", closeOutside, true);
      document.removeEventListener("keydown", closeOnEscape);
      window.removeEventListener("scroll", onClose, true);
      window.removeEventListener("resize", onClose);
    };
  }, [menu, onClose]);

  if (!menu) return null;

  const openFolder = () => {
    const path = menu.series.path;
    onClose();
    void revealItemInDir(path).catch((reason) => {
      onError(errorMessage(reason));
    });
  };

  const left = Math.min(menu.x, Math.max(8, window.innerWidth - 208));
  const top = Math.min(menu.y, Math.max(8, window.innerHeight - 56));

  return createPortal(
    <div
      ref={menuRef}
      role="menu"
      aria-label={`${menu.series.name} 操作`}
      className="fixed z-[120] w-48 rounded-lg border border-neutral-800 bg-neutral-900 p-1 shadow-2xl"
      style={{ left, top }}
    >
      <button
        type="button"
        role="menuitem"
        autoFocus
        onClick={openFolder}
        className="flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left text-xs text-neutral-200 transition-colors hover:bg-neutral-800 focus:bg-neutral-800 focus:outline-none"
      >
        <FolderOpen size={14} className="shrink-0 text-indigo-400" />
        在文件夹中显示
      </button>
    </div>,
    document.body,
  );
}
