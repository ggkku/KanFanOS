import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { Loader2, Search, Trophy, X } from "lucide-react";
import {
  checkSeasonLinks,
  clearSeriesHistory,
  markSeriesWatched,
  updateSeriesConfig,
} from "../../api/library";
import { errorMessage } from "../../lib/errors";
import { ConfirmDialog } from "../common/ConfirmDialog";
import type {
  EpisodeMode,
  SeasonLinkConflict,
  SeriesConfigView,
  SeriesOption,
  SeriesSummary,
} from "../../types/library";
import { SeriesPickerDialog } from "./SeriesPickerDialog";
import { SeriesEpisodePicker } from "./SeriesEpisodePicker";

interface SeriesConfigDialogProps {
  series: SeriesSummary;
  config: SeriesConfigView;
  onClose: () => void;
  onSaved: (config: SeriesConfigView) => void;
  /// 关闭弹窗，回到选集页右上角输入奖项。
  onAddAward: () => void;
  awards: string[];
  onRemoveAward: (name: string) => Promise<void>;
  onChanged: () => Promise<void>;
}

function parseAliases(value: string): string[] {
  const aliases = value
    .split(/[\n,，;；]+/)
    .map((alias) => alias.trim())
    .filter(Boolean);

  return Array.from(new Set(aliases));
}

const actionButton =
  "rounded-lg border border-neutral-800 bg-neutral-900 px-2.5 py-1.5 text-[11px] text-neutral-300 transition-colors hover:bg-neutral-800 disabled:cursor-not-allowed disabled:opacity-40";

interface SeasonFieldProps {
  label: string;
  value: SeriesOption | null;
  onPick: () => void;
}

/// 季链接字段：点击后打开搜索式选择器，而不是一次性加载全部番剧。
function SeasonField({ label, value, onPick }: SeasonFieldProps) {
  return (
    <div>
      <span className="mb-1.5 block text-xs text-neutral-400">{label}</span>
      <button
        type="button"
        onClick={onPick}
        aria-label={`选择${label}`}
        className="flex w-full items-center justify-between gap-2 rounded-lg border border-neutral-800 bg-neutral-900 px-3 py-2 text-left text-xs transition-colors hover:bg-neutral-800"
      >
        <span
          className={`min-w-0 truncate ${value ? "text-neutral-200" : "text-neutral-500"}`}
        >
          {value ? `${value.name} · ${value.category}` : "搜索选择番剧"}
        </span>
        <Search size={12} className="shrink-0 text-neutral-600" />
      </button>
    </div>
  );
}

/// 选集页右上角「配置」弹窗：显示名、别名、季链接和观看记录操作。
export function SeriesConfigDialog({
  series,
  config,
  onClose,
  onSaved,
  onAddAward,
  awards,
  onRemoveAward,
  onChanged,
}: SeriesConfigDialogProps) {
  const [name, setName] = useState(config.name);
  const [aliases, setAliases] = useState(config.aliases.join("\n"));
  const [previous, setPrevious] = useState<SeriesOption | null>(
    config.previous,
  );
  const [next, setNext] = useState<SeriesOption | null>(config.next);
  const [picker, setPicker] = useState<"previous" | "next" | null>(null);
  const [episodeMode, setEpisodeMode] = useState<EpisodeMode>(
    config.episodeMode ?? "auto",
  );
  const [episodes, setEpisodes] = useState<string[]>(config.episodes ?? []);
  const [episodePickerOpen, setEpisodePickerOpen] = useState(false);
  const [conflicts, setConflicts] = useState<SeasonLinkConflict[] | null>(
    null,
  );
  const [saving, setSaving] = useState(false);
  const [actionBusy, setActionBusy] = useState<"watched" | "clear" | null>(
    null,
  );
  const [awardBusy, setAwardBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // Esc 关闭；保存、批量操作、选择器或确认弹窗打开时不关闭。
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (
        event.key === "Escape" &&
        !saving &&
        actionBusy == null &&
        picker == null &&
        !episodePickerOpen &&
        conflicts == null
      ) {
        onClose();
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    actionBusy,
    conflicts,
    episodePickerOpen,
    picker,
    saving,
    onClose,
  ]);

  const removeAward = async (name: string) => {
    if (awardBusy) return;

    setAwardBusy(true);
    setError(null);
    try {
      await onRemoveAward(name);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setAwardBusy(false);
    }
  };

  const runAction = async (kind: "watched" | "clear") => {
    if (saving || actionBusy != null) return;

    setActionBusy(kind);
    setError(null);
    setNotice(null);
    try {
      const affected =
        kind === "watched"
          ? await markSeriesWatched(series.id)
          : await clearSeriesHistory(series.id);
      await onChanged();
      setNotice(
        kind === "watched"
          ? `已把 ${affected} 个视频标记为看完`
          : `已清除 ${affected} 条观看记录`,
      );
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setActionBusy(null);
    }
  };

  const persist = async (overwriteInverse: boolean) => {
    setSaving(true);
    setError(null);
    try {
      const updated = await updateSeriesConfig(
        series.id,
        name,
        parseAliases(aliases),
        previous?.id ?? null,
        next?.id ?? null,
        overwriteInverse,
        episodeMode,
        episodes,
      );
      await onChanged();
      onSaved(updated);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const save = async () => {
    if (saving || actionBusy != null) return;

    setSaving(true);
    setError(null);
    try {
      const nextConflicts = await checkSeasonLinks(
        series.id,
        previous?.id ?? null,
        next?.id ?? null,
      );
      if (nextConflicts.length > 0) {
        setConflicts(nextConflicts);
        setSaving(false);
        return;
      }
    } catch (err) {
      setError(errorMessage(err));
      setSaving(false);
      return;
    }

    await persist(true);
  };

  const resolveConflicts = (overwriteInverse: boolean) => {
    setConflicts(null);
    void persist(overwriteInverse);
  };

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget && !saving) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="番剧配置"
        className="flex max-h-[calc(100vh-2rem)] w-full max-w-lg flex-col rounded-2xl border border-neutral-800 bg-neutral-950 shadow-2xl"
      >
        <header className="flex shrink-0 items-center justify-between border-b border-neutral-800 px-5 py-3.5">
          <div>
            <h2 className="text-sm font-medium text-neutral-100">番剧配置</h2>
            <p className="mt-0.5 text-[11px] text-neutral-500">{series.name}</p>
          </div>
          <button
            type="button"
            onClick={onClose}
            disabled={saving}
            aria-label="关闭"
            className="flex h-7 w-7 items-center justify-center rounded-lg text-neutral-500 transition-colors hover:bg-neutral-800 hover:text-neutral-200 disabled:opacity-40"
          >
            <X size={15} />
          </button>
        </header>

        <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-5 py-4">
          <label className="block">
            <span className="mb-1.5 block text-xs text-neutral-400">
              显示名称
            </span>
            <input
              autoFocus
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder={`留空则使用「${series.name}」`}
              className="w-full rounded-lg border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm text-neutral-100 outline-none transition-colors placeholder:text-neutral-600 focus:border-indigo-600"
            />
          </label>

          <label className="block">
            <span className="mb-1.5 block text-xs text-neutral-400">
              搜索别名
            </span>
            <textarea
              rows={3}
              value={aliases}
              onChange={(event) => setAliases(event.target.value)}
              placeholder="每行一个，也可以用逗号分隔"
              className="w-full resize-none rounded-lg border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm leading-relaxed text-neutral-100 outline-none transition-colors placeholder:text-neutral-600 focus:border-indigo-600"
            />
          </label>

          <div className="grid grid-cols-2 gap-3">
            <SeasonField
              label="上一季"
              value={previous}
              onPick={() => setPicker("previous")}
            />
            <SeasonField
              label="下一季"
              value={next}
              onPick={() => setPicker("next")}
            />
          </div>
          <p className="-mt-1 text-[10px] leading-relaxed text-neutral-600">
            保存时会自动同步目标的上一季 / 下一季；目标已有其它链接时会先询问。
          </p>

          <div className="rounded-xl border border-neutral-800 bg-neutral-900/40 px-3.5 py-3">
            <div className="flex items-center justify-between gap-3">
              <div>
                <div className="text-xs text-neutral-300">选集来源</div>
                <div className="mt-0.5 text-[11px] text-neutral-500">
                  {episodeMode === "auto"
                    ? "根目录视频自动成为选集"
                    : `手动选择：${episodes.length} 个视频`}
                </div>
              </div>
              <div className="flex shrink-0 items-center gap-1 rounded-lg border border-neutral-800 bg-neutral-950 p-0.5 text-[11px]">
                {(["auto", "manual"] as const).map((mode) => (
                  <button
                    key={mode}
                    type="button"
                    onClick={() => setEpisodeMode(mode)}
                    className={`rounded-md px-2 py-1 transition-colors ${
                      episodeMode === mode
                        ? "bg-neutral-800 text-neutral-100"
                        : "text-neutral-500 hover:text-neutral-300"
                    }`}
                  >
                    {mode === "auto" ? "自动" : "手动"}
                  </button>
                ))}
              </div>
            </div>

            {episodeMode === "manual" && (
              <div className="mt-2.5 flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => setEpisodePickerOpen(true)}
                  disabled={saving}
                  className={actionButton}
                >
                  选择视频…
                </button>
                <span className="text-[11px] text-neutral-600">
                  未选中的视频进入「其它视频」
                </span>
              </div>
            )}
          </div>

          <div className="rounded-xl border border-neutral-800 bg-neutral-900/40 px-3.5 py-3">
            <div className="flex items-center justify-between gap-3">
              <div>
                <div className="text-xs text-neutral-300">奖项</div>
                <div className="mt-0.5 text-[11px] text-neutral-500">
                  回到选集页右上角输入奖项名称
                </div>
              </div>
              <button
                type="button"
                onClick={onAddAward}
                disabled={saving}
                className={actionButton}
              >
                添加奖项
              </button>
            </div>

            {awards.length > 0 ? (
              <ul className="mt-2.5 space-y-1">
                {awards.map((award) => (
                  <li
                    key={award}
                    className="flex items-center gap-2 rounded-lg border border-amber-900/40 bg-amber-950/20 px-2.5 py-1"
                  >
                    <Trophy size={12} className="shrink-0 text-amber-400" />
                    <span className="min-w-0 flex-1 truncate text-xs text-amber-200">
                      {award}
                    </span>
                    <button
                      type="button"
                      onClick={() => void removeAward(award)}
                      disabled={awardBusy || saving}
                      className="shrink-0 rounded px-1.5 py-0.5 text-[10px] text-neutral-500 transition-colors hover:bg-red-950/40 hover:text-red-300 disabled:cursor-not-allowed disabled:opacity-40"
                    >
                      删除
                    </button>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="mt-2 text-[11px] text-neutral-600">暂无奖项</p>
            )}
          </div>

          <div className="rounded-xl border border-neutral-800 bg-neutral-900/40 px-3.5 py-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div>
                <div className="text-xs text-neutral-300">观看记录</div>
                <div className="mt-0.5 text-[11px] text-neutral-500">
                  作用于本番剧的全部剧集，包括其他视频
                </div>
              </div>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => void runAction("watched")}
                  disabled={saving || actionBusy != null}
                  className={actionButton}
                >
                  {actionBusy === "watched"
                    ? "标记中…"
                    : "全部标记已看完"}
                </button>
                <button
                  type="button"
                  onClick={() => void runAction("clear")}
                  disabled={saving || actionBusy != null}
                  className={`${actionButton} hover:border-red-900/60 hover:bg-red-950/30 hover:text-red-300`}
                >
                  {actionBusy === "clear" ? "清除中…" : "清除观看记录"}
                </button>
              </div>
            </div>
            {notice && (
              <p className="mt-2 text-[11px] text-emerald-400">{notice}</p>
            )}
          </div>

          {error && <p className="text-[11px] text-red-400">{error}</p>}
        </div>

        <footer className="flex shrink-0 items-center justify-end gap-2 border-t border-neutral-800 px-5 py-3">
          <button
            type="button"
            onClick={onClose}
            disabled={saving}
            className="rounded-lg border border-neutral-800 bg-neutral-900 px-3.5 py-1.5 text-xs text-neutral-300 transition-colors hover:bg-neutral-800 disabled:opacity-40"
          >
            取消
          </button>
          <button
            type="button"
            onClick={() => void save()}
            disabled={saving || actionBusy != null}
            className="flex items-center gap-1.5 rounded-lg bg-indigo-600 px-3.5 py-1.5 text-xs font-medium text-white transition-colors hover:bg-indigo-500 disabled:cursor-not-allowed disabled:opacity-40"
          >
            {saving && <Loader2 size={12} className="animate-spin" />}
            {saving ? "保存中…" : "保存"}
          </button>
        </footer>
      </div>

      {picker && (
        <SeriesPickerDialog
          title={picker === "previous" ? "选择上一季" : "选择下一季"}
          libraryId={series.libraryId}
          excludeId={series.id}
          selectedId={(picker === "previous" ? previous : next)?.id ?? null}
          // 同一系列的前后季名称通常很接近，预填前两个字省一次输入。
          initialQuery={series.name.trim().slice(0, 2)}
          onSelect={(option) => {
            if (picker === "previous") setPrevious(option);
            else setNext(option);
            setPicker(null);
          }}
          onClose={() => setPicker(null)}
        />
      )}

      {episodePickerOpen && (
        <SeriesEpisodePicker
          seriesId={series.id}
          selected={episodes}
          onConfirm={(paths) => {
            setEpisodes(paths);
            setEpisodePickerOpen(false);
          }}
          onClose={() => setEpisodePickerOpen(false)}
        />
      )}

      {conflicts && (
        <ConfirmDialog
          title="自动设置反向季链接"
          confirmLabel="一并修改"
          cancelLabel="保留原设置"
          busy={saving}
          onConfirm={() => resolveConflicts(true)}
          onCancel={() => resolveConflicts(false)}
        >
          <p>保存后，以下番剧的反向季链接需要修改：</p>
          <ul className="mt-3 space-y-2">
            {conflicts.map((conflict, index) => {
              const side = conflict.direction === "next" ? "下一季" : "上一季";
              const currentName =
                conflict.current?.name ?? "一个已失效的番剧";

              return (
                <li
                  key={`${conflict.target.id}-${conflict.direction}-${index}`}
                  className="rounded-lg border border-neutral-800 bg-neutral-900/60 px-3 py-2"
                >
                  「{conflict.target.name}」的{side}当前是「{currentName}」，
                  是否改为「{series.name}」？
                </li>
              );
            })}
          </ul>
          <p className="mt-3 text-[11px] text-neutral-500">
            「一并修改」会覆盖并解除对方原来的配对；「保留原设置」只保存本番剧的链接。
          </p>
        </ConfirmDialog>
      )}
    </div>,
    document.body,
  );
}
