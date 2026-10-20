import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { Loader2 } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { AppShell } from "./components/layout/AppShell";
import { MediaBanner } from "./components/media/MediaBanner";
import { ScanBanner } from "./components/scan/ScanBanner";
import { SeriesDetail } from "./components/series/SeriesDetail";
import { SetupScreen } from "./components/setup/SetupScreen";
import { CategoryPage } from "./pages/category/CategoryPage";
import { RecentPage } from "./pages/recent/RecentPage";
import { SearchPage } from "./pages/search/SearchPage";
import { SettingsPage } from "./pages/settings/SettingsPage";
import {
  cancelMediaProcessing,
  cancelScan,
  getMediaStats,
  getSettings,
  listCategories,
  listEpisodes,
  listRecent,
  listSeries,
  onLibrarySync,
  onMediaProgress,
  onScanProgress,
  playEpisode,
  scanLibrary,
  searchSeries,
  setLibraryRoot,
  startMediaProcessing,
} from "./api/library";
import { errorMessage } from "./lib/errors";
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
} from "./types/library";
import type { View } from "./types/navigation";

const PAGE_SIZE = 60;

function App() {
  const [view, setView] = useState<View>({ kind: "recent" });
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
  const [selectedSeries, setSelectedSeries] = useState<SeriesSummary | null>(
    null,
  );
  const [episodes, setEpisodes] = useState<EpisodeSummary[]>([]);
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
  const [refreshKey, setRefreshKey] = useState(0);

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
          const media = await getMediaStats(nextSettings.libraryId);
          if (!disposed && media.pending + media.processing > 0) {
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
      setSeries([]);
      setSeriesTotal(0);
      return;
    }

    const libraryId = settings.libraryId;
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
      setSearchItems([]);
      setSearchTotal(0);
      setSearchLoading(false);
      return;
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

    listRecent(libraryId, 50)
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

  const navigate = (next: View) => {
    setSelectedSeries(null);
    setEpisodes([]);
    setQuery("");
    setView(next);
  };

  const handleQueryChange = (value: string) => {
    setQuery(value);
    if (value.trim().length > 0) {
      setSelectedSeries(null);
      setEpisodes([]);
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

  const runScan = useCallback(async (libraryId: number) => {
    setScanning(true);
    setError(null);
    setNotice(null);
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
        setNotice(
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
        setNotice(
          `扫描完成：${parts.join("，")}（${(summary.elapsedMs / 1000).toFixed(1)} 秒）。`,
        );
      }
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setScanning(false);
      setProgress(null);
    }
  }, []);

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
      setNotice(`已设置番剧目录：${library.rootPath}`);
      await runScan(library.id);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const openSeries = async (target: SeriesSummary) => {
    setSelectedSeries(target);
    try {
      const rows = await listEpisodes(target.id);
      const collator = new Intl.Collator("zh-CN", {
        numeric: true,
        sensitivity: "base",
      });
      rows.sort((a, b) => collator.compare(a.filename, b.filename));
      setEpisodes(rows);
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

  const play = async (episodeId: number) => {
    try {
      await playEpisode(episodeId);
      setRecentVersion((value) => value + 1);
    } catch (err) {
      setError(errorMessage(err));
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
        coverUrl={seriesCoverUrl(selectedSeries, settings?.dataRoot ?? null)}
        onBack={() => {
          setSelectedSeries(null);
          setEpisodes([]);
        }}
        onPlay={(episode) => void play(episode.id)}
      />
    );
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
        dataRoot={settings?.dataRoot ?? null}
        onOpenSeries={(item) => void openSeries(item)}
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
        onPlay={(videoId) => void play(videoId)}
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
        dataRoot={settings?.dataRoot ?? null}
        onSortChange={setSort}
        onOpenSeries={(item) => void openSeries(item)}
        onRescan={rescan}
        onLoadMore={() => void loadMoreSeries()}
      />
    );
  }

  return (
    <AppShell
      view={view}
      categories={categories}
      scanning={scanning}
      syncing={syncing}
      query={query}
      onQueryChange={handleQueryChange}
      onNavigate={navigate}
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

      {error && (
        <div className="rounded-lg border border-red-900/60 bg-red-950/40 px-4 py-3 text-sm text-red-300">
          {error}
        </div>
      )}

      {notice && (
        <div className="rounded-lg border border-emerald-900/60 bg-emerald-950/30 px-4 py-3 text-sm text-emerald-300">
          {notice}
        </div>
      )}

      {content}
    </AppShell>
  );
}

export default App;
