import { useCallback, useEffect, useRef, useState } from "react";
import {
  Loader2,
  Maximize2,
  Minimize2,
  Pause,
  Play,
  SkipBack,
  SkipForward,
  Square,
  Subtitles,
  Volume2,
  VolumeX,
  X,
} from "lucide-react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  onPlayerClosed,
  onPlayerState,
  playerSeek,
  playerSetMuted,
  playerSetPaused,
  playerSetSpeed,
  playerSetVolume,
  playerToggleSubtitles,
  resizePlayer,
  setPlayerFullscreen,
  startPlayer,
  stopPlayer,
} from "../../api/library";
import { errorMessage } from "../../lib/errors";
import { thumbnailUrl } from "../../lib/thumbnails";
import type {
  EpisodeSummary,
  PlayerState,
  SeriesSummary,
} from "../../types/library";

interface PlayerDrawerProps {
  series: SeriesSummary;
  episodes: EpisodeSummary[];
  episode: EpisodeSummary;
  dataRoot: string | null;
  onClose: () => void;
  onSelectEpisode: (episode: EpisodeSummary) => void;
}

function formatTime(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;

  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
  }
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

const iconButton =
  "flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-neutral-300 transition-colors hover:bg-neutral-800 hover:text-white disabled:cursor-not-allowed disabled:opacity-30";

export function PlayerDrawer({
  series,
  episodes,
  episode,
  dataRoot,
  onClose,
  onSelectEpisode,
}: PlayerDrawerProps) {
  const [open, setOpen] = useState(false);
  const [closing, setClosing] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [starting, setStarting] = useState(true);
  const [playerState, setPlayerState] = useState<PlayerState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [seekPreview, setSeekPreview] = useState<number | null>(null);
  const [volumePreview, setVolumePreview] = useState<number | null>(null);

  const videoRef = useRef<HTMLDivElement>(null);
  const openedOnce = useRef(false);

  const syncVideoRect = useCallback(() => {
    const element = videoRef.current;
    if (!element) return;

    const rect = element.getBoundingClientRect();
    void resizePlayer({
      x: rect.x,
      y: rect.y,
      width: rect.width,
      height: rect.height,
    }).catch(() => {
      // 播放器尚未启动时忽略。
    });
  }, []);

  useEffect(() => {
    const frame = requestAnimationFrame(() => setOpen(true));
    return () => cancelAnimationFrame(frame);
  }, []);

  // 启动或切换剧集。首次打开等滑入动画结束后再取视频区域坐标。
  useEffect(() => {
    let disposed = false;
    const delay = openedOnce.current ? 0 : 320;
    openedOnce.current = true;

    setStarting(true);
    setPlayerState(null);
    setError(null);
    setSeekPreview(null);
    setVolumePreview(null);

    const timer = window.setTimeout(async () => {
      const element = videoRef.current;
      if (!element) {
        if (!disposed) {
          setError("找不到视频区域");
          setStarting(false);
        }
        return;
      }

      const rect = element.getBoundingClientRect();

      try {
        await startPlayer(episode.id, {
          x: rect.x,
          y: rect.y,
          width: rect.width,
          height: rect.height,
        });
        if (!disposed) setStarting(false);
      } catch (err) {
        if (!disposed) {
          setError(errorMessage(err));
          setStarting(false);
        }
      }
    }, delay);

    return () => {
      disposed = true;
      window.clearTimeout(timer);
    };
  }, [episode.id]);

  // 订阅播放状态。
  useEffect(() => {
    let disposed = false;
    let unlistenState: UnlistenFn | undefined;
    let unlistenClosed: UnlistenFn | undefined;

    onPlayerState((state) => {
      if (disposed || state.episodeId !== episode.id) return;
      setPlayerState(state);
      setStarting(false);
    }).then((fn) => {
      if (disposed) fn();
      else unlistenState = fn;
    });

    onPlayerClosed((closedId) => {
      if (disposed || closedId !== episode.id) return;
      setStarting(false);
    }).then((fn) => {
      if (disposed) fn();
      else unlistenClosed = fn;
    });

    return () => {
      disposed = true;
      unlistenState?.();
      unlistenClosed?.();
    };
  }, [episode.id]);

  // 窗口尺寸变化时同步原生视频窗口的位置。
  useEffect(() => {
    const element = videoRef.current;
    if (!element) return;

    let timer: number | undefined;
    const observer = new ResizeObserver(() => {
      if (timer) window.clearTimeout(timer);
      timer = window.setTimeout(syncVideoRect, 120);
    });

    observer.observe(element);
    return () => {
      observer.disconnect();
      if (timer) window.clearTimeout(timer);
    };
  }, [syncVideoRect]);

  // 组件卸载时停止播放并退出全屏。
  useEffect(() => {
    return () => {
      void stopPlayer();
      void setPlayerFullscreen(false);
    };
  }, []);

  const handleClose = () => {
    if (closing) return;
    setClosing(true);
    void stopPlayer();

    if (fullscreen) {
      setFullscreen(false);
      void setPlayerFullscreen(false);
    }

    setOpen(false);
    window.setTimeout(onClose, 280);
  };

  const toggleFullscreen = async () => {
    const next = !fullscreen;
    setFullscreen(next);

    try {
      await setPlayerFullscreen(next);
      // 系统窗口尺寸变化完成后再同步一次视频区域。
      window.setTimeout(syncVideoRect, 400);
    } catch (err) {
      setFullscreen(!next);
      setError(errorMessage(err));
    }
  };

  const paused = playerState?.paused ?? false;
  const ended = playerState?.ended ?? false;
  const durationMs = playerState?.durationMs || episode.durationMs || 0;
  const positionMs = seekPreview ?? playerState?.positionMs ?? 0;
  const volume = volumePreview ?? playerState?.volume ?? 100;
  const muted = playerState?.muted ?? false;

  const currentIndex = episodes.findIndex((item) => item.id === episode.id);
  const previous = currentIndex > 0 ? episodes[currentIndex - 1] : null;
  const next =
    currentIndex >= 0 && currentIndex < episodes.length - 1
      ? episodes[currentIndex + 1]
      : null;

  const togglePlay = () => {
    if (!playerState) return;

    if (ended) {
      void playerSeek(0);
      void playerSetPaused(false);
      return;
    }

    void playerSetPaused(!playerState.paused);
  };

  const commitSeek = () => {
    if (seekPreview != null) {
      void playerSeek(seekPreview);
      setSeekPreview(null);
    }
  };

  // 键盘快捷键
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      const tag = (event.target as HTMLElement | null)?.tagName;
      if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") return;

      if (event.key === "Escape") {
        if (fullscreen) {
          void toggleFullscreen();
        } else {
          handleClose();
        }
      } else if (event.key === " ") {
        event.preventDefault();
        togglePlay();
      } else if (event.key === "ArrowLeft") {
        event.preventDefault();
        void playerSeek(Math.max(0, (playerState?.positionMs ?? 0) - 5000));
      } else if (event.key === "ArrowRight") {
        event.preventDefault();
        void playerSeek((playerState?.positionMs ?? 0) + 5000);
      }
    };

    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  });

  return (
    <>
      {!fullscreen && (
        <div
          className={`fixed inset-0 z-40 bg-black/60 transition-opacity duration-300 ${
            open && !closing ? "opacity-100" : "opacity-0"
          }`}
          onClick={handleClose}
        />
      )}

      <aside
        className={`fixed right-0 top-0 z-50 flex h-full flex-col bg-neutral-950 shadow-2xl transition-transform duration-300 ease-out ${
          fullscreen
            ? "w-screen border-l-0"
            : "w-[min(880px,76vw)] border-l border-neutral-800"
        } ${open && !closing ? "translate-x-0" : "translate-x-full"}`}
      >
        {!fullscreen && (
          <header className="flex shrink-0 items-center justify-between gap-4 border-b border-neutral-800 px-5 py-3">
            <div className="min-w-0">
              <div className="truncate text-sm font-medium text-neutral-100">
                {series.name}
              </div>
              <div className="truncate text-[11px] text-neutral-500">
                {episode.filename}
              </div>
            </div>
            <button
              type="button"
              onClick={handleClose}
              className={iconButton}
              title="关闭播放器"
            >
              <X size={16} />
            </button>
          </header>
        )}

        <div
          ref={videoRef}
          className={`relative w-full bg-black ${
            fullscreen ? "min-h-0 flex-1" : "aspect-video shrink-0"
          }`}
        >
          {!error && (!playerState || starting) && (
            <div className="absolute bottom-2 right-3 flex items-center gap-2 rounded bg-black/60 px-2 py-1 text-[11px] text-neutral-400">
              <Loader2 size={12} className="animate-spin" />
              正在启动播放器…
            </div>
          )}
        </div>

        {error && (
          <div className="shrink-0 border-b border-red-900/60 bg-red-950/40 px-4 py-2 text-xs text-red-300">
            {error}
          </div>
        )}

        <div className="shrink-0 border-b border-neutral-800 bg-neutral-900/60 px-4 py-3">
          <div className="flex items-center gap-3">
            <span className="w-14 text-right text-[11px] tabular-nums text-neutral-400">
              {formatTime(positionMs)}
            </span>
            <input
              type="range"
              min={0}
              max={Math.max(1, durationMs)}
              value={Math.min(positionMs, Math.max(1, durationMs))}
              onChange={(event) => setSeekPreview(Number(event.target.value))}
              onPointerUp={commitSeek}
              onKeyUp={commitSeek}
              disabled={!playerState || durationMs <= 0}
              className="h-1 flex-1 cursor-pointer accent-indigo-500 disabled:cursor-not-allowed"
            />
            <span className="w-14 text-[11px] tabular-nums text-neutral-500">
              {formatTime(durationMs)}
            </span>
          </div>

          <div className="mt-3 flex items-center gap-1.5">
            <button
              type="button"
              onClick={() => previous && onSelectEpisode(previous)}
              disabled={!previous}
              className={iconButton}
              title="上一集"
            >
              <SkipBack size={16} />
            </button>

            <button
              type="button"
              onClick={togglePlay}
              disabled={!playerState}
              className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-indigo-600 text-white transition-colors hover:bg-indigo-500 disabled:cursor-not-allowed disabled:opacity-40"
              title={paused ? "播放" : "暂停"}
            >
              {paused ? (
                <Play size={18} fill="currentColor" />
              ) : (
                <Pause size={18} fill="currentColor" />
              )}
            </button>

            <button
              type="button"
              onClick={() => next && onSelectEpisode(next)}
              disabled={!next}
              className={iconButton}
              title="下一集"
            >
              <SkipForward size={16} />
            </button>

            <button
              type="button"
              onClick={handleClose}
              className={iconButton}
              title="停止并关闭"
            >
              <Square size={13} fill="currentColor" />
            </button>

            <div className="mx-2 h-5 w-px bg-neutral-800" />

            <button
              type="button"
              onClick={() => playerState && void playerSetMuted(!muted)}
              disabled={!playerState}
              className={iconButton}
              title={muted ? "取消静音" : "静音"}
            >
              {muted ? <VolumeX size={16} /> : <Volume2 size={16} />}
            </button>
            <input
              type="range"
              min={0}
              max={100}
              value={muted ? 0 : Math.round(volume)}
              onChange={(event) => {
                const value = Number(event.target.value);
                setVolumePreview(value);
                void playerSetVolume(value);
              }}
              onPointerUp={() => setVolumePreview(null)}
              disabled={!playerState}
              className="h-1 w-24 cursor-pointer accent-indigo-500 disabled:cursor-not-allowed"
              title="音量"
            />
            <span className="w-8 text-[11px] tabular-nums text-neutral-500">
              {Math.round(muted ? 0 : volume)}
            </span>

            <div className="ml-auto flex items-center gap-1.5">
              <select
                value={playerState?.speed ?? 1}
                onChange={(event) =>
                  void playerSetSpeed(Number(event.target.value))
                }
                disabled={!playerState}
                className="rounded-md border border-neutral-800 bg-neutral-950 px-1.5 py-1 text-[11px] text-neutral-300 focus:border-indigo-500/60 focus:outline-none disabled:opacity-40"
                title="播放速度"
              >
                {[0.5, 0.75, 1, 1.25, 1.5, 2].map((speed) => (
                  <option key={speed} value={speed}>
                    {speed}×
                  </option>
                ))}
              </select>

              <button
                type="button"
                onClick={() => void playerToggleSubtitles()}
                disabled={!playerState}
                className={iconButton}
                title="切换字幕"
              >
                <Subtitles size={16} />
              </button>

              <button
                type="button"
                onClick={() => void toggleFullscreen()}
                className={iconButton}
                title={fullscreen ? "退出全屏" : "全屏"}
              >
                {fullscreen ? (
                  <Minimize2 size={16} />
                ) : (
                  <Maximize2 size={16} />
                )}
              </button>
            </div>
          </div>
        </div>

        {!fullscreen && (
          <div className="min-h-0 flex-1 overflow-y-auto">
            <div className="px-4 pb-1 pt-3 text-[10px] uppercase tracking-widest text-neutral-600">
              剧集 · {episodes.length}
            </div>

            {episodes.map((item, index) => {
            const active = item.id === episode.id;
            const url = thumbnailUrl(dataRoot, item.thumbnailKey);

            return (
              <button
                key={item.id}
                type="button"
                onClick={() => onSelectEpisode(item)}
                className={`flex w-full items-center gap-3 border-b border-neutral-900 px-4 py-2 text-left transition-colors ${
                  active
                    ? "bg-indigo-500/10 text-neutral-100"
                    : "text-neutral-400 hover:bg-neutral-900 hover:text-neutral-200"
                }`}
              >
                <span className="w-7 shrink-0 text-right text-[11px] tabular-nums text-neutral-600">
                  {index + 1}
                </span>

                <div className="h-9 w-16 shrink-0 overflow-hidden rounded bg-neutral-800">
                  {url ? (
                    <img
                      src={url}
                      alt=""
                      loading="lazy"
                      className="h-full w-full object-cover"
                    />
                  ) : null}
                </div>

                <span className="min-w-0 flex-1 truncate text-xs">
                  {item.filename}
                </span>

                <span className="shrink-0 text-[11px] tabular-nums text-neutral-600">
                  {formatTime(item.durationMs ?? 0)}
                </span>
              </button>
            );
            })}
          </div>
        )}
      </aside>
    </>
  );
}
