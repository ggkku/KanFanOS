import { useEffect, useState } from "react";
import {
  AppWindow,
  ArrowUpDown,
  Clapperboard,
  Database,
  FolderCog,
  FolderOpen,
  HardDrive,
  Image,
  Info,
  Power,
  RefreshCw,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { ask } from "@tauri-apps/plugin-dialog";
import { SelectMenu } from "../../components/common/SelectMenu";
import {
  clearThumbnailCache,
  getCacheStats,
  getLibraryStats,
  quitApp,
  setAutoStart,
  setAutoWatch,
  setPageTransition,
  setSeriesTransition,
  setThumbnailWidth,
  setWindowMode,
  startMediaProcessing,
} from "../../api/library";
import { errorMessage } from "../../lib/errors";
import type {
  AppSettings,
  CacheStats,
  LibraryStats,
  PageTransition,
  SeriesTransition,
  WindowMode,
} from "../../types/library";

interface SettingsPageProps {
  settings: AppSettings | null;
  scanning: boolean;
  onChooseRoot: () => void;
  onRescan: () => void;
  onSettingsChange: (settings: AppSettings) => void;
  onCacheCleared: () => void;
}

interface InfoRowProps {
  icon: LucideIcon;
  label: string;
  value: string;
  mono?: boolean;
}

function InfoRow({ icon: Icon, label, value, mono }: InfoRowProps) {
  return (
    <div className="flex items-center gap-4 border-b border-neutral-800/70 px-5 py-3.5 last:border-b-0">
      <Icon size={16} className="shrink-0 text-neutral-500" />
      <div className="w-20 shrink-0 text-xs text-neutral-500">{label}</div>
      <div
        data-selectable
        className={`min-w-0 break-all text-xs text-neutral-300 ${mono ? "font-mono" : ""}`}
      >
        {value}
      </div>
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(
    Math.floor(Math.log(bytes) / Math.log(1024)),
    units.length - 1,
  );
  return `${(bytes / 1024 ** index).toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
}

const actionButton =
  "shrink-0 rounded-lg border border-neutral-800 bg-neutral-900 px-2.5 py-1.5 text-[11px] text-neutral-300 transition-colors hover:bg-neutral-800 disabled:cursor-not-allowed disabled:opacity-40";

const THUMBNAIL_WIDTH_OPTIONS = [320, 480, 640, 960].map((width) => ({
  value: width,
  label: `${width} px`,
}));

const PAGE_TRANSITION_OPTIONS: { value: PageTransition; label: string }[] = [
  { value: "fade", label: "淡入滑动" },
  { value: "slide", label: "整体滑动" },
];

const SERIES_TRANSITION_OPTIONS: { value: SeriesTransition; label: string }[] = [
  { value: "rise", label: "淡入上浮" },
  { value: "zoom", label: "缩放淡入" },
  { value: "slide", label: "从右侧滑入" },
  { value: "flip", label: "封面飞入" },
];

const WINDOW_MODE_OPTIONS: { value: WindowMode; label: string }[] = [
  { value: "windowed", label: "窗口" },
  { value: "borderless", label: "全屏" },
];

export function SettingsPage({
  settings,
  scanning,
  onChooseRoot,
  onRescan,
  onSettingsChange,
  onCacheCleared,
}: SettingsPageProps) {
  const [stats, setStats] = useState<LibraryStats | null>(null);
  const [cache, setCache] = useState<CacheStats | null>(null);
  const [clearing, setClearing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;

    Promise.all([getLibraryStats(), getCacheStats()])
      .then(([nextStats, nextCache]) => {
        if (disposed) return;
        setStats(nextStats);
        setCache(nextCache);
      })
      .catch((err: unknown) => {
        if (!disposed) setError(errorMessage(err));
      });

    return () => {
      disposed = true;
    };
  }, []);

  const hasRoot = settings?.libraryRoot != null;

  const toggleAutoWatch = async () => {
    if (!settings) return;
    setError(null);

    try {
      const next = await setAutoWatch(!settings.autoWatch);
      onSettingsChange(next);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const toggleAutoStart = async () => {
    if (!settings) return;
    setError(null);

    try {
      const next = await setAutoStart(!settings.autoStart);
      onSettingsChange(next);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const changeWidth = async (width: number) => {
    setError(null);

    try {
      const next = await setThumbnailWidth(width);
      onSettingsChange(next);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const changePageTransition = async (transition: PageTransition) => {
    setError(null);

    try {
      const next = await setPageTransition(transition);
      onSettingsChange(next);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const changeWindowMode = async (mode: WindowMode) => {
    setError(null);

    try {
      const next = await setWindowMode(mode);
      onSettingsChange(next);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const changeSeriesTransition = async (transition: SeriesTransition) => {
    setError(null);

    try {
      const next = await setSeriesTransition(transition);
      onSettingsChange(next);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const handleQuit = async () => {
    try {
      await quitApp();
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const clearCache = async () => {
    const confirmed = await ask(
      "清理缩略图缓存？\n清理后会立即重新生成所有视频的缩略图和时长。",
      {
        title: "清理缓存",
        kind: "warning",
        okLabel: "清理",
        cancelLabel: "取消",
      },
    );
    if (!confirmed) return;

    setClearing(true);
    setError(null);

    try {
      await clearThumbnailCache();
      const nextCache = await getCacheStats();
      setCache(nextCache);
      onCacheCleared();

      // 清理会重置所有未失效视频的媒体任务（包括以往因缺工具失败的任务），
      // 立即开始重建，不必等下次启动。
      if (settings?.libraryId != null) {
        try {
          await startMediaProcessing(settings.libraryId);
        } catch (err) {
          const message = errorMessage(err);
          if (!message.includes("已在进行中")) throw err;
        }
      }
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setClearing(false);
    }
  };

  return (
    <div className="flex flex-1 flex-col gap-6">
      <header>
        <h1 className="text-2xl font-semibold">设置</h1>
        <p className="mt-1 text-sm text-neutral-400">
          指定番剧根目录，KanFanOS 会按档期和番剧文件夹建立索引。
        </p>
      </header>

      <section className="overflow-hidden rounded-xl border border-neutral-800 bg-neutral-900/50">
        <div className="flex items-center justify-between gap-4 border-b border-neutral-800/70 px-5 py-4">
          <div className="min-w-0">
            <div className="flex items-center gap-2 text-xs text-neutral-500">
              <FolderOpen size={14} />
              番剧目录
            </div>
            <div
              data-selectable
              className="mt-1.5 break-all font-mono text-xs text-neutral-300"
            >
              {settings?.libraryRoot ?? "尚未设置"}
            </div>
          </div>

          <div className="flex shrink-0 items-center gap-2">
            {hasRoot && (
              <button
                type="button"
                onClick={onRescan}
                disabled={scanning}
                className={`${actionButton} flex items-center gap-1.5`}
              >
                <RefreshCw
                  size={13}
                  className={scanning ? "animate-spin" : ""}
                />
                重新扫描
              </button>
            )}
            <button
              type="button"
              onClick={onChooseRoot}
              className="rounded-lg bg-indigo-600 px-3 py-1.5 text-[11px] font-medium text-white transition-colors hover:bg-indigo-500"
            >
              选择目录
            </button>
          </div>
        </div>

        <div className="flex items-center gap-4 border-b border-neutral-800/70 px-5 py-3.5">
          <RefreshCw size={16} className="shrink-0 text-neutral-500" />
          <div className="w-20 shrink-0 text-xs text-neutral-500">自动同步</div>
          <div className="min-w-0 flex-1 text-xs text-neutral-300">
            {settings?.autoWatch
              ? "已开启 · 文件变化后自动更新索引"
              : "已关闭 · 需要手动重新扫描"}
          </div>
          <button
            type="button"
            onClick={() => void toggleAutoWatch()}
            disabled={!hasRoot}
            className={actionButton}
          >
            {settings?.autoWatch ? "关闭" : "开启"}
          </button>
        </div>

        <div className="flex items-center gap-4 border-b border-neutral-800/70 px-5 py-3.5">
          <Power size={16} className="shrink-0 text-neutral-500" />
          <div className="w-20 shrink-0 text-xs text-neutral-500">
            开机自启动
          </div>
          <div className="min-w-0 flex-1 text-xs text-neutral-300">
            {settings?.autoStart
              ? "已开启 · 登录 Windows 后自动启动"
              : "已关闭 · 需要手动启动"}
          </div>
          <button
            type="button"
            onClick={() => void toggleAutoStart()}
            className={actionButton}
          >
            {settings?.autoStart ? "关闭" : "开启"}
          </button>
        </div>

        <div className="flex items-center gap-4 border-b border-neutral-800/70 px-5 py-3.5">
          <Image size={16} className="shrink-0 text-neutral-500" />
          <div className="w-20 shrink-0 text-xs text-neutral-500">
            缩略图宽度
          </div>
          <SelectMenu
            value={settings?.thumbnailWidth ?? 480}
            options={THUMBNAIL_WIDTH_OPTIONS}
            onChange={(width) => void changeWidth(width)}
            disabled={!hasRoot}
            ariaLabel="缩略图宽度"
          />
          <span className="text-[11px] text-neutral-600">
            仅影响之后生成的缩略图
          </span>
        </div>

        <div className="flex items-center gap-4 border-b border-neutral-800/70 px-5 py-3.5">
          <ArrowUpDown size={16} className="shrink-0 text-neutral-500" />
          <div className="w-20 shrink-0 text-xs text-neutral-500">页面动画</div>
          <SelectMenu
            value={settings?.pageTransition ?? "fade"}
            options={PAGE_TRANSITION_OPTIONS}
            onChange={(transition) => void changePageTransition(transition)}
            ariaLabel="页面动画"
          />
          <span className="text-[11px] text-neutral-600">
            切换左侧页面时的过渡效果
          </span>
        </div>

        <div className="flex items-center gap-4 border-b border-neutral-800/70 px-5 py-3.5">
          <AppWindow size={16} className="shrink-0 text-neutral-500" />
          <div className="w-20 shrink-0 text-xs text-neutral-500">窗口模式</div>
          <SelectMenu
            value={settings?.windowMode ?? "windowed"}
            options={WINDOW_MODE_OPTIONS}
            onChange={(mode) => void changeWindowMode(mode)}
            ariaLabel="窗口模式"
          />
        </div>

        <div className="flex items-center gap-4 border-b border-neutral-800/70 px-5 py-3.5">
          <Clapperboard size={16} className="shrink-0 text-neutral-500" />
          <div className="w-20 shrink-0 text-xs text-neutral-500">选集动画</div>
          <SelectMenu
            value={settings?.seriesTransition ?? "rise"}
            options={SERIES_TRANSITION_OPTIONS}
            onChange={(transition) =>
              void changeSeriesTransition(transition)
            }
            ariaLabel="选集动画"
          />
          <span className="text-[11px] text-neutral-600">
            打开番剧进入选集页时的过渡效果
          </span>
        </div>

        <div className="flex items-center gap-4 border-b border-neutral-800/70 px-5 py-3.5">
          <HardDrive size={16} className="shrink-0 text-neutral-500" />
          <div className="w-20 shrink-0 text-xs text-neutral-500">
            缩略图缓存
          </div>
          <div className="min-w-0 flex-1 text-xs tabular-nums text-neutral-300">
            {cache
              ? `${cache.files.toLocaleString()} 个文件 · ${formatBytes(cache.bytes)}`
              : "统计中…"}
          </div>
          <button
            type="button"
            onClick={() => void clearCache()}
            disabled={clearing || cache == null}
            title="清空缩略图并重新生成所有视频的缩略图和时长"
            className={actionButton}
          >
            {clearing ? "清理中…" : "清理并重建"}
          </button>
        </div>

        <InfoRow
          icon={FolderCog}
          label="番剧配置"
          value="每个番剧文件夹内的 kanfanos.toml（可手动编辑）"
        />
        <InfoRow
          icon={Database}
          label="数据库"
          value={stats?.databasePath ?? "读取中…"}
          mono
        />
        <InfoRow
          icon={Info}
          label="已索引"
          value={
            stats
              ? `${stats.videoCount.toLocaleString()} 个视频`
              : "读取中…"
          }
        />
        <InfoRow icon={Info} label="版本" value="KanFanOS 0.1.1" />
      </section>

      {error && (
        <div className="rounded-lg border border-red-900/60 bg-red-950/40 px-4 py-3 text-sm text-red-300">
          操作失败：{error}
        </div>
      )}

      <p className="text-xs leading-relaxed text-neutral-600">
        首次扫描会在每部番剧文件夹内生成 kanfanos.toml，可编辑显示名、搜索别名和封面；
        已存在的文件不会被覆盖。数据库只保存索引，清理缓存不会删除任何原始视频。
      </p>

      <div className="mt-auto flex justify-end pt-2">
        <button
          type="button"
          onClick={() => void handleQuit()}
          className="flex items-center gap-2 rounded-lg bg-red-600 px-3.5 py-2 text-xs font-medium text-white transition-colors hover:bg-red-500"
        >
          <Power size={14} />
          退出系统
        </button>
      </div>
    </div>
  );
}
