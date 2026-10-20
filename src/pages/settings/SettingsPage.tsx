import { useEffect, useState } from "react";
import {
  Database,
  FolderCog,
  FolderOpen,
  HardDrive,
  Image,
  Info,
  RefreshCw,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { ask } from "@tauri-apps/plugin-dialog";
import {
  clearThumbnailCache,
  getCacheStats,
  getLibraryStats,
  setAutoWatch,
  setThumbnailWidth,
} from "../../api/library";
import { errorMessage } from "../../lib/errors";
import type {
  AppSettings,
  CacheStats,
  LibraryStats,
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
  const [message, setMessage] = useState<string | null>(null);

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
    setMessage(null);

    try {
      const next = await setAutoWatch(!settings.autoWatch);
      onSettingsChange(next);
      setMessage(
        next.autoWatch
          ? "已开启自动同步。"
          : "已关闭自动同步，目录变化需要手动重新扫描。",
      );
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const changeWidth = async (width: number) => {
    setError(null);
    setMessage(null);

    try {
      const next = await setThumbnailWidth(width);
      onSettingsChange(next);
      setMessage(
        `缩略图宽度已设为 ${next.thumbnailWidth} px。仅影响之后生成的缩略图，清理缓存可全部重建。`,
      );
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const clearCache = async () => {
    const confirmed = await ask(
      "清理缩略图缓存？\n缓存会在下次启动或重新扫描时自动重建。",
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
    setMessage(null);

    try {
      const result = await clearThumbnailCache();
      const nextCache = await getCacheStats();
      setCache(nextCache);
      onCacheCleared();
      setMessage(
        `已清理 ${result.removedFiles.toLocaleString()} 个文件，释放 ${formatBytes(result.removedBytes)}。`,
      );
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
          <Image size={16} className="shrink-0 text-neutral-500" />
          <div className="w-20 shrink-0 text-xs text-neutral-500">
            缩略图宽度
          </div>
          <select
            value={settings?.thumbnailWidth ?? 480}
            onChange={(event) =>
              void changeWidth(Number(event.target.value))
            }
            disabled={!hasRoot}
            className="rounded-lg border border-neutral-800 bg-neutral-900 px-2 py-1 text-xs text-neutral-300 focus:border-indigo-500/60 focus:outline-none disabled:opacity-40"
          >
            <option value={320}>320 px</option>
            <option value={480}>480 px</option>
            <option value={640}>640 px</option>
            <option value={960}>960 px</option>
          </select>
          <span className="text-[11px] text-neutral-600">
            仅影响之后生成的缩略图
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
            disabled={clearing || cache == null || cache.files === 0}
            className={actionButton}
          >
            {clearing ? "清理中…" : "清理"}
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
        <InfoRow icon={Info} label="版本" value="KanFanOS 0.1.0" />
      </section>

      {message && (
        <div className="rounded-lg border border-emerald-900/60 bg-emerald-950/30 px-4 py-3 text-sm text-emerald-300">
          {message}
        </div>
      )}

      {error && (
        <div className="rounded-lg border border-red-900/60 bg-red-950/40 px-4 py-3 text-sm text-red-300">
          操作失败：{error}
        </div>
      )}

      <p className="text-xs leading-relaxed text-neutral-600">
        首次扫描会在每部番剧文件夹内生成 kanfanos.toml，可编辑显示名、搜索别名和封面；
        已存在的文件不会被覆盖。数据库只保存索引，清理缓存不会删除任何原始视频。
      </p>
    </div>
  );
}
