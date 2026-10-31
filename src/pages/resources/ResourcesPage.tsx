import { useCallback, useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  ArrowDown,
  ArrowLeft,
  ArrowUp,
  Bookmark as BookmarkIcon,
  CheckCircle2,
  ExternalLink,
  Globe,
  Loader2,
  Play,
  Plus,
  RefreshCw,
  RotateCw,
  Server,
  Settings2,
  Square,
  Star,
  Trash2,
  X,
} from "lucide-react";
import { BookmarksDialog } from "../../components/bookmarks/BookmarksDialog";
import { addBookmark, listBookmarks, removeBookmark } from "../../api/bookmarks";
import type { Bookmark } from "../../api/bookmarks";
import {
  browserCloseTab,
  browserCloseUntracked,
  browserGetZoom,
  browserGoBack,
  browserHideTab,
  browserNavigateTab,
  browserOpenTab,
  browserReloadTab,
  browserSetTabBounds,
  browserSetZoom,
  browserShowTab,
  getQbittorrentConfig,
  qbittorrentListTorrents,
  qbittorrentTest,
  qbittorrentWebUiUrl,
  setQbittorrentConfig,
  setQbittorrentSavePath,
  setTorrentRunning,
} from "../../api/resources";
import type {
  BrowserNewTab,
  BrowserRect,
  BrowserTabUpdate,
  QbittorrentConfig,
  TorrentAddedEvent,
  TorrentInfo,
  TorrentListing,
} from "../../api/resources";
import { errorMessage } from "../../lib/errors";

interface BrowserTab {
  id: number;
  title: string;
  url: string;
}

interface ImageSavedEvent {
  ok: boolean;
  path: string | null;
  error: string | null;
}

interface FolderCreatedEvent {
  ok: boolean;
  path: string | null;
  error: string | null;
}

interface DownloadDirMissingEvent {
  path: string;
}

interface ToastItem {
  id: number;
  kind: "success" | "warning";
  text: string;
  detail?: string;
}

const QB_MODES = [
  ["sidecar", "内置实例"],
  ["external", "外部 WebUI"],
] as const;

/// 离开资源页后要保留的状态。组件卸载时各标签页的 WebView 由后端隐藏而不销毁，
/// 这里记录标签页列表与最近一次界面状态，返回时直接恢复。
const sessionCache = {
  address: "",
  tabs: [] as BrowserTab[],
  activeTabId: null as number | null,
  qbVersion: null as string | null,
  qbTorrents: [] as TorrentInfo[],
  savePath: "",
};

/// qBittorrent 的状态码转成简短中文，未知状态直接显示原值。
const STATE_LABELS: Record<string, string> = {
  downloading: "下载中",
  stalledDL: "等待",
  metaDL: "获取元数据",
  forcedMetaDL: "获取元数据",
  queuedDL: "排队",
  checkingDL: "校验",
  allocating: "分配空间",
  pausedDL: "暂停",
  stoppedDL: "已停止",
  uploading: "做种",
  stalledUP: "空闲",
  queuedUP: "排队",
  checkingUP: "校验",
  pausedUP: "暂停",
  stoppedUP: "已停止",
  forcedDL: "强制下载",
  forcedUP: "强制做种",
  moving: "移动中",
  checkingResumeData: "校验",
  error: "出错",
  missingFiles: "文件丢失",
  unknown: "未知",
};

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let index = 0;
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024;
    index += 1;
  }
  return `${value.toFixed(value >= 100 ? 0 : 1)} ${units[index]}`;
}

function formatSpeed(bytes: number): string {
  return `${formatBytes(bytes)}/s`;
}

/// 标签栏显示的标题。空白页没有标题，显示「新标签页」。
function tabLabel(tab: BrowserTab): string {
  if (tab.title) return tab.title;
  if (!tab.url || tab.url === "about:blank") return "新标签页";
  return tab.url;
}

/// 地址栏显示的内容。空白页的 about:blank 显示为空。
function addressFor(url: string): string {
  return !url || url === "about:blank" ? "" : url;
}

/// 这些状态下的任务视为已停止，按钮显示「启动」。
const STOPPED_TORRENT_STATES = new Set([
  "stoppedDL",
  "stoppedUP",
  "pausedDL",
  "pausedUP",
  "error",
  "missingFiles",
]);

function isTorrentStopped(state: string): boolean {
  return STOPPED_TORRENT_STATES.has(state);
}

/// 占位区域相对视口的矩形；尺寸为 0 时视为不可用。
function measure(element: HTMLElement | null): BrowserRect | null {
  if (!element) return null;
  const rect = element.getBoundingClientRect();
  if (rect.width < 1 || rect.height < 1) return null;
  return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
}

/// 返回 Windows 目录的上一级；已经是盘符/UNC 共享根时返回 null。
function parentDirectory(path: string): string | null {
  const trimmed = path.trim().replace(/[\\/]+$/, "");
  if (!trimmed) return null;

  const separator = trimmed.includes("\\") ? "\\" : "/";

  if (trimmed.startsWith("\\\\")) {
    const parts = trimmed.slice(2).split(separator).filter(Boolean);
    if (parts.length <= 2) return null;
    return `\\\\${parts.slice(0, -1).join(separator)}`;
  }

  const normalized = trimmed.replace(/[\\/]/g, separator);
  const index = normalized.lastIndexOf(separator);
  if (index < 0) return null;

  const parent = normalized.slice(0, index);
  if (!parent) return null;
  if (/^[A-Za-z]:$/.test(parent)) return `${parent}\\`;
  return parent;
}

const iconButton =
  "flex h-[38px] w-[38px] shrink-0 items-center justify-center rounded-lg border border-neutral-800 bg-neutral-900 text-neutral-400 transition-colors hover:bg-neutral-800 hover:text-neutral-200 disabled:opacity-40";

export function ResourcesPage() {
  const areaRef = useRef<HTMLDivElement>(null);
  const addressInputRef = useRef<HTMLInputElement>(null);
  // 避免上一次轮询还没返回时重复发请求。
  const pollingRef = useRef(false);
  const pollFailuresRef = useRef(0);
  const autoConnectRef = useRef(false);
  const mountedRef = useRef(true);
  const savePathRef = useRef(sessionCache.savePath);
  // 正在切换下载目录时暂停轮询，避免旧目录的任务又被刷回来。
  const pathChangingRef = useRef(false);
  // 目录切换一代，丢弃切换前发出、切换后才返回的旧列表。
  const listingEpochRef = useRef(0);
  // 启动/停止后的乐观状态，服务端状态追上来之前覆盖轮询结果。
  const optimisticStatesRef = useRef(
    new Map<string, { state: string; stopped: boolean }>(),
  );
  const activeTabIdRef = useRef<number | null>(sessionCache.activeTabId);
  const tabsRef = useRef<BrowserTab[]>(sessionCache.tabs);
  const newTabRef = useRef<(url?: string) => Promise<void>>(async () => {});
  const lastNewTabRef = useRef<{ url: string; at: number } | null>(null);

  const [address, setAddress] = useState(sessionCache.address);
  const [tabs, setTabs] = useState<BrowserTab[]>(sessionCache.tabs);
  const [activeTabId, setActiveTabId] = useState<number | null>(
    sessionCache.activeTabId,
  );
  // 只有恢复完成（或首次打开）后才开始同步边界，避免页面切换动画期间
  // 把隐藏的 webview 定位到动画中的 transform 矩形上。
  const [browserVisible, setBrowserVisible] = useState(false);
  const [browserBusy, setBrowserBusy] = useState(false);
  const [browserError, setBrowserError] = useState<string | null>(null);
  const [zoomPercent, setZoomPercent] = useState(80);

  const [bookmarks, setBookmarks] = useState<Bookmark[]>([]);
  const [bookmarksOpen, setBookmarksOpen] = useState(false);
  const [manageOpen, setManageOpen] = useState(false);

  const [qbConfig, setQbConfig] = useState<QbittorrentConfig>({
    mode: "sidecar",
    url: "http://127.0.0.1:8080",
    username: "admin",
    password: "",
    savePath: null,
    advanced: false,
  });
  const [qbVersion, setQbVersion] = useState<string | null>(
    sessionCache.qbVersion,
  );
  const [qbTorrents, setQbTorrents] = useState<TorrentInfo[]>(
    sessionCache.qbTorrents,
  );
  const [qbBusy, setQbBusy] = useState(false);
  const [qbError, setQbError] = useState<string | null>(null);
  const qbErrorRef = useRef<string | null>(null);
  const [savePath, setSavePath] = useState(sessionCache.savePath);
  // 正在启动/停止的任务 hash，按钮显示转圈并禁用。
  const [pendingHashes, setPendingHashes] = useState<Set<string>>(new Set());
  const [toast, setToast] = useState<ToastItem | null>(null);
  const [leavingToast, setLeavingToast] = useState<ToastItem | null>(null);
  const toastRef = useRef<ToastItem | null>(null);
  const toastAutoTimerRef = useRef<number | undefined>(undefined);
  const toastExitTimerRef = useRef<number | undefined>(undefined);
  const toastIdRef = useRef(0);

  const beginToastExit = (item: ToastItem) => {
    if (toastExitTimerRef.current !== undefined) {
      window.clearTimeout(toastExitTimerRef.current);
    }
    setLeavingToast(item);
    toastExitTimerRef.current = window.setTimeout(() => {
      setLeavingToast(null);
      toastExitTimerRef.current = undefined;
    }, 200);
  };

  const dismissToast = () => {
    const current = toastRef.current;
    if (!current) return;
    if (toastAutoTimerRef.current !== undefined) {
      window.clearTimeout(toastAutoTimerRef.current);
      toastAutoTimerRef.current = undefined;
    }
    toastRef.current = null;
    setToast(null);
    beginToastExit(current);
  };

  const showToast = (
    kind: ToastItem["kind"],
    text: string,
    detail?: string,
  ) => {
    if (toastAutoTimerRef.current !== undefined) {
      window.clearTimeout(toastAutoTimerRef.current);
      toastAutoTimerRef.current = undefined;
    }
    const current = toastRef.current;
    if (current) beginToastExit(current);

    const next: ToastItem = {
      id: ++toastIdRef.current,
      kind,
      text,
      detail,
    };
    toastRef.current = next;
    setToast(next);

    if (kind === "success") {
      toastAutoTimerRef.current = window.setTimeout(() => {
        toastAutoTimerRef.current = undefined;
        dismissToast();
      }, 3200);
    }
  };

  const showSuccessToast = (text: string) => showToast("success", text);
  const reportQbError = (reason: unknown) => {
    const text = errorMessage(reason);
    setQbError(text);
    if (qbErrorRef.current !== text) {
      qbErrorRef.current = text;
      showToast("warning", text);
    }
  };
  const clearQbError = () => {
    qbErrorRef.current = null;
    setQbError(null);
  };
  const showDirWarning = (path: string) =>
    showToast("warning", "选中的下载目录不存在", path);

  useEffect(() => {
    return () => {
      if (toastAutoTimerRef.current !== undefined) {
        window.clearTimeout(toastAutoTimerRef.current);
      }
      if (toastExitTimerRef.current !== undefined) {
        window.clearTimeout(toastExitTimerRef.current);
      }
    };
  }, []);

  const hasTabs = tabs.length > 0;
  const activeTab = tabs.find((tab) => tab.id === activeTabId) ?? null;
  const canBookmark =
    !!activeTab?.url && activeTab.url !== "about:blank";
  // 收藏夹菜单/弹窗是 HTML 层，会被原生 webview 盖住，打开时先把当前页隐藏。
  const overlayOpen = bookmarksOpen || manageOpen;

  // 离开资源页时只隐藏各标签页的原生 webview，保留页面状态。
  // 同时关掉界面状态里没有的孤儿 webview，避免它们一直盖在其他页面上。
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      const active = activeTabIdRef.current;
      if (active != null) void browserHideTab(active).catch(() => {});
      void browserCloseUntracked(tabsRef.current.map((tab) => tab.id)).catch(
        () => {},
      );
    };
  }, []);

  // 进入资源页时清理上次遗留的孤儿 webview。
  useEffect(() => {
    void browserCloseUntracked(
      sessionCache.tabs.map((tab) => tab.id),
    ).catch(() => {});
  }, []);

  // 回到资源页时恢复当前标签页。等页面切换动画（最长 360ms）结束再定位，
  // 否则 measure 拿到的是动画中的矩形。
  useEffect(() => {
    if (sessionCache.activeTabId == null) return;

    let disposed = false;
    const timer = window.setTimeout(() => {
      const rect = measure(areaRef.current);
      const tabId = sessionCache.activeTabId;
      if (!rect || tabId == null) return;

      void browserShowTab(tabId, rect)
        .then((exists) => {
          if (disposed) return;
          if (exists) {
            setBrowserVisible(true);
            return;
          }

          // 后端已经没有这些 webview（例如异常退出），清空标签页，
          // 并顺手回收所有没人跟踪的 webview。
          setTabs([]);
          setActiveTabId(null);
          sessionCache.tabs = [];
          sessionCache.activeTabId = null;
          void browserCloseUntracked([]).catch(() => {});
        })
        .catch(() => {});
    }, 380);

    return () => {
      disposed = true;
      window.clearTimeout(timer);
    };
  }, []);

  // 界面状态写回缓存，供下次进入恢复。
  useEffect(() => {
    sessionCache.address = address;
  }, [address]);

  useEffect(() => {
    sessionCache.tabs = tabs;
    sessionCache.activeTabId = activeTabId;
    tabsRef.current = tabs;
    activeTabIdRef.current = activeTabId;
  }, [tabs, activeTabId]);

  useEffect(() => {
    sessionCache.qbVersion = qbVersion;
    sessionCache.qbTorrents = qbTorrents;
  }, [qbVersion, qbTorrents]);

  useEffect(() => {
    sessionCache.savePath = savePath;
  }, [savePath]);

  // 布局或滚动变化时同步当前标签页位置。原生窗口不会跟着 DOM 移动。
  useEffect(() => {
    if (!browserVisible || activeTabId == null || overlayOpen) return;

    const sync = () => {
      const rect = measure(areaRef.current);
      if (rect) void browserSetTabBounds(activeTabId, rect).catch(() => {});
    };

    sync();
    const observer = new ResizeObserver(sync);
    if (areaRef.current) observer.observe(areaRef.current);
    window.addEventListener("resize", sync);
    // AppShell 的 main 容器会滚动，捕获阶段才能收到。
    window.addEventListener("scroll", sync, true);

    return () => {
      observer.disconnect();
      window.removeEventListener("resize", sync);
      window.removeEventListener("scroll", sync, true);
    };
  }, [browserVisible, activeTabId, overlayOpen]);

  // 收藏夹菜单/弹窗打开时隐藏当前页，关闭时按当前布局恢复。
  useEffect(() => {
    if (activeTabId == null || !browserVisible) return;
    const rect = measure(areaRef.current);
    if (!rect) return;

    if (overlayOpen) {
      void browserHideTab(activeTabId).catch(() => {});
    } else {
      void browserShowTab(activeTabId, rect).catch(() => {});
    }
  }, [overlayOpen, activeTabId, browserVisible]);

  // 标签页标题、URL 变化与 target=_blank 请求。
  useEffect(() => {
    let disposed = false;
    let unlistenUpdate: UnlistenFn | undefined;
    let unlistenNew: UnlistenFn | undefined;
    let unlistenImage: UnlistenFn | undefined;
    let unlistenFolder: UnlistenFn | undefined;
    let unlistenMissingDir: UnlistenFn | undefined;

    void listen<BrowserTabUpdate>("browser-tab-updated", (event) => {
      const { tabId, title, url } = event.payload;
      setTabs((prev) =>
        prev.map((tab) =>
          tab.id === tabId
            ? {
                ...tab,
                title: title ?? tab.title,
                url: url || tab.url,
              }
            : tab,
        ),
      );

      if (
        tabId === activeTabIdRef.current &&
        url &&
        url !== "about:blank" &&
        document.activeElement !== addressInputRef.current
      ) {
        setAddress(url);
      }
    }).then((fn) => {
      if (disposed) fn();
      else unlistenUpdate = fn;
    });

    void listen<BrowserNewTab>("browser-new-tab", (event) => {
      const url = event.payload.url;
      const now = Date.now();
      // 原生 on_new_window 和网页点击兜底可能同时到达，短时间内同一
      // 地址只开一个标签页。
      if (
        lastNewTabRef.current?.url === url &&
        now - lastNewTabRef.current.at < 800
      ) {
        return;
      }
      lastNewTabRef.current = { url, at: now };
      void newTabRef.current(url);
    }).then((fn) => {
      if (disposed) fn();
      else unlistenNew = fn;
    });

    void listen<ImageSavedEvent>("browser-image-saved", (event) => {
      if (event.payload.ok) {
        showSuccessToast("图片已保存到「下载到」目录");
      }
    }).then((fn) => {
      if (disposed) fn();
      else unlistenImage = fn;
    });

    void listen<FolderCreatedEvent>("browser-folder-created", (event) => {
      const { ok, path } = event.payload;
      if (ok) {
        showSuccessToast("番剧目录及配置文件已创建");
      }
      if (ok && path) {
        // 新建番剧目录后把 qBittorrent 的「下载到」切到该目录。
        void changeSavePath(path);
      }
    }).then((fn) => {
      if (disposed) fn();
      else unlistenFolder = fn;
    });

    void listen<DownloadDirMissingEvent>("download-dir-missing", (event) => {
      showDirWarning(event.payload.path);
    }).then((fn) => {
      if (disposed) fn();
      else unlistenMissingDir = fn;
    });

    return () => {
      disposed = true;
      unlistenUpdate?.();
      unlistenNew?.();
      unlistenImage?.();
      unlistenFolder?.();
      unlistenMissingDir?.();
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    void listBookmarks()
      .then((items) => {
        if (!disposed) setBookmarks(items);
      })
      .catch(() => {
        // 读取失败时收藏夹为空，写操作会再次提示。
      });
    return () => {
      disposed = true;
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    void browserGetZoom()
      .then((value) => {
        if (!disposed) setZoomPercent(value);
      })
      .catch(() => {
        // 读取失败时沿用默认 80%。
      });
    return () => {
      disposed = true;
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    void getQbittorrentConfig()
      .then((config) => {
        if (disposed) return;
        setQbConfig(config);
        setSavePath(config.savePath ?? "");
      })
      .catch(() => {
        // 读取失败时保留默认值，连接时再提示。
      });
    return () => {
      disposed = true;
    };
  }, []);

  // 后端返回的目录和任务一起更新；目录变化时先清空旧列表。
  const applyListing = useCallback((listing: TorrentListing) => {
    if (listing.savePath !== savePathRef.current) {
      savePathRef.current = listing.savePath;
      setSavePath(listing.savePath);
      setQbTorrents([]);
    }

    // 用乐观状态覆盖服务端的旧状态，避免刚点完按钮又被轮询翻回去。
    const items = listing.torrents.map((torrent) => {
      const override = optimisticStatesRef.current.get(torrent.hash);
      return override ? { ...torrent, state: override.state } : torrent;
    });

    // 服务端已经反映操作后，移除覆盖，改用真实状态。
    for (const [hash, override] of [...optimisticStatesRef.current]) {
      const current = listing.torrents.find((torrent) => torrent.hash === hash);
      if (
        !current ||
        isTorrentStopped(current.state) === override.stopped
      ) {
        optimisticStatesRef.current.delete(hash);
      }
    }

    setQbTorrents(items);
  }, []);

  const refresh = useCallback(async () => {
    pollFailuresRef.current = 0;
    const status = await qbittorrentTest();
    setQbVersion(status.version);
    applyListing(await qbittorrentListTorrents());
    clearQbError();
  }, [applyListing]);

  // 资源页可见时每 2 秒刷新种子进度。连续失败时把按钮切回未连接状态，
  // 让用户能看出连接已经断开并重试。
  const pollTorrents = useCallback(async () => {
    if (pollingRef.current || pathChangingRef.current) return;
    pollingRef.current = true;
    const epoch = listingEpochRef.current;
    try {
      const listing = await qbittorrentListTorrents();
      // 请求期间切换了目录：这是旧目录的结果，直接丢弃。
      if (epoch !== listingEpochRef.current) return;
      applyListing(listing);
      pollFailuresRef.current = 0;
    } catch (error) {
      pollFailuresRef.current += 1;
      if (pollFailuresRef.current >= 3) {
        setQbVersion(null);
        reportQbError(error);
      }
    } finally {
      pollingRef.current = false;
    }
  }, [applyListing]);

  // 进入资源页自动连接；切回来时已经有会话就只做后台刷新。
  const autoConnect = useCallback(async () => {
    const alreadyConnected = sessionCache.qbVersion != null;
    if (!alreadyConnected) setQbBusy(true);
    clearQbError();
    try {
      await refresh();
    } catch (error) {
      setQbVersion(null);
      setQbTorrents([]);
      reportQbError(error);
    } finally {
      if (!alreadyConnected) setQbBusy(false);
    }
  }, [refresh]);

  useEffect(() => {
    if (autoConnectRef.current) return;
    autoConnectRef.current = true;
    void autoConnect();
  }, [autoConnect]);

  useEffect(() => {
    if (!qbVersion) return;

    const tick = () => {
      if (document.visibilityState === "visible") void pollTorrents();
    };

    const timer = window.setInterval(tick, 2000);
    return () => window.clearInterval(timer);
  }, [qbVersion, pollTorrents]);

  // 浏览器里点磁力链接或 .torrent 链接后，后端添加完成会发事件。
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let disposed = false;

    void listen<TorrentAddedEvent>("qbittorrent-torrent", (event) => {
      const { ok, kind, error } = event.payload;
      // 目录不存在已经由 download-dir-missing 事件用右下角红色 Toast 提示，
      // 不再在右侧卡片重复显示红色文字。
      if (!ok && error?.includes("选中的下载目录不存在")) return;
      showToast(
        ok ? "success" : "warning",
        ok
          ? kind === "magnet"
            ? "已添加磁力链接"
            : "已添加种子链接"
          : `添加失败：${error ?? "未知错误"}`,
      );

      if (ok) {
        void refresh().catch((reason) => reportQbError(reason));
      }
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [refresh]);

  const newTab = async (target: string = "") => {
    const rect = measure(areaRef.current);
    if (!rect) return;

    setBrowserBusy(true);
    setBrowserError(null);
    try {
      const previous = activeTabId;
      const id = await browserOpenTab(target, rect);

      // 新 webview 建好之后再隐藏旧标签：创建失败时原来的页面不会消失。
      if (previous != null && previous !== id) {
        await browserHideTab(previous).catch(() => {});
      }

      // 创建过程中用户已经离开资源页：创建完成后立即隐藏。
      if (!mountedRef.current) {
        await browserHideTab(id).catch(() => {});
        return;
      }

      setTabs((prev) => [...prev, { id, title: "", url: target }]);
      setActiveTabId(id);
      activeTabIdRef.current = id;
      setAddress(target);
      setBrowserVisible(true);
    } catch (error) {
      setBrowserError(errorMessage(error));
    } finally {
      setBrowserBusy(false);
    }
  };

  useEffect(() => {
    newTabRef.current = newTab;
  });

  const selectTab = async (id: number) => {
    if (id === activeTabId) return;

    const rect = measure(areaRef.current);
    if (!rect) return;

    try {
      if (activeTabId != null) {
        await browserHideTab(activeTabId).catch(() => {});
      }

      const exists = await browserShowTab(id, rect).catch(() => false);
      if (!exists) {
        const remaining = tabs.filter((tab) => tab.id !== id);
        setTabs(remaining);
        void browserCloseUntracked(remaining.map((tab) => tab.id)).catch(
          () => {},
        );
        return;
      }

      setActiveTabId(id);
      const tab = tabs.find((item) => item.id === id);
      if (tab) setAddress(addressFor(tab.url));
      setBrowserVisible(true);
    } catch (error) {
      setBrowserError(errorMessage(error));
    }
  };

  const closeTab = async (id: number) => {
    await browserCloseTab(id).catch(() => {});

    const remaining = tabs.filter((tab) => tab.id !== id);
    setTabs(remaining);
    // close 失败或状态不一致时，确保后端没有留下这个标签页的 webview。
    void browserCloseUntracked(remaining.map((tab) => tab.id)).catch(() => {});
    if (id !== activeTabId) return;

    const next = remaining[remaining.length - 1] ?? null;
    setActiveTabId(next?.id ?? null);
    activeTabIdRef.current = next?.id ?? null;

    if (next) {
      const rect = measure(areaRef.current);
      if (rect && (await browserShowTab(next.id, rect).catch(() => false))) {
        setAddress(addressFor(next.url));
        setBrowserVisible(true);
      }
    } else {
      setBrowserVisible(false);
      setAddress("");
    }
  };

  const goBack = () => {
    if (activeTabId == null) return;
    void browserGoBack(activeTabId).catch((error) =>
      setBrowserError(errorMessage(error)),
    );
  };

  const reloadTab = () => {
    if (activeTabId == null) return;
    void browserReloadTab(activeTabId).catch((error) =>
      setBrowserError(errorMessage(error)),
    );
  };

  const applyZoom = async (percent: number) => {
    const next = Math.max(30, Math.min(150, Math.round(percent)));
    try {
      const applied = await browserSetZoom(next);
      setZoomPercent(applied);
    } catch (error) {
      setBrowserError(errorMessage(error));
    }
  };

  const goTo = async (target: string) => {
    const normalized = target.trim();
    if (!normalized) return;

    const rect = measure(areaRef.current);
    if (!rect) return;

    setBrowserBusy(true);
    setBrowserError(null);
    try {
      if (activeTabId == null || tabs.length === 0) {
        await newTab(normalized);
        return;
      }

      await browserNavigateTab(activeTabId, normalized);
      setAddress(normalized);
      setBrowserVisible(true);
    } catch (error) {
      setBrowserError(errorMessage(error));
    } finally {
      setBrowserBusy(false);
    }
  };

  const submitAddress = (event: FormEvent) => {
    event.preventDefault();
    const target = address.trim();
    if (!target) {
      // 空地址：没有标签页时新建一个空白页，否则保持当前页不动。
      if (tabs.length === 0) void newTab();
      return;
    }
    void goTo(target);
  };

  const bookmarkCurrent = async () => {
    const tab = tabs.find((item) => item.id === activeTabId);
    if (!tab?.url || tab.url === "about:blank") return;

    try {
      setBookmarks(await addBookmark(tab.title, tab.url));
      showSuccessToast("已收藏");
    } catch (error) {
      showToast("warning", errorMessage(error));
    }
  };

  const openBookmark = async (url: string) => {
    setBookmarksOpen(false);
    await goTo(url);
  };

  const deleteBookmark = async (id: number) => {
    try {
      setBookmarks(await removeBookmark(id));
    } catch (error) {
      showToast("warning", errorMessage(error));
    }
  };

  const connect = async () => {
    setQbBusy(true);
    clearQbError();
    try {
      await setQbittorrentConfig(
        qbConfig.mode,
        qbConfig.url,
        qbConfig.username,
        qbConfig.password,
      );
      await refresh();
    } catch (error) {
      setQbVersion(null);
      setQbTorrents([]);
      reportQbError(error);
    } finally {
      setQbBusy(false);
    }
  };

  const toggleTorrent = async (torrent: TorrentInfo) => {
    const start = isTorrentStopped(torrent.state);

    setPendingHashes((prev) => new Set(prev).add(torrent.hash));
    // 先乐观更新按钮和状态，qBittorrent 实际状态稍后由轮询校正。
    const optimistic = start
      ? torrent.progress >= 1
        ? "queuedUP"
        : "queuedDL"
      : torrent.progress >= 1
        ? "stoppedUP"
        : "stoppedDL";
    optimisticStatesRef.current.set(torrent.hash, {
      state: optimistic,
      stopped: !start,
    });
    setQbTorrents((prev) =>
      prev.map((item) =>
        item.hash === torrent.hash ? { ...item, state: optimistic } : item,
      ),
    );

    try {
      await setTorrentRunning(torrent.hash, start);
      window.setTimeout(() => void pollTorrents(), 600);
    } catch (error) {
      optimisticStatesRef.current.delete(torrent.hash);
      const text = errorMessage(error);
      if (!text.includes("选中的下载目录不存在")) {
        showToast("warning", text);
      }
      void pollTorrents();
    } finally {
      setPendingHashes((prev) => {
        const next = new Set(prev);
        next.delete(torrent.hash);
        return next;
      });
    }
  };

  const changeSavePath = async (selected: string) => {
    // 换目录时立即清空旧列表，暂停轮询并作废在途请求，
    // 等新目录读取完成后由 pollTorrents 填充。
    pathChangingRef.current = true;
    listingEpochRef.current += 1;
    setQbTorrents([]);

    try {
      const preferences = await setQbittorrentSavePath(selected);
      savePathRef.current = preferences.savePath;
      setSavePath(preferences.savePath);
      // 用户改好了下载目录，旧的目录不存在警告自动收起。
      if (toastRef.current?.kind === "warning") dismissToast();
      pathChangingRef.current = false;
      void pollTorrents();
    } catch (error) {
      pathChangingRef.current = false;
      reportQbError(error);
    }
  };

  const chooseSavePath = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "选择 qBittorrent 下载目录",
      });
      if (typeof selected !== "string") return;
      await changeSavePath(selected);
    } catch (error) {
      reportQbError(error);
    }
  };

  const goUpSavePath = () => {
    const parent = parentDirectory(savePath);
    if (parent) void changeSavePath(parent);
  };

  // 在内嵌浏览器的新标签页里打开 qBittorrent 自带的 WebUI。
  const openWebUi = async () => {
    try {
      const url = await qbittorrentWebUiUrl();
      await newTab(url);
      // 打开 WebUI 的同时同步一次面板状态。
      void refresh().catch(() => {});
    } catch (error) {
      reportQbError(error);
    }
  };

  const renderToast = (item: ToastItem, leaving: boolean) => {
    const success = item.kind === "success";
    return (
      <div
        key={item.id}
        className={`${leaving ? "kf-toast-out" : "kf-toast"} relative rounded-lg border px-3 py-2 text-[11px] leading-relaxed shadow-xl ${
          success
            ? "border-emerald-700/70 bg-emerald-950/95 text-emerald-200"
            : "border-rose-800/70 bg-rose-950/95 pr-7 text-rose-200"
        }`}
      >
        <div className="flex items-start gap-2">
          {success ? (
            <CheckCircle2 size={13} className="mt-0.5 shrink-0 text-emerald-400" />
          ) : (
            <AlertTriangle size={13} className="mt-0.5 shrink-0 text-rose-400" />
          )}
          <div className="min-w-0">
            <p className={success ? "break-words" : "font-medium text-rose-100"}>
              {item.text}
            </p>
            {item.detail && (
              <p className="mt-1 break-all font-mono text-[10px] text-rose-300/90">
                {item.detail}
              </p>
            )}
          </div>
        </div>
        {!success && (
          <button
            type="button"
            onClick={dismissToast}
            aria-label="关闭提示"
            className="absolute right-1.5 top-1.5 flex h-5 w-5 items-center justify-center rounded text-rose-400 transition-colors hover:bg-rose-900/60 hover:text-rose-200"
          >
            <X size={12} />
          </button>
        )}
      </div>
    );
  };

  return (
    <div className="flex h-[calc(100vh-3rem)] min-h-0 flex-col gap-4">
      <header className="flex shrink-0 items-start justify-between gap-4">
        <h1 className="text-2xl font-semibold">资源</h1>
      </header>

      <form
        onSubmit={submitAddress}
        className="relative flex shrink-0 items-center gap-2"
      >
        <div className="flex shrink-0 items-center gap-1">
          <button
            type="button"
            onClick={goBack}
            disabled={!hasTabs}
            title="后退"
            className={iconButton}
          >
            <ArrowLeft size={15} />
          </button>
          <button
            type="button"
            onClick={reloadTab}
            disabled={!hasTabs}
            title="刷新"
            className={iconButton}
          >
            <RotateCw size={14} />
          </button>
        </div>

        <div className="relative min-w-0 flex-1">
          <Globe
            size={14}
            className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-neutral-500"
          />
          <input
            ref={addressInputRef}
            value={address}
            onChange={(event) => setAddress(event.target.value)}
            aria-label="地址栏"
            className="w-full rounded-lg border border-neutral-800 bg-neutral-950 py-2 pl-9 pr-3 text-sm text-neutral-200 placeholder:text-neutral-600 focus:border-indigo-500/60 focus:outline-none"
          />
        </div>

        <button
          type="submit"
          disabled={browserBusy}
          className="flex h-[38px] shrink-0 items-center gap-2 rounded-lg border border-neutral-800 bg-neutral-900 px-4 text-sm text-neutral-300 transition-colors hover:bg-neutral-800 disabled:opacity-60"
        >
          {browserBusy ? (
            <Loader2 size={14} className="animate-spin" />
          ) : (
            <Globe size={14} />
          )}
          {hasTabs ? "前往" : "打开"}
        </button>

        <div className="flex shrink-0 items-center gap-1 rounded-lg border border-neutral-800 bg-neutral-900 px-1">
          <button
            type="button"
            onClick={() => void applyZoom(zoomPercent - 10)}
            title="缩小网页"
            className="flex h-7 w-7 items-center justify-center rounded text-sm text-neutral-400 transition-colors hover:bg-neutral-800 hover:text-neutral-200"
          >
            −
          </button>
          <span className="w-10 text-center text-[11px] tabular-nums text-neutral-400">
            {zoomPercent}%
          </span>
          <button
            type="button"
            onClick={() => void applyZoom(zoomPercent + 10)}
            title="放大网页"
            className="flex h-7 w-7 items-center justify-center rounded text-sm text-neutral-400 transition-colors hover:bg-neutral-800 hover:text-neutral-200"
          >
            +
          </button>
        </div>

        <div className="flex shrink-0 items-center gap-1">
          <button
            type="button"
            onClick={() => void bookmarkCurrent()}
            disabled={!canBookmark}
            title="收藏当前页"
            className={iconButton}
          >
            <Star size={14} />
          </button>
          <button
            type="button"
            onClick={() => setBookmarksOpen((value) => !value)}
            title="收藏夹"
            className={iconButton}
          >
            <BookmarkIcon size={14} />
          </button>
          <button
            type="button"
            onClick={() => setManageOpen(true)}
            title="管理收藏夹"
            className={iconButton}
          >
            <Settings2 size={14} />
          </button>
        </div>

        {bookmarksOpen && (
          <>
            <div
              className="fixed inset-0 z-40"
              onPointerDown={() => setBookmarksOpen(false)}
            />
            <div className="absolute right-0 top-full z-50 mt-2 flex max-h-[70vh] w-80 flex-col overflow-hidden rounded-xl border border-neutral-800 bg-neutral-950 shadow-2xl">
              <div className="flex items-center justify-between border-b border-neutral-800 px-3 py-2 text-[11px] text-neutral-500">
                <span>收藏夹</span>
                <span className="tabular-nums">{bookmarks.length} 条</span>
              </div>

              {bookmarks.length === 0 ? (
                <p className="px-3 py-5 text-center text-xs text-neutral-600">
                  还没有收藏。浏览网页时点工具栏的 ★。
                </p>
              ) : (
                <div className="min-h-0 flex-1 overflow-y-auto p-1.5">
                  {bookmarks.map((bookmark) => (
                    <div
                      key={bookmark.id}
                      className="group flex items-center gap-1 rounded-lg px-2 py-1.5 transition-colors hover:bg-neutral-900"
                    >
                      <button
                        type="button"
                        onClick={() => void openBookmark(bookmark.url)}
                        className="min-w-0 flex-1 text-left"
                      >
                        <div className="truncate text-xs text-neutral-200">
                          {bookmark.title}
                        </div>
                        <div className="truncate text-[10px] text-neutral-500">
                          {bookmark.url}
                        </div>
                      </button>
                      <button
                        type="button"
                        onClick={() => void deleteBookmark(bookmark.id)}
                        title="删除收藏"
                        className="shrink-0 rounded p-1 text-neutral-600 opacity-0 transition-opacity hover:text-rose-300 group-hover:opacity-100"
                      >
                        <Trash2 size={12} />
                      </button>
                    </div>
                  ))}
                </div>
              )}

              <button
                type="button"
                onClick={() => {
                  setBookmarksOpen(false);
                  setManageOpen(true);
                }}
                className="flex items-center gap-2 border-t border-neutral-800 px-3 py-2 text-[11px] text-neutral-400 transition-colors hover:bg-neutral-900 hover:text-neutral-200"
              >
                <Settings2 size={12} />
                管理收藏夹…
              </button>
            </div>
          </>
        )}
      </form>

      <div className="flex min-h-0 flex-1 gap-4">
        <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-2">
          {/* 标签栏 */}
          <div className="flex shrink-0 items-center gap-1 rounded-lg border border-neutral-800 bg-neutral-900/60 px-1 py-1">
            <div className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto">
              {tabs.length === 0 ? (
                <span className="px-2 text-[11px] text-neutral-600">
                  没有打开的标签页
                </span>
              ) : (
                tabs.map((tab) => {
                  const active = tab.id === activeTabId;
                  return (
                    <div
                      key={tab.id}
                      data-tab-id={tab.id}
                      className={`group flex min-w-[64px] max-w-[220px] flex-1 basis-0 items-center gap-1 rounded-md px-2 py-1 text-xs transition-colors ${
                        active
                          ? "bg-neutral-800 text-neutral-100"
                          : "text-neutral-400 hover:bg-neutral-800/60"
                      }`}
                    >
                      <button
                        type="button"
                        onClick={() => void selectTab(tab.id)}
                        className="min-w-0 flex-1 truncate text-left"
                        title={tabLabel(tab)}
                      >
                        {tabLabel(tab)}
                      </button>
                      <button
                        type="button"
                        onClick={() => void closeTab(tab.id)}
                        title="关闭标签页"
                        className="shrink-0 rounded p-0.5 text-neutral-500 opacity-0 transition-opacity hover:text-neutral-100 group-hover:opacity-100"
                      >
                        <X size={11} />
                      </button>
                    </div>
                  );
                })
              )}
            </div>

            <button
              type="button"
              onClick={() => void newTab()}
              title="新建标签页"
              className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-neutral-400 transition-colors hover:bg-neutral-800 hover:text-neutral-100"
            >
              <Plus size={13} />
            </button>
          </div>

          <div
            ref={areaRef}
            data-browser-area
            className={`relative flex min-h-0 min-w-0 flex-1 items-center justify-center overflow-hidden ${
              hasTabs && !overlayOpen
                ? "rounded-xl border border-neutral-800 bg-neutral-900"
                : "bg-transparent"
            }`}
          >
            {(!hasTabs || overlayOpen) && (
              <div className="flex flex-col items-center gap-3">
                <Globe
                  size={72}
                  strokeWidth={1.2}
                  className="text-neutral-700"
                  aria-hidden="true"
                />
                {browserError && (
                  <p className="max-w-md px-6 text-center text-xs text-rose-400">
                    {browserError}
                  </p>
                )}
              </div>
            )}
          </div>
        </div>

        <aside className="flex w-60 shrink-0 flex-col gap-3 overflow-hidden rounded-xl border border-neutral-800 bg-neutral-900/60 p-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2 text-sm font-medium">
              <Server size={15} className="text-indigo-400" />
              qBittorrent
            </div>
            <div className="flex items-center gap-1.5">
              <button
                type="button"
                onClick={() => void openWebUi()}
                title="在内嵌浏览器打开 qBittorrent WebUI"
                className="flex items-center gap-1 rounded border border-neutral-700 px-1.5 py-0.5 text-[10px] text-neutral-400 transition-colors hover:bg-neutral-800 hover:text-neutral-200"
              >
                <ExternalLink size={10} />
                WebUI
              </button>
              {qbVersion && (
                <span className="rounded bg-indigo-500/15 px-2 py-0.5 text-[11px] text-indigo-300">
                  v{qbVersion}
                </span>
              )}
            </div>
          </div>

          {qbConfig.advanced && (
            <>
              <div className="flex shrink-0 rounded-lg border border-neutral-800 bg-neutral-950 p-0.5 text-[11px]">
                {QB_MODES.map(([mode, label]) => (
                  <button
                    key={mode}
                    type="button"
                    onClick={() => setQbConfig({ ...qbConfig, mode })}
                    className={`flex-1 rounded-md px-2 py-1 transition-colors ${
                      qbConfig.mode === mode
                        ? "bg-neutral-800 text-neutral-100"
                        : "text-neutral-500 hover:text-neutral-300"
                    }`}
                  >
                    {label}
                  </button>
                ))}
              </div>

              {qbConfig.mode === "external" && (
                <div className="flex flex-col gap-2">
                  <label className="flex flex-col gap-1 text-[11px] text-neutral-500">
                    WebUI 地址
                    <input
                      value={qbConfig.url}
                      onChange={(event) =>
                        setQbConfig({ ...qbConfig, url: event.target.value })
                      }
                      className="rounded-lg border border-neutral-800 bg-neutral-950 px-2.5 py-1.5 text-xs text-neutral-200 focus:border-indigo-500/60 focus:outline-none"
                    />
                  </label>
                  <div className="flex gap-2">
                    <label className="flex min-w-0 flex-1 flex-col gap-1 text-[11px] text-neutral-500">
                      用户名
                      <input
                        value={qbConfig.username}
                        onChange={(event) =>
                          setQbConfig({
                            ...qbConfig,
                            username: event.target.value,
                          })
                        }
                        className="w-full rounded-lg border border-neutral-800 bg-neutral-950 px-2.5 py-1.5 text-xs text-neutral-200 focus:border-indigo-500/60 focus:outline-none"
                      />
                    </label>
                    <label className="flex min-w-0 flex-1 flex-col gap-1 text-[11px] text-neutral-500">
                      密码
                      <input
                        type="password"
                        value={qbConfig.password}
                        onChange={(event) =>
                          setQbConfig({
                            ...qbConfig,
                            password: event.target.value,
                          })
                        }
                        className="w-full rounded-lg border border-neutral-800 bg-neutral-950 px-2.5 py-1.5 text-xs text-neutral-200 focus:border-indigo-500/60 focus:outline-none"
                      />
                    </label>
                  </div>
                </div>
              )}
            </>
          )}

          <button
            type="button"
            onClick={() => void connect()}
            disabled={qbBusy}
            title={qbVersion ? "点击重新连接" : "启动并连接"}
            className={`flex shrink-0 items-center justify-center gap-2 rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors disabled:opacity-60 ${
              qbVersion
                ? "border-emerald-900/60 bg-emerald-600/15 text-emerald-300 hover:bg-emerald-600/25"
                : "border-transparent bg-indigo-600 text-white hover:bg-indigo-500"
            }`}
          >
            {qbBusy ? (
              <Loader2 size={13} className="animate-spin" />
            ) : qbVersion ? (
              <CheckCircle2 size={13} />
            ) : (
              <RefreshCw size={13} />
            )}
            {qbBusy
              ? "正在连接…"
              : qbVersion
                ? "已连接"
                : qbError
                  ? "重试连接"
                  : qbConfig.mode === "sidecar"
                    ? "启动并连接"
                    : "保存并连接"}
          </button>

          {qbBusy && qbConfig.mode === "sidecar" && (
            <p className="text-[11px] text-neutral-500">
              正在启动内置 qBittorrent，首次启动约需 1–2 秒…
            </p>
          )}

          <div className="flex shrink-0 items-center gap-2 rounded-lg border border-neutral-800 bg-neutral-950 px-2.5 py-2 text-[11px]">
            <button
              type="button"
              onClick={goUpSavePath}
              disabled={!parentDirectory(savePath)}
              title="返回上级目录"
              className="flex h-5 w-5 shrink-0 items-center justify-center rounded border border-neutral-700 text-neutral-300 transition-colors hover:bg-neutral-800 disabled:cursor-not-allowed disabled:opacity-30"
            >
              <ArrowUp size={11} />
            </button>
            <span className="shrink-0 text-neutral-500">下载到</span>
            <span
              className="min-w-0 flex-1 truncate text-left text-neutral-300 [direction:rtl]"
              title={savePath || undefined}
            >
              {savePath || "qBittorrent 默认目录"}
            </span>
            <button
              type="button"
              onClick={() => void chooseSavePath()}
              className="shrink-0 rounded border border-neutral-700 px-2 py-0.5 text-neutral-300 transition-colors hover:bg-neutral-800"
            >
              选择
            </button>
          </div>

          <div className="flex min-h-0 flex-1 flex-col gap-2">
              <div className="flex items-center justify-between text-[11px] text-neutral-500">
                <span>当前目录任务</span>
                <span className="tabular-nums">{qbTorrents.length} 条</span>
              </div>

              {qbTorrents.length === 0 ? (
                <p className="py-6 text-center text-[11px] text-neutral-600">
                  {qbVersion
                    ? "当前下载目录没有任务。"
                    : "连接成功后显示任务列表。"}
                </p>
              ) : (
                <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto pr-1">
                  {qbTorrents.map((torrent) => (
                    <div
                      key={torrent.hash}
                      className="rounded-lg border border-neutral-800 bg-neutral-950 p-2.5"
                    >
                      <div className="flex items-start gap-1.5">
                        <div
                          className="min-w-0 flex-1 truncate text-xs text-neutral-200"
                          title={torrent.name}
                        >
                          {torrent.name}
                        </div>
                        <button
                          type="button"
                          onClick={() => void toggleTorrent(torrent)}
                          disabled={pendingHashes.has(torrent.hash)}
                          title={
                            isTorrentStopped(torrent.state)
                              ? "启动任务"
                              : "停止任务"
                          }
                          className={`flex h-5 w-5 shrink-0 items-center justify-center rounded transition-colors disabled:opacity-40 ${
                            isTorrentStopped(torrent.state)
                              ? "text-neutral-500 hover:bg-indigo-500/15 hover:text-indigo-300"
                              : "text-neutral-500 hover:bg-rose-950/60 hover:text-rose-300"
                          }`}
                        >
                          {pendingHashes.has(torrent.hash) ? (
                            <Loader2 size={11} className="animate-spin" />
                          ) : isTorrentStopped(torrent.state) ? (
                            <Play size={11} />
                          ) : (
                            <Square size={11} />
                          )}
                        </button>
                      </div>
                      <div className="mt-1.5 flex items-center justify-between text-[11px] text-neutral-500">
                        <span>
                          {STATE_LABELS[torrent.state] ?? torrent.state} ·{" "}
                          {Math.round(torrent.progress * 100)}%
                        </span>
                        <span className="flex items-center gap-2 tabular-nums">
                          <span className="flex items-center gap-0.5 text-indigo-400">
                            <ArrowDown size={10} />
                            {formatSpeed(torrent.dlspeed)}
                          </span>
                          <span className="flex items-center gap-0.5 text-emerald-400">
                            <ArrowUp size={10} />
                            {formatSpeed(torrent.upspeed)}
                          </span>
                        </span>
                      </div>
                      <div className="mt-1.5 h-1 overflow-hidden rounded bg-neutral-800">
                        <div
                          className="h-full rounded bg-indigo-500"
                          style={{
                            width: `${Math.min(100, Math.max(0, torrent.progress * 100))}%`,
                          }}
                        />
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
        </aside>
      </div>

      {(toast || leavingToast) && (
        <div className="fixed bottom-4 right-3 z-[100] w-56">
          {leavingToast && (
            <div className="absolute bottom-0 left-0 w-full">
              {renderToast(leavingToast, true)}
            </div>
          )}
          {toast && <div className="relative">{renderToast(toast, false)}</div>}
        </div>
      )}

      {manageOpen && (
        <BookmarksDialog
          bookmarks={bookmarks}
          onChange={setBookmarks}
          onClose={() => setManageOpen(false)}
        />
      )}
    </div>
  );
}
