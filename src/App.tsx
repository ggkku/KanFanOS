import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { Loader2 } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { AppShell } from "./components/layout/AppShell";
import { MediaBanner } from "./components/media/MediaBanner";
import { PlayerDrawer } from "./components/player/PlayerDrawer";
import { ScanBanner } from "./components/scan/ScanBanner";
import { SeriesDetail } from "./components/series/SeriesDetail";
import { SeriesExtras } from "./components/series/SeriesExtras";
import { SetupScreen } from "./components/setup/SetupScreen";
import { CategoryPage } from "./pages/category/CategoryPage";
import { RecentPage } from "./pages/recent/RecentPage";
import { ResourcesPage } from "./pages/resources/ResourcesPage";
import { SearchPage } from "./pages/search/SearchPage";
import { SettingsPage } from "./pages/settings/SettingsPage";
import {
  cancelMediaProcessing,
  cancelScan,
  getMediaStats,
  getSeries,
  getSettings,
  listCategories,
  listEpisodes,
  listExtraEpisodes,
  listRecent,
  listSeries,
  onLibrarySync,
  onMediaProgress,
  onScanProgress,
  scanLibrary,
  searchSeries,
  setLibraryRoot,
  startMediaProcessing,
} from "./api/library";
import { errorMessage } from "./lib/errors";
import { sortEpisodes } from "./lib/episodes";
import { seriesCoverUrl } from "./lib/thumbnails";
import type {
  AppSettings,
  CategorySummary,
  EpisodeSummary,
  MediaProgress,
  RecentItem,
  ScanProgress,
  SearchItem,
  SeriesSort,
  SeriesSummary,
  SeriesTransition,
} from "./types/library";
import type { View } from "./types/navigation";

const PAGE_SIZE = 60;
/// 「最近观看」页面最多展示的条数。
const RECENT_LIMIT = 6;

/// 选集页里评分、看过遍数或名称改动后，就地更新列表中的同一部番。
/// 这样返回选番页时卡片 hover 立刻使用新数据，而不是等待下一次整页刷新。
function patchSeriesSummary<T extends SeriesSummary>(
  items: T[],
  next: SeriesSummary,
): T[] {
  let changed = false;
  const patched = items.map((item) => {
    if (item.id !== next.id) return item;
    changed = true;
    return { ...item, ...next } as T;
  });
  return changed ? patched : items;
}

function App() {
  const [view, setView] = useState<View>({ kind: "recent" });
  // 页面切换动画：方向由左侧导航的上下顺序决定。
  const [pageTransition, setPageTransition] = useState<"down" | "up" | "fade">(
    "fade",
  );
  const [outgoingView, setOutgoingView] = useState<{
    page: ReactNode;
    scrollTop: number;
    rect: { top: number; left: number; width: number; height: number };
    /// `undefined` 用当前页面退出动画；空字符串表示静态背景快照。
    exitClass?: string | null;
    behind: boolean;
  } | null>(null);
  const outgoingTimer = useRef<number | null>(null);
  const outgoingScrollRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<ReactNode>(null);
  // 主滚动容器；打开选集页前记下位置，返回时恢复，
  // 这样封面飞回的落点和离开时完全一致。
  const scrollRef = useRef<HTMLElement>(null);
  const savedScrollTopRef = useRef<number | null>(null);
  // 新页面渲染后要设置的主滚动位置。
  const pendingScrollTopRef = useRef<number | null>(null);
  // 打开番剧进入选集页的动画模式与封面飞入浮层。
  const [detailAnimation, setDetailAnimation] =
    useState<SeriesTransition>("rise");
  const [flipCover, setFlipCover] = useState<{
    url: string | null;
    /// 没有手动封面时，浮层用番剧名首字占位，和卡片保持一致。
    label: string;
    from: DOMRect;
    /// 返回时的目标位置（卡片封面）。飞入时由详情页封面提供。
    to?: DOMRect;
  } | null>(null);
  // 记住打开详情时卡片封面的位置，返回时封面飞回去。
  const flipOriginRef = useRef<{
    url: string | null;
    label: string;
    from: DOMRect;
  } | null>(null);
  // 从选集页返回时使用的倒放动画。
  const [detailReturn, setDetailReturn] = useState<SeriesTransition | null>(
    null,
  );
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [categories, setCategories] = useState<CategorySummary[]>([]);
  const [series, setSeries] = useState<SeriesSummary[]>([]);
  const [seriesTotal, setSeriesTotal] = useState(0);
  const [loadingMore, setLoadingMore] = useState(false);
  const [sort, setSort] = useState<SeriesSort>("name");
  const [query, setQuery] = useState("");
  const [searchItems, setSearchItems] = useState<SearchItem[]>([]);
  const [searchTotal, setSearchTotal] = useState(0);
  const [searchLoading, setSearchLoading] = useState(false);
  const [searchLoadingMore, setSearchLoadingMore] = useState(false);
  const [recentItems, setRecentItems] = useState<RecentItem[]>([]);
  const [recentLoading, setRecentLoading] = useState(false);
  const [recentVersion, setRecentVersion] = useState(0);
  const [playerTarget, setPlayerTarget] = useState<{
    series: SeriesSummary;
    episodes: EpisodeSummary[];
    episode: EpisodeSummary;
  } | null>(null);
  const [selectedSeries, setSelectedSeries] = useState<SeriesSummary | null>(
    null,
  );
  const [episodes, setEpisodes] = useState<EpisodeSummary[]>([]);
  const [extraEpisodes, setExtraEpisodes] = useState<EpisodeSummary[]>([]);
  const [extrasOpen, setExtrasOpen] = useState(false);
  const [extrasClosing, setExtrasClosing] = useState(false);
  const extrasTimer = useRef<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [seriesLoading, setSeriesLoading] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [syncOffline, setSyncOffline] = useState(false);
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const [mediaProgress, setMediaProgress] = useState<MediaProgress | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [noticeVisible, setNoticeVisible] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);
  // 当前番剧列表 / 搜索结果对应的查询标识（库 + 档期 + 排序等）。
  // 只有查询真正变化时才清空旧数据；后台刷新（文件监听、媒体处理完成）
  // 必须保留已渲染的卡片，否则卡片会被卸载重挂，hover 缓存的奖项状态随之丢失。
  const seriesQueryRef = useRef<string | null>(null);
  const searchQueryRef = useRef<string | null>(null);

  // 启动时读取配置与档期。
  useEffect(() => {
    let disposed = false;

    (async () => {
      try {
        const nextSettings = await getSettings();
        if (disposed) return;
        setSettings(nextSettings);

        if (nextSettings.libraryId != null) {
          const nextCategories = await listCategories(nextSettings.libraryId);
          if (disposed) return;
          setCategories(nextCategories);
          if (nextCategories.length > 0) {
            setView({ kind: "category", category: nextCategories[0].key });
          }

          // 上次未完成的元数据/缩略图任务继续处理。
          // failed > 0 时也要启动一次：安装包补上 FFprobe/FFmpeg 后，
          // 后端会把这些三连失败的任务重新排队。
          const media = await getMediaStats(nextSettings.libraryId);
          const hasMediaWork =
            media.pending + media.processing > 0 ||
            (media.failed > 0 && nextSettings.toolsReady);
          if (!disposed && hasMediaWork) {
            try {
              await startMediaProcessing(nextSettings.libraryId);
            } catch (err) {
              const message = errorMessage(err);
              if (!disposed && !message.includes("已在进行中")) {
                setError(message);
              }
            }
          }
        }
      } catch (err) {
        if (!disposed) setError(errorMessage(err));
      } finally {
        if (!disposed) setLoading(false);
      }
    })();

    return () => {
      disposed = true;
    };
  }, []);

  // 监听扫描进度。
  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;

    onScanProgress((update) => {
      setProgress(update.phase === "scanning" ? update : null);
    }).then((fn) => {
      if (disposed) {
        fn();
      } else {
        unlisten = fn;
      }
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // 监听元数据/缩略图处理进度；完成后刷新列表以显示封面。
  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;

    onMediaProgress((update) => {
      if (update.phase === "processing") {
        setMediaProgress(update.total > 0 ? update : null);
      } else {
        setMediaProgress(null);
        setRefreshKey((value) => value + 1);
      }
    }).then((fn) => {
      if (disposed) {
        fn();
      } else {
        unlisten = fn;
      }
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // 文件系统监听：变化后自动刷新档期与当前列表。
  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    const libraryId = settings?.libraryId;

    onLibrarySync((update) => {
      if (disposed) return;

      if (update.phase === "syncing") {
        setSyncing(true);
        return;
      }

      setSyncing(false);

      if (update.phase === "completed") {
        setSyncOffline(false);
        setRefreshKey((value) => value + 1);

        if (libraryId != null) {
          listCategories(libraryId)
            .then((next) => {
              if (disposed) return;
              setCategories(next);
              setView((current) => {
                if (
                  current.kind === "category" &&
                  !next.some((item) => item.key === current.category)
                ) {
                  return next.length > 0
                    ? { kind: "category", category: next[0].key }
                    : current;
                }
                return current;
              });
            })
            .catch(() => {
              // 分类刷新失败不覆盖当前内容。
            });
        }
      } else if (update.phase === "offline") {
        setSyncOffline(true);
      } else if (update.phase === "watching") {
        setSyncOffline(false);
      }
    }).then((fn) => {
      if (disposed) {
        fn();
      } else {
        unlisten = fn;
      }
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [settings?.libraryId]);

  // 切换档期或排序时加载第一页。
  useEffect(() => {
    if (view.kind !== "category" || settings?.libraryId == null) {
      seriesQueryRef.current = null;
      setSeries([]);
      setSeriesTotal(0);
      return;
    }

    const libraryId = settings.libraryId;
    // 查询变化时先清空，让加载态正常出现；同一查询的后台刷新不清空。
    const queryKey = `${libraryId}:${view.category}:${sort}`;
    if (seriesQueryRef.current !== queryKey) {
      seriesQueryRef.current = queryKey;
      setSeries([]);
      setSeriesTotal(0);
    }

    let disposed = false;
    setSeriesLoading(true);

    listSeries(libraryId, view.category, sort, 0, PAGE_SIZE)
      .then((page) => {
        if (!disposed) {
          setSeries(page.items);
          setSeriesTotal(page.total);
        }
      })
      .catch((err: unknown) => {
        if (!disposed) setError(errorMessage(err));
      })
      .finally(() => {
        if (!disposed) setSeriesLoading(false);
      });

    return () => {
      disposed = true;
    };
  }, [view, settings?.libraryId, refreshKey, sort]);

  // 搜索带 250ms 防抖，结果按页加载。
  useEffect(() => {
    const trimmed = query.trim();
    const libraryId = settings?.libraryId;

    if (!trimmed || libraryId == null) {
      searchQueryRef.current = null;
      setSearchItems([]);
      setSearchTotal(0);
      setSearchLoading(false);
      return;
    }

    // 关键词变化时清空旧结果；refreshKey 触发的后台刷新保留卡片，
    // 避免 hover 中的卡片被卸载后奖项状态丢失。
    const queryKey = `${libraryId}:${trimmed}`;
    if (searchQueryRef.current !== queryKey) {
      searchQueryRef.current = queryKey;
      setSearchItems([]);
      setSearchTotal(0);
    }

    let disposed = false;
    setSearchLoading(true);

    const timer = setTimeout(() => {
      searchSeries(libraryId, trimmed, 0, PAGE_SIZE)
        .then((page) => {
          if (!disposed) {
            setSearchItems(page.items);
            setSearchTotal(page.total);
          }
        })
        .catch((err: unknown) => {
          if (!disposed) setError(errorMessage(err));
        })
        .finally(() => {
          if (!disposed) setSearchLoading(false);
        });
    }, 250);

    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [query, settings?.libraryId, refreshKey]);

  // 最近观看按打开时间排序，播放后重新拉取。
  useEffect(() => {
    if (view.kind !== "recent" || settings?.libraryId == null) {
      return;
    }

    const libraryId = settings.libraryId;
    let disposed = false;
    setRecentLoading(true);

    listRecent(libraryId, RECENT_LIMIT)
      .then((items) => {
        if (!disposed) setRecentItems(items);
      })
      .catch((err: unknown) => {
        if (!disposed) setError(errorMessage(err));
      })
      .finally(() => {
        if (!disposed) setRecentLoading(false);
      });

    return () => {
      disposed = true;
    };
  }, [view, settings?.libraryId, refreshKey, recentVersion]);

  // 导航项在侧栏里的顺序：最近观看 → 资源 → 各档期 → 设置。
  const navIndex = (target: View): number | null => {
    if (target.kind === "recent") return 0;
    if (target.kind === "resources") return 1;
    if (target.kind === "category") {
      const index = categories.findIndex(
        (item) => item.key === target.category,
      );
      return index >= 0 ? index + 2 : null;
    }
    return categories.length + 2;
  };

  // 把当前页面快照固定到主滚动区域的大小，并按离开时的 scrollTop 显示。
  // 这样整页滑动不会因为旧页面内容太长而溢出，也不会只显示列表顶部。
  const showOutgoing = (
    page: ReactNode,
    duration: number,
    options?: { exitClass?: string | null; behind?: boolean },
  ) => {
    const element = scrollRef.current;
    if (!element || page == null) {
      setOutgoingView(null);
      return;
    }

    const rect = element.getBoundingClientRect();
    setOutgoingView({
      page,
      scrollTop: element.scrollTop,
      rect: {
        top: rect.top,
        left: rect.left,
        width: element.clientWidth,
        height: element.clientHeight,
      },
      exitClass: options?.exitClass,
      behind: options?.behind ?? false,
    });

    if (outgoingTimer.current) window.clearTimeout(outgoingTimer.current);
    outgoingTimer.current = window.setTimeout(() => {
      setOutgoingView(null);
    }, duration);
  };

  const navigate = (next: View) => {
    savedScrollTopRef.current = null;
    if (extrasTimer.current) window.clearTimeout(extrasTimer.current);
    setExtrasClosing(false);

    const from = navIndex(view);
    const to = navIndex(next);

    // 离开资源页时不要生成离场快照：快照会再挂载一个 ResourcesPage，
    // 它的恢复逻辑可能把刚隐藏的原生 webview 重新显示出来。
    if (
      from != null &&
      to != null &&
      from !== to &&
      view.kind !== "resources"
    ) {
      // 目标在下方时内容向上移动（新页面从下方进入），与列表滚动方向一致。
      const direction = to > from ? "up" : "down";
      setPageTransition(direction);
      setDetailReturn(null);
      const duration = settings?.pageTransition === "slide" ? 360 : 260;
      showOutgoing(contentRef.current, duration + 40);
    } else {
      setOutgoingView(null);
    }

    // 新页面从顶部开始；旧页面的滚动位置已经保存在 outgoingView 里。
    pendingScrollTopRef.current = 0;
    setSelectedSeries(null);
    setEpisodes([]);
    setExtraEpisodes([]);
    setExtrasOpen(false);
    setQuery("");
    setFlipCover(null);
    flipOriginRef.current = null;
    setView(next);
  };

  // 从选集页返回：使用进入动画的倒放。
  const closeSeries = () => {
    const content = contentRef.current;
    setDetailReturn(detailAnimation);

    if (content) {
      showOutgoing(content, 340);
    } else {
      setOutgoingView(null);
      setDetailReturn(null);
    }

    // 封面飞入模式：让详情页封面飞回原来的卡片位置。
    if (detailAnimation === "flip" && flipOriginRef.current) {
      const cover = document.querySelector<HTMLElement>("[data-series-cover]");
      const rect = cover?.getBoundingClientRect();
      if (rect) {
        const origin = flipOriginRef.current;
        setFlipCover({
          url: origin.url,
          label: origin.label,
          from: rect,
          to: origin.from,
        });
      }
      flipOriginRef.current = null;
    }

    // 返回后恢复离开时的滚动位置；必须在新页面提交后、封面飞回动画开始前完成。
    pendingScrollTopRef.current = savedScrollTopRef.current ?? 0;
    savedScrollTopRef.current = null;

    setSelectedSeries(null);
    setEpisodes([]);
    setExtraEpisodes([]);
    setExtrasOpen(false);
    setExtrasClosing(false);
    if (extrasTimer.current) window.clearTimeout(extrasTimer.current);
  };

  // 进入「其他视频」：固定整块实心底色，从右侧滑入。
  const openExtras = () => {
    if (extrasTimer.current) window.clearTimeout(extrasTimer.current);
    setExtrasClosing(false);
    setExtrasOpen(true);
  };

  // 退出「其他视频」：同一块实心页面向右侧滑出，动画结束后再卸载。
  const closeExtras = () => {
    if (!extrasOpen || extrasClosing) return;
    setExtrasClosing(true);
    if (extrasTimer.current) window.clearTimeout(extrasTimer.current);
    extrasTimer.current = window.setTimeout(() => {
      setExtrasOpen(false);
      setExtrasClosing(false);
      extrasTimer.current = null;
    }, 360);
  };

  // 新页面每次提交后应用一次待恢复的滚动位置。
  // 没有依赖数组：只在 ref 有值时动作，且一定早于封面飞回动画的 rAF。
  useLayoutEffect(() => {
    const top = pendingScrollTopRef.current;
    if (top == null) return;
    pendingScrollTopRef.current = null;

    const container = scrollRef.current;
    if (container) container.scrollTop = top;
  });

  // 等旧页面快照渲染出来后再滚到离开时的位置。
  useLayoutEffect(() => {
    if (!outgoingView) return;
    const container = outgoingScrollRef.current;
    if (container) container.scrollTop = outgoingView.scrollTop;
  }, [outgoingView]);

  const handleQueryChange = (value: string) => {
    setQuery(value);
    if (value.trim().length > 0) {
      setSelectedSeries(null);
      setEpisodes([]);
      setExtraEpisodes([]);
      setExtrasOpen(false);
    }
  };

  const loadMoreSeries = async () => {
    if (
      view.kind !== "category" ||
      settings?.libraryId == null ||
      loadingMore
    ) {
      return;
    }

    setLoadingMore(true);
    try {
      const page = await listSeries(
        settings.libraryId,
        view.category,
        sort,
        series.length,
        PAGE_SIZE,
      );
      setSeries((current) => [...current, ...page.items]);
      setSeriesTotal(page.total);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoadingMore(false);
    }
  };

  const loadMoreSearch = async () => {
    const trimmed = query.trim();
    if (!trimmed || settings?.libraryId == null || searchLoadingMore) {
      return;
    }

    setSearchLoadingMore(true);
    try {
      const page = await searchSeries(
        settings.libraryId,
        trimmed,
        searchItems.length,
        PAGE_SIZE,
      );
      setSearchItems((current) => [...current, ...page.items]);
      setSearchTotal(page.total);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSearchLoadingMore(false);
    }
  };

  // 扫描结果提示：淡入出现，几秒后淡出收起。
  const noticeTimer = useRef<number | null>(null);
  const noticeCleanupTimer = useRef<number | null>(null);
  const noticeShown = useRef(false);

  const clearNotice = useCallback(() => {
    if (noticeTimer.current) window.clearTimeout(noticeTimer.current);
    if (noticeCleanupTimer.current) {
      window.clearTimeout(noticeCleanupTimer.current);
      noticeCleanupTimer.current = null;
    }
    if (!noticeShown.current) return;

    setNoticeVisible(false);
    noticeCleanupTimer.current = window.setTimeout(() => {
      noticeShown.current = false;
      setNotice(null);
    }, 300);
  }, []);

  const showNotice = useCallback((message: string) => {
    if (noticeTimer.current) window.clearTimeout(noticeTimer.current);
    if (noticeCleanupTimer.current) {
      window.clearTimeout(noticeCleanupTimer.current);
      noticeCleanupTimer.current = null;
    }

    if (noticeShown.current) {
      setNotice(message);
      setNoticeVisible(true);
    } else {
      noticeShown.current = true;
      setNotice(message);
      setNoticeVisible(false);
      // 先以收起状态挂载，再在下一帧展开，才能触发过渡。
      requestAnimationFrame(() => setNoticeVisible(true));
    }

    noticeTimer.current = window.setTimeout(() => {
      setNoticeVisible(false);
      noticeCleanupTimer.current = window.setTimeout(() => {
        noticeShown.current = false;
        setNotice(null);
      }, 300);
    }, 4000);
  }, []);

  useEffect(() => {
    return () => {
      if (noticeTimer.current) window.clearTimeout(noticeTimer.current);
      if (noticeCleanupTimer.current) {
        window.clearTimeout(noticeCleanupTimer.current);
      }
    };
  }, []);

  const runScan = useCallback(async (libraryId: number) => {
    setScanning(true);
    setError(null);
    clearNotice();
    setProgress({
      libraryId,
      phase: "scanning",
      seriesFound: 0,
      filesFound: 0,
      errors: 0,
      currentPath: null,
      elapsedMs: 0,
    });

    try {
      const summary = await scanLibrary(libraryId);

      // 扫描完成后在后台补齐元数据与缩略图。
      void startMediaProcessing(libraryId).catch((err: unknown) => {
        const message = errorMessage(err);
        if (!message.includes("已在进行中")) setError(message);
      });

      const nextCategories = await listCategories(libraryId);
      setCategories(nextCategories);
      setSelectedSeries(null);
      setEpisodes([]);
      setExtraEpisodes([]);
      setExtrasOpen(false);
      setRefreshKey((value) => value + 1);

      setView((current) => {
        if (
          current.kind === "category" &&
          nextCategories.some((item) => item.key === current.category)
        ) {
          return current;
        }
        if (nextCategories.length > 0) {
          return { kind: "category", category: nextCategories[0].key };
        }
        return current;
      });

      if (summary.cancelled) {
        showNotice(
          `扫描已取消：发现 ${summary.seriesFound} 部番剧、${summary.filesFound.toLocaleString()} 个视频。`,
        );
      } else {
        const parts = [
          `${summary.seriesFound} 部番剧`,
          `${summary.filesFound.toLocaleString()} 个视频`,
        ];
        if (summary.missing > 0) {
          parts.push(`${summary.missing.toLocaleString()} 个文件已失效`);
        }
        if (summary.errors > 0) {
          parts.push(`${summary.errors.toLocaleString()} 个文件读取失败`);
        }
        showNotice(
          `扫描完成：${parts.join("，")}（${(summary.elapsedMs / 1000).toFixed(1)} 秒）。`,
        );
      }
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setScanning(false);
      setProgress(null);
    }
  }, [clearNotice, showNotice]);

  // 已经配置了目录但数据库里没有任何索引时，自动扫描一次。
  const autoScanned = useRef(false);
  useEffect(() => {
    if (autoScanned.current || loading || scanning) return;
    if (settings?.libraryId != null && categories.length === 0) {
      autoScanned.current = true;
      void runScan(settings.libraryId);
    }
  }, [loading, scanning, settings?.libraryId, categories.length, runScan]);

  const rescan = () => {
    if (settings?.libraryId != null) {
      void runScan(settings.libraryId);
    }
  };

  const chooseRoot = async () => {
    setError(null);
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "选择番剧目录",
      });
      if (typeof selected !== "string") return;

      const library = await setLibraryRoot(selected);
      const nextSettings = await getSettings();
      setSettings(nextSettings);
      showNotice(`已设置番剧目录：${library.rootPath}`);
      await runScan(library.id);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const openSeries = async (
    target: SeriesSummary,
    coverRect?: DOMRect | null,
    resumeVideoId?: number | null,
  ) => {
    savedScrollTopRef.current = scrollRef.current?.scrollTop ?? null;

    const mode = settings?.seriesTransition ?? "rise";
    setDetailAnimation(mode);

    // 旧页面固定成离开时可见的滚动快照，详情页从顶部进入。
    showOutgoing(contentRef.current, 340);
    pendingScrollTopRef.current = 0;

    // 封面飞入：先记下卡片封面在屏幕上的位置，详情页挂载后再算目标位置。
    // 没有手动封面时用首字占位浮层，保证「封面飞入」动画仍然有效。
    const coverUrl = seriesCoverUrl(target);
    if (mode === "flip" && coverRect) {
      const label = target.name.slice(0, 1);
      flipOriginRef.current = { url: coverUrl, label, from: coverRect };
      setFlipCover({ url: coverUrl, label, from: coverRect });
    } else {
      flipOriginRef.current = null;
      setFlipCover(null);
    }
    setDetailReturn(null);

    setSelectedSeries(target);
    setExtrasOpen(false);
    try {
      const [mainRows, extraRows] = await Promise.all([
        listEpisodes(target.id),
        listExtraEpisodes(target.id),
      ]);
      const sorted = sortEpisodes(mainRows);
      const sortedExtras = sortEpisodes(extraRows);
      setEpisodes(sorted);
      setExtraEpisodes(sortedExtras);

      if (resumeVideoId != null) {
        const mainEpisode = sorted.find(
          (row) => row.id === resumeVideoId,
        );
        if (mainEpisode) {
          openPlayer(target, sorted, mainEpisode);
        } else {
          const extraEpisode = sortedExtras.find(
            (row) => row.id === resumeVideoId,
          );
          if (extraEpisode) {
            setExtrasOpen(true);
            openPlayer(target, sortedExtras, extraEpisode);
          }
        }
      }
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const handleCancelScan = async () => {
    try {
      await cancelScan();
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const handleCancelMedia = async () => {
    try {
      await cancelMediaProcessing();
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const openPlayer = (
    series: SeriesSummary,
    list: EpisodeSummary[],
    episode: EpisodeSummary,
  ) => {
    setPlayerTarget({ series, episodes: list, episode });
  };

  const openRecentSeries = async (item: RecentItem) => {
    if (item.seriesId == null) {
      setError("该视频没有关联番剧");
      return;
    }

    try {
      const series = await getSeries(item.seriesId);
      await openSeries(series, null, item.videoId);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  // 「上一季 / 下一季」：按 id 取出完整番剧信息后走正常的选集页切换。
  const openSeriesById = async (seriesId: number) => {
    try {
      const target = await getSeries(seriesId);
      await openSeries(target, null);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const refreshSelectedSeries = async () => {
    if (!selectedSeries) return;

    try {
      const [series, mainRows, extraRows] = await Promise.all([
        getSeries(selectedSeries.id),
        listEpisodes(selectedSeries.id),
        listExtraEpisodes(selectedSeries.id),
      ]);
      setSelectedSeries(series);
      // 列表页可能正被选集页覆盖着，但它仍是返回后要显示的数据源。
      // 评分 / 看过遍数 / 名称在选集页改动后必须同步过去，否则返回时卡片 hover 还是旧值。
      setSeries((items) => patchSeriesSummary(items, series));
      setSearchItems((items) => patchSeriesSummary(items, series));
      setEpisodes(sortEpisodes(mainRows));
      setExtraEpisodes(sortEpisodes(extraRows));
      if (extraRows.length === 0) setExtrasOpen(false);
      setRecentVersion((value) => value + 1);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const closePlayer = () => {
    setPlayerTarget(null);
    setRecentVersion((value) => value + 1);
    setRefreshKey((value) => value + 1);

    if (selectedSeries) {
      void refreshSelectedSeries();
    }
  };

  let content: ReactNode;
  if (loading) {
    content = (
      <div className="flex flex-1 items-center justify-center py-24 text-neutral-500">
        <Loader2 size={22} className="animate-spin" />
      </div>
    );
  } else if (selectedSeries) {
    content = (
      <SeriesDetail
        series={selectedSeries}
        episodes={episodes}
        dataRoot={settings?.dataRoot ?? null}
        coverUrl={seriesCoverUrl(selectedSeries)}
        extraCount={extraEpisodes.length}
        onBack={closeSeries}
        onPlay={(episode) => {
          if (selectedSeries) {
            openPlayer(selectedSeries, episodes, episode);
          }
        }}
        onOpenExtras={openExtras}
        onNavigateSeries={(seriesId) => void openSeriesById(seriesId)}
        onChanged={refreshSelectedSeries}
      />
    );
  } else if (view.kind === "resources") {
    // 资源页不依赖媒体库，未选择番剧目录时也能使用。
    content = <ResourcesPage />;
  } else if (settings?.libraryRoot == null) {
    content = <SetupScreen onChooseRoot={() => void chooseRoot()} />;
  } else if (query.trim().length > 0) {
    content = (
      <SearchPage
        query={query.trim()}
        items={searchItems}
        total={searchTotal}
        loading={searchLoading}
        loadingMore={searchLoadingMore}
        onOpenSeries={(item, rect) => void openSeries(item, rect)}
        onLoadMore={() => void loadMoreSearch()}
      />
    );
  } else if (view.kind === "settings") {
    content = (
      <SettingsPage
        settings={settings}
        scanning={scanning}
        onChooseRoot={() => void chooseRoot()}
        onRescan={rescan}
        onSettingsChange={setSettings}
        onCacheCleared={() => setRefreshKey((value) => value + 1)}
      />
    );
  } else if (view.kind === "recent") {
    content = (
      <RecentPage
        items={recentItems}
        loading={recentLoading}
        dataRoot={settings?.dataRoot ?? null}
        onPlay={(item) => void openRecentSeries(item)}
        onRefresh={() => setRecentVersion((value) => value + 1)}
      />
    );
  } else {
    const label =
      categories.find((item) => item.key === view.category)?.label ??
      view.category;
    content = (
      <CategoryPage
        label={label}
        series={series}
        total={seriesTotal}
        loading={seriesLoading}
        loadingMore={loadingMore}
        scanning={scanning}
        sort={sort}
        onSortChange={setSort}
        onOpenSeries={(item, rect) => void openSeries(item, rect)}
        onRescan={rescan}
        onLoadMore={() => void loadMoreSeries()}
      />
    );
  }

  // 保存当前页面元素，供下一次导航做退出动画。
  useEffect(() => {
    contentRef.current = content;
  });

  useEffect(() => {
    return () => {
      if (outgoingTimer.current) window.clearTimeout(outgoingTimer.current);
    };
  }, []);

  const viewKey = selectedSeries
    ? `series:${selectedSeries.id}`
    : view.kind === "category"
      ? `category:${view.category}`
      : view.kind;

  const isSlideTransition = settings?.pageTransition === "slide";

  const enterAnimation = isSlideTransition
    ? pageTransition === "down"
      ? "animate-page-slide-in-down"
      : pageTransition === "up"
        ? "animate-page-slide-in-up"
        : "animate-[page-in-fade_260ms_ease-out_both]"
    : pageTransition === "down"
      ? "animate-[page-in-down_260ms_ease-out_both]"
      : pageTransition === "up"
        ? "animate-[page-in-up_260ms_ease-out_both]"
        : "animate-[page-in-fade_260ms_ease-out_both]";

  const exitAnimation = isSlideTransition
    ? pageTransition === "down"
      ? "animate-page-slide-out-down"
      : pageTransition === "up"
        ? "animate-page-slide-out-up"
        : "animate-[page-out-fade_260ms_ease-out_both]"
    : pageTransition === "down"
      ? "animate-[page-out-down_260ms_ease-out_both]"
      : pageTransition === "up"
        ? "animate-[page-out-up_260ms_ease-out_both]"
        : "animate-[page-out-fade_260ms_ease-out_both]";

  const detailEnterAnimation =
    detailAnimation === "rise"
      ? "animate-detail-in-rise"
      : detailAnimation === "zoom"
        ? "animate-detail-in-zoom"
        : detailAnimation === "slide"
          ? "animate-detail-in-slide"
          : "animate-detail-in-fade";

  const detailExitAnimation =
    detailAnimation === "slide"
      ? "animate-detail-out-slide"
      : "animate-detail-out-fade";

  const detailReverseExitAnimation =
    detailAnimation === "rise"
      ? "animate-detail-out-rise"
      : detailAnimation === "zoom"
        ? "animate-detail-out-zoom"
        : detailAnimation === "slide"
          ? "animate-detail-out-slide-right"
          : "animate-detail-out-fade";

  const activeEnterAnimation = detailReturn
    ? "animate-detail-in-fade"
    : selectedSeries
      ? detailEnterAnimation
      : enterAnimation;
  const activeExitAnimation = detailReturn
    ? detailReverseExitAnimation
    : selectedSeries
      ? detailExitAnimation
      : exitAnimation;

  // 封面飞入/飞出：浮层图片在卡片封面和详情页封面之间动画。
  useEffect(() => {
    if (!flipCover) return;

    const frame = requestAnimationFrame(() => {
      const overlay = document.querySelector<HTMLImageElement>(
        "[data-flip-cover]",
      );

      if (!overlay) {
        setFlipCover(null);
        return;
      }

      // 飞入时目标在详情页里；返回时目标已经由 flipCover.to 给出。
      let target = flipCover.to ?? null;
      let targetElement: HTMLElement | null = null;

      if (!target) {
        targetElement = document.querySelector<HTMLElement>(
          "[data-series-cover]",
        );
        target = targetElement?.getBoundingClientRect() ?? null;
      }

      const visible =
        target != null &&
        target.width > 0 &&
        target.height > 0 &&
        target.bottom > 0 &&
        target.top < window.innerHeight;

      if (!target || !visible) {
        setFlipCover(null);
        return;
      }

      const from = flipCover.from;
      const dx = target.left - from.left;
      const dy = target.top - from.top;
      const scaleX = target.width / from.width;
      const scaleY = target.height / from.height;

      if (targetElement) targetElement.style.visibility = "hidden";
      const animation = overlay.animate(
        [
          { transform: "translate(0, 0) scale(1, 1)" },
          {
            transform: `translate(${dx}px, ${dy}px) scale(${scaleX}, ${scaleY})`,
          },
        ],
        {
          duration: 320,
          easing: "cubic-bezier(0.16, 1, 0.3, 1)",
          // 保持最后一帧，避免动画结束后浮层先跳回起点再被移除。
          fill: "forwards",
        },
      );

      animation.onfinish = () => {
        if (targetElement) targetElement.style.visibility = "";
        overlay.style.visibility = "hidden";
        setFlipCover(null);
      };
    });

    return () => cancelAnimationFrame(frame);
  }, [flipCover]);

  return (
    <>
      <AppShell
        view={view}
        categories={categories}
        scanning={scanning}
        syncing={syncing}
        query={query}
        onQueryChange={handleQueryChange}
        onNavigate={navigate}
        scrollRef={scrollRef}
        wide={view.kind === "resources"}
      >
      {progress && <ScanBanner progress={progress} onCancel={() => void handleCancelScan()} />}

      {mediaProgress && (
        <MediaBanner
          progress={mediaProgress}
          onCancel={() => void handleCancelMedia()}
        />
      )}

      {syncOffline && (
        <div className="rounded-lg border border-amber-900/60 bg-amber-950/30 px-4 py-3 text-sm text-amber-300">
          番剧目录当前不可访问，自动同步已暂停。请检查磁盘或网络位置是否已连接。
        </div>
      )}

      {settings && !settings.toolsReady && (
        <div className="rounded-lg border border-amber-900/60 bg-amber-950/30 px-4 py-3 text-sm text-amber-300">
          未找到 FFprobe / FFmpeg，视频时长和缩略图无法生成。请确认安装目录中有
          ffmpeg.exe 和 ffprobe.exe，或重新安装完整安装包。
        </div>
      )}

      {error && (
        <div className="rounded-lg border border-red-900/60 bg-red-950/40 px-4 py-3 text-sm text-red-300">
          {error}
        </div>
      )}

      {(notice || noticeVisible) && (
        <div
          className={`fixed left-[calc(50%+7.5rem)] top-6 z-[60] max-w-md -translate-x-1/2 rounded-lg border border-emerald-900/60 bg-emerald-950/90 px-4 py-3 text-sm text-emerald-300 shadow-2xl backdrop-blur transition-all duration-300 ease-out ${
            noticeVisible
              ? "translate-y-0 opacity-100"
              : "-translate-y-2 opacity-0"
          }`}
        >
          {notice}
        </div>
      )}

        <div className="relative flex flex-1 flex-col">
          <div
            key={viewKey}
            className={`relative z-10 flex flex-1 flex-col ${activeEnterAnimation}`}
          >
            {content}
          </div>
        </div>
      </AppShell>

      {extrasOpen && selectedSeries && (
        <div className="pointer-events-none fixed inset-y-0 left-60 right-0 z-20 overflow-hidden">
          <div
            className={`h-full w-full overflow-y-auto bg-neutral-950 ${
              extrasClosing
                ? "pointer-events-none animate-extras-out-right"
                : "pointer-events-auto animate-extras-in-right"
            }`}
          >
            <div className="mx-auto flex min-h-full max-w-6xl flex-col gap-5 px-8 py-8">
              <SeriesExtras
                series={selectedSeries}
                episodes={extraEpisodes}
                dataRoot={settings?.dataRoot ?? null}
                coverUrl={seriesCoverUrl(selectedSeries)}
                onBack={closeExtras}
                onPlay={(episode) =>
                  openPlayer(selectedSeries, extraEpisodes, episode)
                }
              />
            </div>
          </div>
        </div>
      )}

      {outgoingView && (
        <div
          className={`pointer-events-none fixed ${
            outgoingView.behind ? "z-0" : "z-30"
          } overflow-hidden ${
            outgoingView.exitClass !== undefined
              ? outgoingView.exitClass
              : activeExitAnimation
          }`}
          style={{
            top: outgoingView.rect.top,
            left: outgoingView.rect.left,
            width: outgoingView.rect.width,
            height: outgoingView.rect.height,
          }}
          aria-hidden
        >
          <div ref={outgoingScrollRef} className="h-full overflow-hidden">
            <div className="mx-auto flex min-h-full max-w-6xl flex-col gap-5 px-8 py-8">
              {outgoingView.page}
            </div>
          </div>
        </div>
      )}

      {flipCover &&
        (flipCover.url ? (
          <img
            data-flip-cover
            src={flipCover.url}
            alt=""
            className="pointer-events-none fixed z-[70] rounded-xl border border-neutral-800 object-cover"
            style={{
              left: flipCover.from.left,
              top: flipCover.from.top,
              width: flipCover.from.width,
              height: flipCover.from.height,
              transformOrigin: "top left",
            }}
          />
        ) : (
          <div
            data-flip-cover
            className="pointer-events-none fixed z-[70] flex items-center justify-center overflow-hidden rounded-xl border border-neutral-800 bg-gradient-to-br from-neutral-800 via-neutral-900 to-neutral-950"
            style={{
              left: flipCover.from.left,
              top: flipCover.from.top,
              width: flipCover.from.width,
              height: flipCover.from.height,
              transformOrigin: "top left",
            }}
          >
            <span className="text-4xl font-semibold text-neutral-700">
              {flipCover.label}
            </span>
          </div>
        ))}

      {playerTarget && (
        <PlayerDrawer
          series={playerTarget.series}
          episodes={playerTarget.episodes}
          episode={playerTarget.episode}
          dataRoot={settings?.dataRoot ?? null}
          onClose={closePlayer}
          onSelectEpisode={(nextEpisode) => {
            setPlayerTarget((current) =>
              current ? { ...current, episode: nextEpisode } : current,
            );
          }}
        />
      )}
    </>
  );
}

export default App;
