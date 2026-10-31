import { useEffect, useState } from "react";
import type { FormEvent } from "react";
import { createPortal } from "react-dom";
import { Loader2, Plus, Save, Trash2, X } from "lucide-react";
import {
  addBookmark,
  removeBookmark,
  updateBookmark,
} from "../../api/bookmarks";
import type { Bookmark } from "../../api/bookmarks";
import { errorMessage } from "../../lib/errors";

interface BookmarksDialogProps {
  bookmarks: Bookmark[];
  onChange: (items: Bookmark[]) => void;
  onClose: () => void;
}

const inputClass =
  "min-w-0 rounded-lg border border-neutral-800 bg-neutral-950 px-2.5 py-1.5 text-xs text-neutral-200 placeholder:text-neutral-600 focus:border-indigo-500/60 focus:outline-none";

/// 收藏夹管理弹窗：手动新增、修改、删除。
/// 原生浏览器 webview 由资源页在弹窗打开时隐藏，否则会盖住这个弹窗。
export function BookmarksDialog({
  bookmarks,
  onChange,
  onClose,
}: BookmarksDialogProps) {
  const [title, setTitle] = useState("");
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  const add = async (event: FormEvent) => {
    event.preventDefault();
    if (!url.trim()) return;

    setBusy(true);
    setError(null);
    try {
      onChange(await addBookmark(title, url));
      setTitle("");
      setUrl("");
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  };

  return createPortal(
    <div
      className="fixed inset-0 z-[80] flex items-center justify-center bg-transparent p-4"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="收藏夹管理"
        className="flex max-h-[82vh] w-full max-w-2xl flex-col rounded-2xl border border-neutral-800 bg-neutral-950 shadow-2xl"
      >
        <header className="flex shrink-0 items-center justify-between border-b border-neutral-800 px-5 py-3.5">
          <div>
            <h2 className="text-sm font-medium text-neutral-100">收藏夹管理</h2>
            <p className="mt-0.5 text-[11px] text-neutral-500">
              共 {bookmarks.length} 条，修改后点每行的 ✓ 保存。
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

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
          <form
            onSubmit={add}
            className="flex flex-col gap-2 rounded-xl border border-neutral-800 bg-neutral-900/50 p-3 sm:flex-row sm:items-center"
          >
            <input
              value={title}
              onChange={(event) => setTitle(event.target.value)}
              placeholder="标题（可留空）"
              className={`${inputClass} flex-1`}
            />
            <input
              value={url}
              onChange={(event) => setUrl(event.target.value)}
              placeholder="https://example.com"
              className={`${inputClass} flex-[1.4]`}
            />
            <button
              type="submit"
              disabled={busy || !url.trim()}
              className="flex shrink-0 items-center justify-center gap-1.5 rounded-lg bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-indigo-500 disabled:opacity-40"
            >
              {busy ? (
                <Loader2 size={12} className="animate-spin" />
              ) : (
                <Plus size={12} />
              )}
              添加
            </button>
          </form>

          {error && (
            <p className="mt-3 rounded-lg border border-rose-900/60 bg-rose-950/30 px-3 py-2 text-[11px] text-rose-300">
              {error}
            </p>
          )}

          <div className="mt-4 flex flex-col gap-2">
            {bookmarks.length === 0 ? (
              <p className="py-6 text-center text-xs text-neutral-600">
                还没有收藏。可以在上方手动添加，或在浏览网页时点工具栏的 ★。
              </p>
            ) : (
              bookmarks.map((bookmark) => (
                <BookmarkRow
                  key={bookmark.id}
                  bookmark={bookmark}
                  onChange={onChange}
                  onError={setError}
                />
              ))
            )}
          </div>
        </div>

        <footer className="flex shrink-0 justify-end border-t border-neutral-800 px-5 py-3">
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg border border-neutral-800 bg-neutral-900 px-3.5 py-1.5 text-xs text-neutral-300 transition-colors hover:bg-neutral-800"
          >
            完成
          </button>
        </footer>
      </div>
    </div>,
    document.body,
  );
}

interface BookmarkRowProps {
  bookmark: Bookmark;
  onChange: (items: Bookmark[]) => void;
  onError: (message: string) => void;
}

function BookmarkRow({ bookmark, onChange, onError }: BookmarkRowProps) {
  const [title, setTitle] = useState(bookmark.title);
  const [url, setUrl] = useState(bookmark.url);
  const [busy, setBusy] = useState(false);

  // 去重或外部更新后，标题/地址可能变化。
  useEffect(() => {
    setTitle(bookmark.title);
    setUrl(bookmark.url);
  }, [bookmark.title, bookmark.url]);

  const save = async () => {
    setBusy(true);
    onError("");
    try {
      onChange(await updateBookmark(bookmark.id, title, url));
    } catch (reason) {
      onError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    setBusy(true);
    onError("");
    try {
      onChange(await removeBookmark(bookmark.id));
    } catch (reason) {
      onError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  };

  const dirty = title !== bookmark.title || url !== bookmark.url;

  return (
    <div className="flex items-center gap-2 rounded-xl border border-neutral-800 bg-neutral-900/40 p-2">
      <input
        value={title}
        onChange={(event) => setTitle(event.target.value)}
        placeholder="标题"
        className={`${inputClass} flex-1`}
      />
      <input
        value={url}
        onChange={(event) => setUrl(event.target.value)}
        placeholder="https://example.com"
        className={`${inputClass} flex-[1.6]`}
      />
      <button
        type="button"
        onClick={() => void save()}
        disabled={busy || !dirty || !url.trim()}
        title="保存"
        className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg text-neutral-400 transition-colors hover:bg-neutral-800 hover:text-neutral-100 disabled:opacity-30"
      >
        {busy ? (
          <Loader2 size={13} className="animate-spin" />
        ) : (
          <Save size={13} />
        )}
      </button>
      <button
        type="button"
        onClick={() => void remove()}
        disabled={busy}
        title="删除"
        className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg text-neutral-500 transition-colors hover:bg-rose-950/60 hover:text-rose-300 disabled:opacity-30"
      >
        <Trash2 size={13} />
      </button>
    </div>
  );
}
