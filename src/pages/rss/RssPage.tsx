import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import {
  Check,
  CheckCircle2,
  Download,
  FolderOpen,
  Loader2,
  Pencil,
  Plus,
  RefreshCw,
  Rss as RssIcon,
  Trash2,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { confirm, open } from "@tauri-apps/plugin-dialog";
import {
  addRssFeed,
  addRssFollow,
  deleteRssFeed,
  deleteRssFollow,
  downloadRssMatch,
  getRssDownloadRoot,
  listRssFeeds,
  listRssFollows,
  listRssMatches,
  parseReleaseTitle,
  refreshAllRss,
  refreshRssFeed,
  updateRssFollow,
} from "../../api/rss";
import { errorMessage } from "../../lib/errors";
import type {
  ParsedReleaseTitle,
  RssFeed,
  RssFollow,
  RssMatch,
} from "../../types/rss";

const inputClass =
  "w-full rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-2 text-xs text-neutral-200 placeholder:text-neutral-600 focus:border-indigo-500/60 focus:outline-none";

function formatTime(value: number | null): string {
  if (value == null) return "从未刷新";
  const diff = Date.now() - value;
  if (diff < 60_000) return "刚刚";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`;
  return new Date(value).toLocaleDateString("zh-CN");
}

function matchStatusLabel(
  status: string,
  autoDownloaded: boolean,
): string {
  switch (status) {
    case "pending":
      return "待下载";
    case "queued":
      return autoDownloaded ? "已自动下载" : "已加入下载";
    case "ignored":
      return "已忽略";
    case "failed":
      return "失败";
    default:
      return status;
  }
}

function followToAliases(follow: RssFollow): string {
  return follow.aliases.join("\n");
}

function followToGroups(follow: RssFollow): string {
  return follow.preferredGroups.join("\n");
}

export function RssPage({
  initialText,
  onInitialTextConsumed,
}: {
  initialText?: string | null;
  onInitialTextConsumed?: () => void;
}) {
  const [feeds, setFeeds] = useState<RssFeed[]>([]);
  const [follows, setFollows] = useState<RssFollow[]>([]);
  const [matches, setMatches] = useState<RssMatch[]>([]);
  const [downloadRoot, setDownloadRoot] = useState("");
  const [downloadRootExists, setDownloadRootExists] = useState(true);
  const [selectedFollowId, setSelectedFollowId] = useState<number | null>(
    null,
  );
  const selectedFollowIdRef = useRef<number | null | undefined>(undefined);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [feedDialogOpen, setFeedDialogOpen] = useState(false);
  const [followDialogOpen, setFollowDialogOpen] = useState(false);
  const [browserSeed, setBrowserSeed] = useState<ParsedReleaseTitle | null>(
    null,
  );
  const [editingFollow, setEditingFollow] = useState<RssFollow | null>(null);

  const setSelection = (followId: number | null) => {
    selectedFollowIdRef.current = followId;
    setSelectedFollowId(followId);
  };

  const loadMatches = async (followId: number | null) => {
    const nextMatches = await listRssMatches(
      80,
      followId == null ? [] : [followId],
    );
    setMatches(nextMatches);
  };

  const reload = async () => {
    const [nextFeeds, nextFollows, nextRoot] = await Promise.all([
      listRssFeeds(),
      listRssFollows(),
      getRssDownloadRoot(),
    ]);
    setFeeds(nextFeeds);
    setFollows(nextFollows);
    setDownloadRoot(nextRoot.path);
    setDownloadRootExists(nextRoot.exists);

    const current = selectedFollowIdRef.current;
    const nextSelection =
      current === undefined
        ? nextFollows[0]?.id ?? null
        : current !== null && nextFollows.some((follow) => follow.id === current)
          ? current
          : nextFollows[0]?.id ?? null;
    setSelection(nextSelection);
    await loadMatches(nextSelection);
  };

  const selectFollow = (followId: number) => {
    if (selectedFollowIdRef.current === followId) return;
    setSelection(followId);
    void loadMatches(followId).catch((reason) =>
      setError(errorMessage(reason)),
    );
  };

  useEffect(() => {
    let disposed = false;
    setLoading(true);
    void reload()
      .catch((reason) => {
        if (!disposed) setError(errorMessage(reason));
      })
      .finally(() => {
        if (!disposed) setLoading(false);
      });

    let unlisten: UnlistenFn | undefined;
    void listen("rss-updated", () => {
      if (!disposed) void reload().catch(() => {});
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });

    let unlistenMissing: UnlistenFn | undefined;
    void listen<{ path: string }>("download-dir-missing", (event) => {
      if (disposed) return;
      setDownloadRootExists(false);
      setError(`下载目录不存在：${event.payload.path}`);
      void reload().catch(() => {});
    }).then((fn) => {
      if (disposed) fn();
      else unlistenMissing = fn;
    });

    return () => {
      disposed = true;
      unlisten?.();
      unlistenMissing?.();
    };
  }, []);

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 5000);
    return () => window.clearTimeout(timer);
  }, [notice]);

  useEffect(() => {
    if (!initialText?.trim()) return;
    let disposed = false;
    void parseReleaseTitle(initialText)
      .then((parsed) => {
        if (disposed) return;
        setBrowserSeed(parsed);
        onInitialTextConsumed?.();
      })
      .catch((reason) => {
        if (disposed) return;
        setError(errorMessage(reason));
        onInitialTextConsumed?.();
      });
    return () => {
      disposed = true;
    };
  }, [initialText, onInitialTextConsumed]);

  useEffect(() => {
    if (!browserSeed) return;
    if (feeds.length === 0) {
      setError("还没有 RSS 源，请先添加 RSS 源后再从浏览器添加追番");
      return;
    }
    setFollowDialogOpen(true);
  }, [browserSeed, feeds.length]);

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
      await reload();
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  };

  const refreshAll = () =>
    void run(async () => {
      const results = await refreshAllRss();
      const entries = results.reduce((sum, item) => sum + item.addedEntries, 0);
      const downloads = results.reduce((sum, item) => sum + item.downloaded, 0);
      setNotice(`刷新完成：${entries} 条更新，自动下载 ${downloads} 条`);
    });

  const refreshOne = (feed: RssFeed) =>
    void run(async () => {
      const result = await refreshRssFeed(feed.id);
      setNotice(
        `「${feed.title || feed.url}」：${result.addedEntries} 条更新，自动下载 ${result.downloaded} 条`,
      );
    });

  const removeFeed = async (feed: RssFeed) => {
    const yes = await confirm(
      `删除 RSS 源「${feed.title || feed.url}」？对应的追番订阅也会删除。`,
      { title: "删除 RSS 源", kind: "warning" },
    );
    if (!yes) return;
    void run(async () => {
      await deleteRssFeed(feed.id);
      setNotice("RSS 源已删除");
    });
  };

  const removeFollow = async (follow: RssFollow) => {
    const yes = await confirm(`不再追「${follow.title}」？`, {
      title: "删除追番",
      kind: "warning",
    });
    if (!yes) return;
    void run(async () => {
      await deleteRssFollow(follow.id);
      setNotice("已停止追番");
    });
  };

  const setAuto = (follow: RssFollow, autoDownload: boolean) =>
    void run(async () => {
      await updateRssFollow({
        followId: follow.id,
        title: follow.title,
        aliases: followToAliases(follow),
        preferredGroups: followToGroups(follow),
        subtitlePackaging: follow.subtitlePackaging,
        resolution: follow.resolution,
        savePath: follow.savePath,
        autoDownload,
        status: follow.status,
      });
    });

  const toggleStatus = (follow: RssFollow) =>
    void run(async () => {
      await updateRssFollow({
        followId: follow.id,
        title: follow.title,
        aliases: followToAliases(follow),
        preferredGroups: followToGroups(follow),
        subtitlePackaging: follow.subtitlePackaging,
        resolution: follow.resolution,
        savePath: follow.savePath,
        autoDownload: follow.autoDownload,
        status: follow.status === "active" ? "paused" : "active",
      });
    });

  const downloadMatch = (item: RssMatch) =>
    void run(async () => {
      await downloadRssMatch(item.id);
      setNotice(`已加入下载：${item.followTitle} 第 ${item.episode ?? "?"} 集`);
    });

  return (
    <div className="flex flex-1 flex-col gap-5">
      <header className="flex flex-col gap-3">
        <div>
          <h1 className="flex items-center gap-2 text-2xl font-semibold">
            <RssIcon size={22} className="text-indigo-400" />
            追番
          </h1>
          <p className="mt-1 text-sm text-neutral-400">
            管理 RSS 源，自动匹配番剧更新，只补最新一集。
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={() => setFeedDialogOpen(true)}
            className="flex items-center gap-1.5 rounded-lg border border-neutral-800 bg-neutral-900 px-3 py-1.5 text-xs text-neutral-300 transition-colors hover:bg-neutral-800"
          >
            <Plus size={13} />
            添加 RSS 源
          </button>
          <button
            type="button"
            onClick={() => setFollowDialogOpen(true)}
            disabled={feeds.length === 0}
            className="flex items-center gap-1.5 rounded-lg bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-indigo-500 disabled:opacity-40"
          >
            <Plus size={13} />
            添加追番
          </button>
          <button
            type="button"
            onClick={refreshAll}
            disabled={busy || feeds.length === 0}
            className="flex items-center gap-1.5 rounded-lg border border-neutral-800 bg-neutral-900 px-3 py-1.5 text-xs text-neutral-300 transition-colors hover:bg-neutral-800 disabled:opacity-40"
          >
            <RefreshCw size={13} className={busy ? "animate-spin" : ""} />
            刷新全部
          </button>
        </div>
      </header>

      {error && (
        <p className="rounded-lg border border-rose-900/60 bg-rose-950/30 px-3 py-2 text-xs text-rose-300">
          {error}
        </p>
      )}
      {downloadRoot && !downloadRootExists && (
        <p className="rounded-lg border border-amber-900/60 bg-amber-950/25 px-3 py-2 text-xs text-amber-300">
          当前「下载到」目录不存在：{downloadRoot}。下载会失败，请先在资源页设置有效目录。
        </p>
      )}
      {notice && (
        <p className="flex items-center gap-1.5 rounded-lg border border-emerald-900/60 bg-emerald-950/30 px-3 py-2 text-xs text-emerald-300">
          <CheckCircle2 size={13} />
          {notice}
        </p>
      )}

      {loading ? (
        <div className="flex flex-1 items-center justify-center py-20 text-neutral-500">
          <Loader2 size={20} className="animate-spin" />
        </div>
      ) : (
        <>
          <section className="flex flex-col gap-2">
            <div className="flex items-center justify-between">
              <h2 className="text-sm font-medium text-neutral-200">RSS 源</h2>
              <span className="text-[11px] text-neutral-500">
                {feeds.length} 个源
              </span>
            </div>
            {feeds.length === 0 ? (
              <p className="rounded-xl border border-dashed border-neutral-800 px-4 py-6 text-center text-xs text-neutral-500">
                还没有 RSS 源。添加一个 Mikan、Nyaa 或其它站点的 RSS 地址开始追番。
              </p>
            ) : (
              <div className="divide-y divide-neutral-800/70 overflow-hidden rounded-xl border border-neutral-800 bg-neutral-900/30">
                {feeds.map((feed) => (
                  <div
                    key={feed.id}
                    className="flex flex-wrap items-center gap-3 px-4 py-3"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-xs text-neutral-200">
                        {feed.title || feed.url}
                      </div>
                      <div className="truncate text-[10px] text-neutral-500">
                        {feed.url}
                      </div>
                      {feed.lastError && (
                        <div className="mt-0.5 text-[10px] text-rose-400">
                          {feed.lastError}
                        </div>
                      )}
                    </div>
                    <span className="shrink-0 text-[10px] text-neutral-500">
                      {formatTime(feed.lastFetchAt)}
                    </span>
                    <button
                      type="button"
                      onClick={() => refreshOne(feed)}
                      disabled={busy}
                      className="rounded border border-neutral-700 px-2 py-1 text-[10px] text-neutral-400 transition-colors hover:bg-neutral-800 hover:text-neutral-200 disabled:opacity-40"
                    >
                      刷新
                    </button>
                    <button
                      type="button"
                      onClick={() => void removeFeed(feed)}
                      disabled={busy}
                      className="rounded p-1 text-neutral-600 transition-colors hover:bg-rose-950/40 hover:text-rose-300 disabled:opacity-40"
                      title="删除 RSS 源"
                    >
                      <Trash2 size={13} />
                    </button>
                  </div>
                ))}
              </div>
            )}
          </section>

          <section className="flex flex-col gap-2">
            <div className="flex items-center justify-between">
              <h2 className="text-sm font-medium text-neutral-200">正在追</h2>
              <span className="text-[11px] text-neutral-500">
                选中 {selectedFollowId == null ? 0 : 1} / {follows.length} 部
              </span>
            </div>
            {follows.length === 0 ? (
              <p className="rounded-xl border border-dashed border-neutral-800 px-4 py-6 text-center text-xs text-neutral-500">
                添加追番后，这里会显示下一集和自动下载状态。
              </p>
            ) : (
              <div className="grid gap-3 md:grid-cols-2">
                {follows.map((follow) => {
                  const selected = selectedFollowId === follow.id;
                  const missingSavePath = follow.savePath
                    ? !follow.savePathExists
                    : !downloadRootExists;
                  return (
                  <div
                    key={follow.id}
                    role="radio"
                    aria-checked={selected}
                    tabIndex={0}
                    onClick={(event) => {
                      const target = event.target as HTMLElement;
                      if (
                        target.closest(
                          "button, input, label, textarea, select, a",
                        )
                      ) {
                        return;
                      }
                      selectFollow(follow.id);
                    }}
                    onKeyDown={(event) => {
                      if (event.key !== "Enter" && event.key !== " ") return;
                      const target = event.target as HTMLElement;
                      if (
                        target.closest(
                          "button, input, label, textarea, select, a",
                        )
                      ) {
                        return;
                      }
                      event.preventDefault();
                      selectFollow(follow.id);
                    }}
                    className={`flex cursor-pointer flex-col gap-2 rounded-xl border p-4 transition-colors ${
                      selected
                        ? "border-indigo-500/50 bg-indigo-950/15"
                        : "border-neutral-800 bg-neutral-900/30 hover:border-neutral-700"
                    }`}
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="flex min-w-0 items-start gap-2">
                        <span
                          className={`mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded border ${
                            selected
                              ? "border-indigo-500 bg-indigo-500 text-white"
                              : "border-neutral-700 text-transparent"
                          }`}
                        >
                          <Check size={11} strokeWidth={3} />
                        </span>
                        <div className="min-w-0">
                          <div className="truncate text-sm text-neutral-100">
                            {follow.title}
                          </div>
                          <div className="mt-0.5 truncate text-[10px] text-neutral-500">
                            {follow.feedTitle || "RSS 源"} ·{" "}
                            {follow.resolution || "1080p"}
                          </div>
                        </div>
                      </div>
                      <span
                        className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] ${
                          follow.status === "active"
                            ? "bg-emerald-500/10 text-emerald-400"
                            : "bg-neutral-800 text-neutral-500"
                        }`}
                      >
                        {follow.status === "active" ? "追番中" : "已暂停"}
                      </span>
                    </div>
                    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-neutral-500">
                      <span>
                        已下载到第 {follow.lastEpisode ?? "—"} 集
                      </span>
                      <span
                        className="truncate"
                        title={follow.savePath ?? "当前「下载到」目录"}
                      >
                        目录：
                        {follow.savePath || "使用当前「下载到」目录"}
                      </span>
                    </div>
                    {missingSavePath && (
                      <div className="rounded border border-amber-900/60 bg-amber-950/25 px-2 py-1 text-[10px] text-amber-300">
                        保存目录不存在：
                        {follow.savePath || downloadRoot || "未设置"}
                      </div>
                    )}
                    {follow.aliases.length > 0 && (
                      <div className="truncate text-[10px] text-neutral-600">
                        别名：{follow.aliases.join("、")}
                      </div>
                    )}
                    {follow.preferredGroups.length > 0 && (
                      <div className="truncate text-[10px] text-indigo-400/70">
                        限定字幕组：{follow.preferredGroups.join("、")}
                      </div>
                    )}
                    {follow.subtitlePackaging && (
                      <div className="truncate text-[10px] text-indigo-400/70">
                        字幕封装：{follow.subtitlePackaging}
                      </div>
                    )}
                    <div className="flex flex-wrap items-center gap-2">
                      <label className="flex cursor-pointer select-none items-center gap-1.5 text-[11px] text-neutral-400">
                        <input
                          type="checkbox"
                          checked={follow.autoDownload}
                          onChange={(event) =>
                            setAuto(follow, event.target.checked)
                          }
                          disabled={busy}
                          className="h-3 w-3 accent-indigo-500"
                        />
                        自动下载最新一集
                      </label>
                      <button
                        type="button"
                        onClick={() => setEditingFollow(follow)}
                        disabled={busy}
                        className="ml-auto flex items-center gap-1 rounded border border-neutral-700 px-2 py-0.5 text-[10px] text-neutral-400 transition-colors hover:bg-neutral-800 hover:text-neutral-200 disabled:opacity-40"
                      >
                        <Pencil size={10} />
                        编辑
                      </button>
                      <button
                        type="button"
                        onClick={() => toggleStatus(follow)}
                        disabled={busy}
                        className="rounded border border-neutral-700 px-2 py-0.5 text-[10px] text-neutral-400 transition-colors hover:bg-neutral-800 hover:text-neutral-200 disabled:opacity-40"
                      >
                        {follow.status === "active" ? "暂停" : "继续"}
                      </button>
                      <button
                        type="button"
                        onClick={() => void removeFollow(follow)}
                        disabled={busy}
                        className="rounded p-1 text-neutral-600 transition-colors hover:bg-rose-950/40 hover:text-rose-300 disabled:opacity-40"
                        title="删除追番"
                      >
                        <Trash2 size={12} />
                      </button>
                    </div>
                  </div>
                  );
                })}
              </div>
            )}
          </section>

          <section className="flex flex-col gap-2">
            <div className="flex items-center justify-between">
              <h2 className="text-sm font-medium text-neutral-200">最近匹配</h2>
              <span className="text-[11px] text-neutral-500">
                自动下载最新一集，其它更新保留在待下载
              </span>
            </div>
            {matches.length === 0 ? (
              <p className="rounded-xl border border-dashed border-neutral-800 px-4 py-6 text-center text-xs text-neutral-500">
                {selectedFollowId == null
                  ? "还没有选中追番。点击上方追番卡片选择要查看的番剧。"
                  : "刷新 RSS 后，匹配到的更新会显示在这里。"}
              </p>
            ) : (
              <div className="divide-y divide-neutral-800/70 overflow-hidden rounded-xl border border-neutral-800 bg-neutral-900/30">
                {matches.map((item) => (
                  <div
                    key={item.id}
                    className="flex flex-wrap items-center gap-3 px-4 py-3"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-xs text-neutral-200">
                        {item.entryTitle}
                      </div>
                      <div className="truncate text-[10px] text-neutral-500">
                        {item.releaseGroup ? `${item.releaseGroup} · ` : ""}
                        {item.subtitlePackaging
                          ? `${item.subtitlePackaging} · `
                          : ""}
                        {item.followTitle}
                        {item.episode != null ? ` · 第 ${item.episode} 集` : ""}
                        {" · "}
                        <span
                          className={
                            item.status === "queued" && item.autoDownloaded
                              ? "text-emerald-400"
                              : ""
                          }
                        >
                          {matchStatusLabel(
                            item.status,
                            item.autoDownloaded,
                          )}
                        </span>
                        {item.error ? ` · ${item.error}` : ""}
                      </div>
                    </div>
                    <span className="shrink-0 text-[10px] text-neutral-600">
                      {formatTime(item.publishedAt || item.createdAt)}
                    </span>
                    {(item.status === "pending" || item.status === "failed") && (
                      <button
                        type="button"
                        onClick={() => downloadMatch(item)}
                        disabled={busy}
                        className="flex shrink-0 items-center gap-1 rounded border border-indigo-800/60 px-2 py-1 text-[10px] text-indigo-300 transition-colors hover:bg-indigo-950/40 disabled:opacity-40"
                      >
                        <Download size={11} />
                        下载
                      </button>
                    )}
                  </div>
                ))}
              </div>
            )}
          </section>
        </>
      )}

      {feedDialogOpen && (
        <AddFeedDialog
          onClose={() => setFeedDialogOpen(false)}
          onAdded={(feed) => {
            setFeedDialogOpen(false);
            setNotice(`已添加 RSS 源：${feed.title || feed.url}`);
            void reload();
          }}
        />
      )}

      {followDialogOpen && (
        <AddFollowDialog
          feeds={feeds}
          initial={browserSeed}
          defaultSavePath={downloadRoot}
          defaultSavePathExists={downloadRootExists}
          onClose={() => {
            setFollowDialogOpen(false);
            setBrowserSeed(null);
          }}
          onAdded={(follow) => {
            setFollowDialogOpen(false);
            setBrowserSeed(null);
            setSelection(follow.id);
            setNotice(`开始追番：${follow.title}`);
            void reload();
          }}
        />
      )}

      {editingFollow && (
        <EditFollowDialog
          follow={editingFollow}
          defaultSavePath={downloadRoot}
          defaultSavePathExists={downloadRootExists}
          onClose={() => setEditingFollow(null)}
          onSaved={() => {
            const feedId = editingFollow.feedId;
            setEditingFollow(null);
            setNotice("追番设置已更新，正在重新匹配");
            void (async () => {
              try {
                await refreshRssFeed(feedId);
              } catch (reason) {
                setError(errorMessage(reason));
              } finally {
                await reload();
              }
            })();
          }}
        />
      )}
    </div>
  );
}

function AddFeedDialog({
  onClose,
  onAdded,
}: {
  onClose: () => void;
  onAdded: (feed: RssFeed) => void;
}) {
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    if (!url.trim()) return;
    setBusy(true);
    setError(null);
    try {
      onAdded(await addRssFeed(url.trim()));
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal title="添加 RSS 源" onClose={onClose}>
      <div className="flex flex-col gap-3">
        <p className="text-xs leading-relaxed text-neutral-500">
          粘贴一个 RSS / Atom 地址。添加后会立即抓取一次，并自动匹配已有的追番。
        </p>
        <input
          autoFocus
          value={url}
          onChange={(event) => setUrl(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !busy) void submit();
          }}
          className={inputClass}
        />
        {error && <p className="text-xs text-rose-400">{error}</p>}
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg border border-neutral-800 px-3 py-1.5 text-xs text-neutral-400 hover:bg-neutral-900"
          >
            取消
          </button>
          <button
            type="button"
            onClick={() => void submit()}
            disabled={busy || !url.trim()}
            className="flex items-center gap-1.5 rounded-lg bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-500 disabled:opacity-40"
          >
            {busy && <Loader2 size={12} className="animate-spin" />}
            添加
          </button>
        </div>
      </div>
    </Modal>
  );
}

function AddFollowDialog({
  feeds,
  initial,
  defaultSavePath,
  defaultSavePathExists,
  onClose,
  onAdded,
}: {
  feeds: RssFeed[];
  initial?: ParsedReleaseTitle | null;
  defaultSavePath: string;
  defaultSavePathExists: boolean;
  onClose: () => void;
  onAdded: (follow: RssFollow) => void;
}) {
  const initialResolution =
    initial?.resolution &&
    ["1080p", "720p", "2160p"].includes(initial.resolution)
      ? initial.resolution
      : "1080p";
  const [feedId, setFeedId] = useState(feeds[0]?.id ?? 0);
  const [title, setTitle] = useState(initial?.title ?? initial?.raw ?? "");
  const [aliases, setAliases] = useState(
    initial?.titleCandidates
      .filter((candidate) => candidate !== initial?.title)
      .join("\n") ?? "",
  );
  const [preferredGroups, setPreferredGroups] = useState(initial?.group ?? "");
  const [subtitlePackaging, setSubtitlePackaging] = useState(
    initial?.subtitlePackaging ?? "",
  );
  const [resolution, setResolution] = useState(initialResolution);
  const [savePath, setSavePath] = useState(defaultSavePath);
  const [autoDownload, setAutoDownload] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!savePath && defaultSavePath) setSavePath(defaultSavePath);
  }, [defaultSavePath, savePath]);

  const pickDirectory = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "选择这部番的保存目录",
      });
      if (typeof selected === "string") setSavePath(selected);
    } catch (reason) {
      setError(errorMessage(reason));
    }
  };

  const submit = async () => {
    if (!title.trim() || !feedId) return;
    setBusy(true);
    setError(null);
    try {
      onAdded(
        await addRssFollow({
          feedId,
          title: title.trim(),
          aliases,
          preferredGroups,
          subtitlePackaging,
          resolution,
          savePath: savePath.trim() || defaultSavePath,
          autoDownload,
        }),
      );
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal title="添加追番" onClose={onClose}>
      <div className="flex flex-col gap-3">
        {initial && (
          <div className="rounded-lg border border-indigo-900/60 bg-indigo-950/25 px-3 py-2 text-[11px] text-neutral-300">
            <div className="flex items-center justify-between gap-2">
              <span className="font-medium text-indigo-300">
                已从选中文字识别
              </span>
              <span className="text-[10px] text-neutral-500">
                可信度 {initial.confidence}%
              </span>
            </div>
            <div className="mt-1 text-[10px] leading-relaxed text-neutral-500">
              字幕组：{initial.group || "未识别"} · 第{" "}
              {initial.episode ?? "?"} 集 ·{" "}
              {initial.resolution || "未知分辨率"}
              {initial.subtitlePackaging
                ? ` · ${initial.subtitlePackaging}`
                : ""}
              {initial.subtitle ? ` · ${initial.subtitle}` : ""}
            </div>
            {initial.titleCandidates.length > 1 && (
              <div className="mt-1.5 flex flex-wrap gap-1">
                {initial.titleCandidates.map((candidate) => (
                  <button
                    key={candidate}
                    type="button"
                    onClick={() => setTitle(candidate)}
                    className={`rounded border px-1.5 py-0.5 text-[10px] ${
                      title === candidate
                        ? "border-indigo-500 bg-indigo-500/20 text-indigo-200"
                        : "border-neutral-700 text-neutral-400 hover:bg-neutral-800"
                    }`}
                  >
                    {candidate}
                  </button>
                ))}
              </div>
            )}
          </div>
        )}
        <label className="flex flex-col gap-1 text-[11px] text-neutral-500">
          RSS 源
          <select
            value={feedId}
            onChange={(event) => setFeedId(Number(event.target.value))}
            className={inputClass}
          >
            {feeds.map((feed) => (
              <option key={feed.id} value={feed.id}>
                {feed.title || feed.url}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-neutral-500">
          番剧名（必须出现在发布标题中）
          <input
            autoFocus
            value={title}
            onChange={(event) => setTitle(event.target.value)}
            className={inputClass}
          />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-neutral-500">
          别名（换行或逗号分隔，可留空）
          <textarea
            value={aliases}
            onChange={(event) => setAliases(event.target.value)}
            rows={3}
            className={inputClass}
          />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-neutral-500">
          限定字幕组（按填写顺序，逗号或换行分隔，可留空）
          <input
            value={preferredGroups}
            onChange={(event) => setPreferredGroups(event.target.value)}
            className={inputClass}
          />
          <span className="text-[10px] leading-relaxed text-neutral-600">
            填写后只自动下载这些字幕组；最近匹配仍显示其它字幕组，可手动下载。
          </span>
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-neutral-500">
          字幕封装方式（可留空）
          <input
            value={subtitlePackaging}
            onChange={(event) => setSubtitlePackaging(event.target.value)}
            className={inputClass}
          />
          <span className="text-[10px] leading-relaxed text-neutral-600">
            填写后只自动下载匹配的封装方式；最近匹配不受限制。
          </span>
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-neutral-500">
          保存目录
          <div className="flex gap-2">
            <input
              value={savePath}
              onChange={(event) => setSavePath(event.target.value)}
              className={inputClass}
            />
            <button
              type="button"
              onClick={() => void pickDirectory()}
              className="flex shrink-0 items-center gap-1 rounded-lg border border-neutral-700 px-2.5 text-[11px] text-neutral-400 transition-colors hover:bg-neutral-800 hover:text-neutral-200"
            >
              <FolderOpen size={12} />
              选择
            </button>
          </div>
          <span className="text-[10px] leading-relaxed text-neutral-600">
            默认使用当前「下载到」目录，可直接修改；修改只影响之后加入下载的任务。
          </span>
          {!defaultSavePathExists && (
            <span className="text-[10px] leading-relaxed text-amber-400">
              当前「下载到」目录不存在：{defaultSavePath}。下载会失败，请先在资源页设置有效目录。
            </span>
          )}
        </label>
        <div className="grid grid-cols-2 gap-3">
          <label className="flex flex-col gap-1 text-[11px] text-neutral-500">
            优先分辨率
            <select
              value={resolution}
              onChange={(event) => setResolution(event.target.value)}
              className={inputClass}
            >
              <option value="1080p">1080p</option>
              <option value="720p">720p</option>
              <option value="2160p">2160p</option>
            </select>
          </label>
          <label className="flex items-end gap-2 pb-2 text-[11px] text-neutral-400">
            <input
              type="checkbox"
              checked={autoDownload}
              onChange={(event) => setAutoDownload(event.target.checked)}
              className="h-3.5 w-3.5 accent-indigo-500"
            />
            自动下载最新一集
          </label>
        </div>
        <p className="text-[10px] leading-relaxed text-neutral-600">
          匹配不依赖正则，标题、别名命中和集数解析都在后台完成。保存目录可以在添加时选择，之后也能在追番卡片里编辑。
        </p>
        {error && <p className="text-xs text-rose-400">{error}</p>}
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg border border-neutral-800 px-3 py-1.5 text-xs text-neutral-400 hover:bg-neutral-900"
          >
            取消
          </button>
          <button
            type="button"
            onClick={() => void submit()}
            disabled={busy || !title.trim() || !feedId}
            className="flex items-center gap-1.5 rounded-lg bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-500 disabled:opacity-40"
          >
            {busy && <Loader2 size={12} className="animate-spin" />}
            开始追番
          </button>
        </div>
      </div>
    </Modal>
  );
}

function EditFollowDialog({
  follow,
  defaultSavePath,
  defaultSavePathExists,
  onClose,
  onSaved,
}: {
  follow: RssFollow;
  defaultSavePath: string;
  defaultSavePathExists: boolean;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [title, setTitle] = useState(follow.title);
  const [aliases, setAliases] = useState(followToAliases(follow));
  const [preferredGroups, setPreferredGroups] = useState(followToGroups(follow));
  const [subtitlePackaging, setSubtitlePackaging] = useState(
    follow.subtitlePackaging,
  );
  const [resolution, setResolution] = useState(follow.resolution || "1080p");
  const [savePath, setSavePath] = useState(follow.savePath ?? defaultSavePath);
  const [autoDownload, setAutoDownload] = useState(follow.autoDownload);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!savePath && defaultSavePath) setSavePath(defaultSavePath);
  }, [defaultSavePath, savePath]);

  const pickDirectory = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "选择这部番的保存目录",
      });
      if (typeof selected === "string") setSavePath(selected);
    } catch (reason) {
      setError(errorMessage(reason));
    }
  };

  const submit = async () => {
    if (!title.trim()) return;
    setBusy(true);
    setError(null);
    try {
      await updateRssFollow({
        followId: follow.id,
        title: title.trim(),
        aliases,
        preferredGroups,
        subtitlePackaging,
        resolution,
        savePath: savePath.trim() || defaultSavePath,
        autoDownload,
        status: follow.status,
      });
      onSaved();
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal title="编辑追番" onClose={onClose}>
      <div className="flex flex-col gap-3">
        <div className="rounded-lg border border-neutral-800 bg-neutral-900/50 px-3 py-2 text-[11px] text-neutral-500">
          RSS 源：{follow.feedTitle || "未知"}
        </div>
        <label className="flex flex-col gap-1 text-[11px] text-neutral-500">
          番剧名
          <input
            autoFocus
            value={title}
            onChange={(event) => setTitle(event.target.value)}
            className={inputClass}
          />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-neutral-500">
          别名（换行或逗号分隔，可留空）
          <textarea
            value={aliases}
            onChange={(event) => setAliases(event.target.value)}
            rows={3}
            className={inputClass}
          />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-neutral-500">
          限定字幕组（按填写顺序，逗号或换行分隔，可留空）
          <input
            value={preferredGroups}
            onChange={(event) => setPreferredGroups(event.target.value)}
            className={inputClass}
          />
          <span className="text-[10px] leading-relaxed text-neutral-600">
            填写后只自动下载这些字幕组；最近匹配仍显示其它字幕组，可手动下载。
          </span>
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-neutral-500">
          字幕封装方式（可留空）
          <input
            value={subtitlePackaging}
            onChange={(event) => setSubtitlePackaging(event.target.value)}
            className={inputClass}
          />
          <span className="text-[10px] leading-relaxed text-neutral-600">
            填写后只自动下载匹配的封装方式；最近匹配不受限制。
          </span>
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-neutral-500">
          保存目录
          <div className="flex gap-2">
            <input
              value={savePath}
              onChange={(event) => setSavePath(event.target.value)}
              className={inputClass}
            />
            <button
              type="button"
              onClick={() => void pickDirectory()}
              className="flex shrink-0 items-center gap-1 rounded-lg border border-neutral-700 px-2.5 text-[11px] text-neutral-400 transition-colors hover:bg-neutral-800 hover:text-neutral-200"
            >
              <FolderOpen size={12} />
              选择
            </button>
          </div>
          <div className="flex items-center justify-between gap-2">
            <span className="text-[10px] leading-relaxed text-neutral-600">
              默认使用当前「下载到」目录。更改目录只影响之后加入下载的任务，不会移动已有文件。
            </span>
            {savePath !== defaultSavePath && (
              <button
                type="button"
                onClick={() => setSavePath(defaultSavePath)}
                className="shrink-0 text-[10px] text-neutral-500 underline-offset-2 hover:text-neutral-300 hover:underline"
              >
                恢复默认
              </button>
            )}
          </div>
          {!defaultSavePathExists && (
            <span className="text-[10px] leading-relaxed text-amber-400">
              当前「下载到」目录不存在：{defaultSavePath}。下载会失败，请先在资源页设置有效目录。
            </span>
          )}
        </label>
        <div className="grid grid-cols-2 gap-3">
          <label className="flex flex-col gap-1 text-[11px] text-neutral-500">
            优先分辨率
            <select
              value={resolution}
              onChange={(event) => setResolution(event.target.value)}
              className={inputClass}
            >
              <option value="1080p">1080p</option>
              <option value="720p">720p</option>
              <option value="2160p">2160p</option>
            </select>
          </label>
          <label className="flex items-end gap-2 pb-2 text-[11px] text-neutral-400">
            <input
              type="checkbox"
              checked={autoDownload}
              onChange={(event) => setAutoDownload(event.target.checked)}
              className="h-3.5 w-3.5 accent-indigo-500"
            />
            自动下载最新一集
          </label>
        </div>
        {error && <p className="text-xs text-rose-400">{error}</p>}
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg border border-neutral-800 px-3 py-1.5 text-xs text-neutral-400 hover:bg-neutral-900"
          >
            取消
          </button>
          <button
            type="button"
            onClick={() => void submit()}
            disabled={busy || !title.trim()}
            className="flex items-center gap-1.5 rounded-lg bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-500 disabled:opacity-40"
          >
            {busy && <Loader2 size={12} className="animate-spin" />}
            保存
          </button>
        </div>
      </div>
    </Modal>
  );
}

function Modal({
  title,
  onClose,
  children,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
}) {
  return createPortal(
    <div
      className="fixed inset-0 z-[80] flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className="max-h-[calc(100vh-2rem)] w-full max-w-md overflow-y-auto rounded-2xl border border-neutral-800 bg-neutral-950 p-5 shadow-2xl"
      >
        <header className="mb-4 flex items-center justify-between">
          <h2 className="text-sm font-medium text-neutral-100">{title}</h2>
          <button
            type="button"
            onClick={onClose}
            className="text-neutral-500 transition-colors hover:text-neutral-200"
            aria-label="关闭"
          >
            ×
          </button>
        </header>
        {children}
      </div>
    </div>,
    document.body,
  );
}
