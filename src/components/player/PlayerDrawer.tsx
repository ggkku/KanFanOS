import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import {
  AudioLines,
  Captions,
  CaptionsOff,
  Check,
  Loader2,
  Maximize2,
  Minimize2,
  Pause,
  Play,
  SkipBack,
  SkipForward,
  Volume2,
  VolumeX,
  X,
} from "lucide-react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  onPlayerClosed,
  onPlayerClick,
  onPlayerError,
  onPlayerMouseActivity,
  onPlayerState,
  playerSeek,
  playerSetAudio,
  playerSetMuted,
  playerSetPaused,
  playerSetSpeed,
  playerSetSubDelay,
  playerSetSubtitle,
  playerSetVolume,
  resizePlayer,
  setPlayerCursorVisible,
  startPlayer,
  stopPlayer,
} from "../../api/library";
import { errorMessage } from "../../lib/errors";
import { thumbnailUrl } from "../../lib/thumbnails";
import type {
  AudioTrack,
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

/// 把视频区域矩形按视频宽高比居中收缩，得到实际画面窗口。
/// 这样原生窗口始终与视频同比例，宽高永远一起变化。
function fitToAspect(
  rect: { x: number; y: number; width: number; height: number },
  aspect: number,
) {
  let width = rect.width;
  let height = width / aspect;

  if (height > rect.height) {
    height = rect.height;
    width = height * aspect;
  }

  return {
    x: rect.x + (rect.width - width) / 2,
    y: rect.y + (rect.height - height) / 2,
    width,
    height,
  };
}

function TrackChip({
  label,
  selected,
  onClick,
}: {
  label: string;
  selected: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex max-w-56 items-center gap-1 rounded-full px-3 py-1 text-xs transition-colors ${
        selected
          ? "bg-indigo-600 text-white"
          : "bg-neutral-800 text-neutral-300 hover:bg-neutral-700"
      }`}
    >
      {selected ? <Check size={12} className="shrink-0" /> : null}
      <span className="truncate">{label}</span>
    </button>
  );
}

function audioTrackLabel(track: AudioTrack): string {
  return track.title || track.lang || `音轨 ${track.id}`;
}

export function PlayerDrawer({
  series,
  episodes,
  episode,
  dataRoot,
  onClose,
  onSelectEpisode,
}: PlayerDrawerProps) {
  const [open, setOpen] = useState(false);
  // 抽屉滑入动画结束、布局稳定之前不启动 mpv，否则按动画中的坐标定位原生窗口会错位。
  const [entered, setEntered] = useState(false);
  const [closing, setClosing] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [starting, setStarting] = useState(true);
  const [playerState, setPlayerState] = useState<PlayerState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [seekPreview, setSeekPreview] = useState<number | null>(null);
  // 松手后先保持目标位置，等播放器回报接近目标后再跟随实时进度。
  const [seekTarget, setSeekTarget] = useState<number | null>(null);
  const [volumePreview, setVolumePreview] = useState<number | null>(null);
  const [subtitleMenuOpen, setSubtitleMenuOpen] = useState(false);
  const [audioMenuOpen, setAudioMenuOpen] = useState(false);
  // 字幕整体时间偏移；正数延后、负数提前。每次换集 mpv 都是新进程，重置为 0。
  const [subtitleDelayMs, setSubtitleDelayMs] = useState(0);
  // 全屏时控制栏自动隐藏，鼠标移动时重新出现。
  const [controlsVisible, setControlsVisible] = useState(true);
  const [activityTick, setActivityTick] = useState(0);

  const videoRef = useRef<HTMLDivElement>(null);
  const drawerRef = useRef<HTMLElement>(null);
  // 视频区域上一次的布局矩形，以及正在运行的 FLIP 动画。
  const videoRectRef = useRef<DOMRect | null>(null);
  const flipAnimRef = useRef<Animation | null>(null);
  const flipFrameRef = useRef<number | null>(null);
  const lastFullscreenRef = useRef(fullscreen);
  const subtitleDelayRef = useRef(0);

  const hideControls = useCallback(() => {
    setSubtitleMenuOpen(false);
    setAudioMenuOpen(false);
    setControlsVisible(false);
  }, []);
  // 控制栏：全屏切换时逐帧跟随视频矩形。
  const controlsRef = useRef<HTMLDivElement>(null);

  // 用视频自身的宽高比，而不是容器宽高比，避免原生窗口在变化过程中
  // 被拉成不同比例，导致画面宽度滞后于高度。
  const videoAspect =
    episode.width != null && episode.height != null && episode.height > 0
      ? episode.width / episode.height
      : 16 / 9;

  const syncVideoRect = useCallback(() => {
    const element = videoRef.current;
    if (!element) return;

    const rect = element.getBoundingClientRect();
    void resizePlayer(fitToAspect(rect, videoAspect)).catch(() => {
      // 播放器尚未启动时忽略。
    });
  }, [videoAspect]);

  useEffect(() => {
    const frame = requestAnimationFrame(() => setOpen(true));
    // 系统关闭动画时 transitionend 不会触发，用超时兜底。
    const fallback = window.setTimeout(() => setEntered(true), 450);
    return () => {
      cancelAnimationFrame(frame);
      window.clearTimeout(fallback);
    };
  }, []);

  // 启动或切换剧集。首次打开必须等抽屉滑入动画结束，否则取到的是动画中的坐标。
  useEffect(() => {
    if (!entered) return;

    let disposed = false;
    let alignTimer: number | undefined;

    setStarting(true);
    setPlayerState(null);
    setError(null);
    setSeekPreview(null);
    setSeekTarget(null);
    setVolumePreview(null);
    setSubtitleMenuOpen(false);
    setAudioMenuOpen(false);

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
        if (!disposed) {
          setStarting(false);
          // 启动后布局仍可能有一两像素的抖动，再对齐一次原生视频窗口。
          alignTimer = window.setTimeout(syncVideoRect, 120);
        }
      } catch (err) {
        if (!disposed) {
          setError(errorMessage(err));
          setStarting(false);
        }
      }
    }, 0);

    return () => {
      disposed = true;
      window.clearTimeout(timer);
      if (alignTimer) window.clearTimeout(alignTimer);
    };
  }, [episode.id, entered, syncVideoRect]);

  useEffect(() => {
    subtitleDelayRef.current = 0;
    setSubtitleDelayMs(0);
  }, [episode.id]);

  // 订阅播放状态。
  useEffect(() => {
    let disposed = false;
    let unlistenState: UnlistenFn | undefined;
    let unlistenClosed: UnlistenFn | undefined;
    let unlistenError: UnlistenFn | undefined;

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

    onPlayerError((payload) => {
      if (disposed || payload.episodeId !== episode.id) return;
      setError(payload.message);
      setStarting(false);
    }).then((fn) => {
      if (disposed) fn();
      else unlistenError = fn;
    });

    return () => {
      disposed = true;
      unlistenState?.();
      unlistenClosed?.();
      unlistenError?.();
    };
  }, [episode.id]);

  // 鼠标活动：后端轮询系统光标，视频画面上方的移动也能收到。
  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;

    onPlayerMouseActivity(() => {
      if (!disposed) {
        setActivityTick((tick) => tick + 1);
        // 只有鼠标移动才主动唤出控制栏；空格播放/暂停不应唤出。
        setControlsVisible(true);
      }
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // 原生视频窗口上的单击由后端轮询鼠标发出；和下方黑边一样只收起控制栏。
  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;

    onPlayerClick(() => {
      if (!disposed) hideControls();
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [hideControls]);

  // 全屏时鼠标静止 2.5 秒收起控制栏；窗口模式只在点击画面时收起。
  // 控制栏已经隐藏时不再因为暂停/播放等状态变化重新出现。
  useEffect(() => {
    if (!controlsVisible) return;
    if (!fullscreen) return;

    const timer = window.setTimeout(() => {
      if (!subtitleMenuOpen && !audioMenuOpen) {
        setSubtitleMenuOpen(false);
        setAudioMenuOpen(false);
        setControlsVisible(false);
      }
    }, 2500);

    return () => window.clearTimeout(timer);
  }, [
    fullscreen,
    activityTick,
    subtitleMenuOpen,
    audioMenuOpen,
    controlsVisible,
  ]);

  // 全屏收起控制栏时一并隐藏鼠标指针；重新出现或退出时恢复。
  useEffect(() => {
    const hideCursor = fullscreen && !controlsVisible;
    void setPlayerCursorVisible(!hideCursor).catch(() => {});

    return () => {
      if (hideCursor) {
        void setPlayerCursorVisible(true).catch(() => {});
      }
    };
  }, [fullscreen, controlsVisible]);

  // 窗口尺寸变化时同步原生视频窗口的位置。
  useEffect(() => {
    const element = videoRef.current;
    if (!element) return;

    let frame: number | undefined;
    const observer = new ResizeObserver(() => {
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = undefined;
        // JS 驱动的原生窗口动画自己负责同步，避免这里用终态矩形覆盖它。
        if (flipFrameRef.current) return;

        syncVideoRect();
        // 没有 FLIP 动画时更新基准矩形；动画中由 FLIP 自己维护。
        if (!flipAnimRef.current) {
          videoRectRef.current = element.getBoundingClientRect();
        }
      });
    });

    observer.observe(element);
    return () => {
      observer.disconnect();
      if (frame) cancelAnimationFrame(frame);
    };
  }, [syncVideoRect]);

  // 窗口（尤其全屏切换）改变视口尺寸时，元素可能只是位置变化、尺寸不变，
  // ResizeObserver 不会触发，所以再监听一次 window.resize。
  useEffect(() => {
    let frame: number | undefined;
    const handler = () => {
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = undefined;
        if (flipFrameRef.current) return;

        syncVideoRect();
        if (!flipAnimRef.current && videoRef.current) {
          videoRectRef.current = videoRef.current.getBoundingClientRect();
        }
      });
    };

    window.addEventListener("resize", handler);
    return () => {
      window.removeEventListener("resize", handler);
      if (frame) cancelAnimationFrame(frame);
    };
  }, [syncVideoRect]);

  // 组件卸载时停止播放。
  useEffect(() => {
    return () => {
      void stopPlayer();
    };
  }, []);

  const handleClose = () => {
    if (closing) return;
    setClosing(true);
    void stopPlayer();

    setOpen(false);
    window.setTimeout(onClose, 280);
  };

  const toggleFullscreen = () => {
    const next = !fullscreen;
    // 退出全屏时控制栏必须可见。和 fullscreen 放在同一批更新里，
    // 避免自动隐藏逻辑之后再次更新状态，触发第二次 FLIP 并打断退出动画。
    if (!next) setControlsVisible(true);

    setFullscreen(next);
  };

  const paused = playerState?.paused ?? false;
  const ended = playerState?.ended ?? false;
  const durationMs = playerState?.durationMs || episode.durationMs || 0;
  const positionMs =
    seekPreview ?? seekTarget ?? playerState?.positionMs ?? 0;
  const volume = volumePreview ?? playerState?.volume ?? 100;
  const muted = playerState?.muted ?? false;

  const subtitleTracks = playerState?.subtitles ?? [];
  const subtitleId = playerState?.subtitleId ?? null;
  const currentSubtitle =
    subtitleTracks.find((track) => track.id === subtitleId) ?? null;
  const subtitleOn =
    subtitleId != null && (playerState?.subtitlesVisible ?? true);
  const subtitleLabel =
    currentSubtitle?.title ||
    currentSubtitle?.lang ||
    (currentSubtitle ? `字幕 ${currentSubtitle.id}` : null);
  const hasSubtitles = subtitleTracks.length > 0;
  const subtitleDelayLabel = `${
    subtitleDelayMs > 0 ? "+" : ""
  }${(subtitleDelayMs / 1000).toFixed(1)}s`;

  const audioTracks = playerState?.audioTracks ?? [];
  const audioId = playerState?.audioId ?? null;
  const currentAudio =
    audioTracks.find((track) => track.id === audioId) ?? null;
  const audioLabel = currentAudio ? audioTrackLabel(currentAudio) : null;
  const hasAudio = audioTracks.length > 0;

  const currentIndex = episodes.findIndex((item) => item.id === episode.id);
  const previous = currentIndex > 0 ? episodes[currentIndex - 1] : null;
  const next =
    currentIndex >= 0 && currentIndex < episodes.length - 1
      ? episodes[currentIndex + 1]
      : null;

  const seekTo = (position: number) => {
    const target = Math.max(0, Math.round(position));
    setSeekTarget(target);
    void playerSeek(target);
  };

  const togglePlay = () => {
    if (!playerState) return;

    if (ended) {
      seekTo(0);
      void playerSetPaused(false);
      return;
    }

    void playerSetPaused(!playerState.paused);
  };

  const commitSeek = () => {
    if (seekPreview == null) return;
    const target = seekPreview;
    setSeekPreview(null);
    seekTo(target);
  };

  const closeMenus = () => {
    setSubtitleMenuOpen(false);
    setAudioMenuOpen(false);
  };

  const selectSubtitle = (id: number | null) => {
    // 切换轨道后保留字幕面板，方便继续调时间或换另一条字幕。
    setAudioMenuOpen(false);
    void playerSetSubtitle(id);
  };

  const selectAudio = (id: number) => {
    closeMenus();
    void playerSetAudio(id);
  };

  const adjustSubtitleDelay = (deltaMs: number) => {
    const next = Math.max(
      -60_000,
      Math.min(60_000, subtitleDelayRef.current + deltaMs),
    );
    subtitleDelayRef.current = next;
    setSubtitleDelayMs(next);
    void playerSetSubDelay(next / 1000);
  };

  const resetSubtitleDelay = () => {
    subtitleDelayRef.current = 0;
    setSubtitleDelayMs(0);
    void playerSetSubDelay(0);
  };

  // 字幕选项直接展开在控制栏内部，和进度条一样是普通网页控件，
  // 不会覆盖原生视频窗口，也不需要任何截图。
  const toggleSubtitleMenu = () => {
    if (!playerState) return;
    setAudioMenuOpen(false);
    setSubtitleMenuOpen((open) => !open);
  };

  // 音轨切换沿用字幕的交互：按钮在控制栏内，展开后显示轨道 chip。
  const toggleAudioMenu = () => {
    if (!playerState || !hasAudio) return;
    setSubtitleMenuOpen(false);
    setAudioMenuOpen((open) => !open);
  };

  // 播放器回报的位置接近目标后，进度条才恢复跟随实时进度。
  useEffect(() => {
    if (seekTarget == null) return;
    if (
      playerState &&
      Math.abs(playerState.positionMs - seekTarget) < 1000
    ) {
      setSeekTarget(null);
    }
  }, [playerState, seekTarget]);

  // 兜底：即使没有收到接近目标的位置，也不要一直停留在目标值。
  useEffect(() => {
    if (seekTarget == null) return;
    const timer = window.setTimeout(() => setSeekTarget(null), 1200);
    return () => window.clearTimeout(timer);
  }, [seekTarget]);

  // 抽屉滑入结束后记录视频的基准矩形（滑入使用 transform，不会触发 ResizeObserver）。
  useLayoutEffect(() => {
    if (!entered) return;
    const video = videoRef.current;
    if (video && !flipAnimRef.current) {
      videoRectRef.current = video.getBoundingClientRect();
    }
  }, [entered]);

  // 播放器内所有改变视频区域布局的操作（全屏、控制栏、字幕面板）都用 FLIP：
  // 记录变化前后的矩形，用一次 translate + scale 过渡，原生窗口逐帧跟随。
  useLayoutEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    const next = video.getBoundingClientRect();
    const prev = videoRectRef.current;
    videoRectRef.current = next;

    if (!prev) return;

    const same =
      Math.abs(prev.left - next.left) < 0.5 &&
      Math.abs(prev.top - next.top) < 0.5 &&
      Math.abs(prev.width - next.width) < 0.5 &&
      Math.abs(prev.height - next.height) < 0.5;
    if (same) return;

    const fullscreenChanged = lastFullscreenRef.current !== fullscreen;
    lastFullscreenRef.current = fullscreen;

    if (fullscreenChanged) {
      // 全屏切换：范围大，用 CSS transform + expo-out，逐帧采样同步。
      flipAnimRef.current?.cancel();

      // 控制栏每帧直接跟随视频当前矩形：宽度、左位置、上位置都从
      // 视频矩形算出来，保证控制栏中点和画面中点始终一致，并且和原生
      // 窗口使用同一帧的采样值。
      const controls = controlsRef.current;
      const controlsTo = controls?.getBoundingClientRect() ?? null;

      // 在第一次绘制前就把控制栏放到视频的起始矩形上，
      // 否则第一帧会先显示终态布局，下一帧才跳回起点，看起来像慢一拍。
      if (controls && controlsTo) {
        // 进入全屏沿用视频区域矩形；退出全屏跟随画面适配框。
        const prevBox = fullscreen
          ? { x: prev.left, width: prev.width }
          : fitToAspect(
              {
                x: prev.left,
                y: prev.top,
                width: prev.width,
                height: prev.height,
              },
              videoAspect,
            );
        controls.style.width = `${prevBox.width}px`;
        controls.style.transform = `translate(${
          prevBox.x - controlsTo.left
        }px, ${prev.bottom - controlsTo.top}px)`;
      }

      const dx = prev.left - next.left;
      const dy = prev.top - next.top;
      const scaleX = prev.width / next.width;
      const scaleY = prev.height / next.height;

      video.style.transformOrigin = "top left";
      const animation = video.animate(
        [
          {
            transform: `translate(${dx}px, ${dy}px) scale(${scaleX}, ${scaleY})`,
          },
          { transform: "translate(0, 0) scale(1, 1)" },
        ],
        { duration: 300, easing: "cubic-bezier(0.16, 1, 0.3, 1)" },
      );
      flipAnimRef.current = animation;

      let frameId = 0;

      // 动画结束的同一时刻恢复控制栏最终布局并同步原生窗口，
      // 不再等下一帧或额外保留内联样式。
      const finalize = () => {
        if (flipAnimRef.current === animation) {
          flipAnimRef.current = null;
          video.style.transformOrigin = "";
        }
        if (controls) {
          controls.style.width = "";
          controls.style.transform = "";
        }
        syncVideoRect();
        // 原生窗口可能还有一帧的处理延迟，再补一次终态同步。
        window.setTimeout(syncVideoRect, 80);
      };

      animation.onfinish = finalize;
      animation.oncancel = finalize;

      const tick = () => {
        if (animation.playState === "finished") return;

        syncVideoRect();

        if (controls && controlsTo) {
          const rect = video.getBoundingClientRect();

          let progress = 1;
          const totalWidth = next.width - prev.width;
          const totalLeft = next.left - prev.left;
          const totalHeight = next.height - prev.height;
          const totalTop = next.top - prev.top;

          if (Math.abs(totalWidth) > 1) {
            progress = (rect.width - prev.width) / totalWidth;
          } else if (Math.abs(totalLeft) > 1) {
            progress = (rect.left - prev.left) / totalLeft;
          } else if (Math.abs(totalHeight) > 1) {
            progress = (rect.height - prev.height) / totalHeight;
          } else if (Math.abs(totalTop) > 1) {
            progress = (rect.top - prev.top) / totalTop;
          }

          progress = Math.min(1, Math.max(0, progress));

          // 进入全屏沿用视频区域矩形（已调好的效果）；
          // 退出全屏跟随画面适配框，和原生视频窗口的收缩速度一致。
          const area = {
            x: prev.left + (next.left - prev.left) * progress,
            y: prev.top + (next.top - prev.top) * progress,
            width: prev.width + (next.width - prev.width) * progress,
            height: prev.height + (next.height - prev.height) * progress,
          };
          const box = fullscreen
            ? { x: area.x, width: area.width }
            : fitToAspect(area, videoAspect);

          controls.style.width = `${box.width}px`;
          controls.style.transform = `translate(${
            box.x - controlsTo.left
          }px, ${area.y + area.height - controlsTo.top}px)`;
        }

        frameId = requestAnimationFrame(tick);
      };

      frameId = requestAnimationFrame(tick);

      return () => {
        animation.cancel();
        finalize();
        cancelAnimationFrame(frameId);
      };
    }

    // 控制栏/字幕面板：变化范围小，原生窗口采样 CSS 合成动画会落后几帧。
    // 这里由 JS 用同一个缓动直接计算每帧矩形并同步原生窗口，避免维度和
    // 终点不同步，结束时再强制对齐一次。
    flipAnimRef.current?.cancel();
    video.style.transformOrigin = "";
    if (flipFrameRef.current) cancelAnimationFrame(flipFrameRef.current);

    const duration = 250;
    const started = performance.now();

    const step = (now: number) => {
      const t = Math.min(1, (now - started) / duration);
      const progress = 1 - Math.pow(1 - t, 3);

      const area = {
        x: prev.left + (next.left - prev.left) * progress,
        y: prev.top + (next.top - prev.top) * progress,
        width: prev.width + (next.width - prev.width) * progress,
        height: prev.height + (next.height - prev.height) * progress,
      };
      void resizePlayer(fitToAspect(area, videoAspect)).catch(() => {});

      if (t < 1) {
        flipFrameRef.current = requestAnimationFrame(step);
      } else {
        flipFrameRef.current = null;
        syncVideoRect();
        // 最后再补一次，确保宽度和高度都落到终态。
        window.setTimeout(syncVideoRect, 80);
      }
    };

    flipFrameRef.current = requestAnimationFrame(step);

    return () => {
      if (flipFrameRef.current) {
        cancelAnimationFrame(flipFrameRef.current);
        flipFrameRef.current = null;
      }
    };
  }, [fullscreen, controlsVisible, subtitleMenuOpen, audioMenuOpen]);

  // 键盘快捷键
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      const tag = (event.target as HTMLElement | null)?.tagName;
      if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") return;

      if (event.key === "Escape" && (subtitleMenuOpen || audioMenuOpen)) {
        event.preventDefault();
        closeMenus();
      } else if (event.key === "Escape") {
        if (fullscreen) {
          void toggleFullscreen();
        } else {
          handleClose();
        }
      } else if (event.key === " ") {
        event.preventDefault();
        togglePlay();
        // 空格播放/暂停后立即收起控制栏；鼠标移动会把控制栏重新唤出。
        if (fullscreen) {
          setSubtitleMenuOpen(false);
          setAudioMenuOpen(false);
          setControlsVisible(false);
        }
      } else if (event.key === "ArrowLeft") {
        event.preventDefault();
        seekTo(Math.max(0, positionMs - 5000));
      } else if (event.key === "ArrowRight") {
        event.preventDefault();
        seekTo(positionMs + 5000);
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
        ref={drawerRef}
        onTransitionEnd={(event) => {
          // Tailwind v4 的 translate-x-* 走的是独立的 translate 属性。
          if (
            event.target === drawerRef.current &&
            (event.propertyName === "translate" ||
              event.propertyName === "transform")
          ) {
            setEntered(true);
          }
        }}
        className={`fixed right-0 top-0 z-50 flex h-full max-w-full flex-col bg-neutral-950 shadow-2xl transition-[translate,transform] duration-300 ease-out ${
          fullscreen
            ? "w-screen border-l-0"
            : "w-[min(880px,76vw)] border-l border-neutral-800"
        } ${open && !closing ? "translate-x-0" : "translate-x-full"}`}
      >
        <div
          className={`grid shrink-0 ${
            fullscreen ? "grid-rows-[0fr]" : "grid-rows-[1fr]"
          }`}
        >
          <div
            className={`overflow-hidden ${
              fullscreen ? "pointer-events-none" : ""
            }`}
          >
            <header className="flex items-center justify-between gap-4 border-b border-neutral-800 px-5 py-3">
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
          </div>
        </div>

        <div
          ref={videoRef}
          onClick={hideControls}
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

        <div
          ref={controlsRef}
          className={`grid shrink-0 ${
            !controlsVisible ? "grid-rows-[0fr]" : "grid-rows-[1fr]"
          }`}
        >
          <div
            className={`overflow-hidden ${
              !controlsVisible ? "pointer-events-none" : ""
            }`}
          >
            <div
              className={`border-b border-neutral-800 bg-neutral-900/60 px-4 py-3 ${
                !controlsVisible ? "opacity-0" : "opacity-100"
              }`}
            >
              <div
                className={`grid ${
                  subtitleMenuOpen || audioMenuOpen
                    ? "grid-rows-[1fr]"
                    : "grid-rows-[0fr]"
                }`}
              >
                <div className="overflow-hidden">
                  {subtitleMenuOpen && (
                    <div className="mb-3 flex flex-wrap items-center gap-2 border-b border-neutral-800 pb-3">
                      <span className="mr-1 text-[10px] uppercase tracking-widest text-neutral-500">
                        字幕
                      </span>

                      {subtitleTracks.length === 0 ? (
                        <span className="text-xs text-neutral-500">
                          该视频没有字幕
                        </span>
                      ) : (
                        <>
                          <TrackChip
                            label="关闭字幕"
                            selected={!subtitleOn}
                            onClick={() => selectSubtitle(null)}
                          />

                          {subtitleTracks.map((track) => (
                            <TrackChip
                              key={track.id}
                              label={
                                track.title ||
                                track.lang ||
                                `字幕 ${track.id}`
                              }
                              selected={subtitleOn && track.id === subtitleId}
                              onClick={() => selectSubtitle(track.id)}
                            />
                          ))}
                        </>
                      )}

                      <button
                        type="button"
                        onClick={closeMenus}
                        className="ml-auto text-[11px] text-neutral-400 transition-colors hover:text-neutral-200"
                      >
                        收起
                      </button>

                      {hasSubtitles && (
                        <div className="mt-2 flex w-full flex-wrap items-center gap-2 border-t border-neutral-800 pt-2">
                          <span className="text-[10px] uppercase tracking-widest text-neutral-500">
                            字幕时间
                          </span>
                          <button
                            type="button"
                            title="字幕提前 0.1 秒"
                            aria-label="字幕提前 0.1 秒"
                            onClick={() => adjustSubtitleDelay(-100)}
                            className="flex w-7 items-center justify-center rounded bg-neutral-800 px-2 py-1 text-sm leading-none text-neutral-300 transition-colors hover:bg-neutral-700"
                          >
                            -
                          </button>
                          <span className="min-w-12 text-center text-xs tabular-nums text-neutral-200">
                            {subtitleDelayLabel}
                          </span>
                          <button
                            type="button"
                            title="字幕延后 0.1 秒"
                            aria-label="字幕延后 0.1 秒"
                            onClick={() => adjustSubtitleDelay(100)}
                            className="flex w-7 items-center justify-center rounded bg-neutral-800 px-2 py-1 text-sm leading-none text-neutral-300 transition-colors hover:bg-neutral-700"
                          >
                            +
                          </button>
                          <button
                            type="button"
                            onClick={resetSubtitleDelay}
                            className="text-[11px] text-neutral-400 transition-colors hover:text-neutral-200"
                          >
                            复位
                          </button>
                        </div>
                      )}
                    </div>
                  )}

                  {audioMenuOpen && (
                    <div className="mb-3 flex flex-wrap items-center gap-2 border-b border-neutral-800 pb-3">
                      <span className="mr-1 text-[10px] uppercase tracking-widest text-neutral-500">
                        音轨
                      </span>

                      {audioTracks.length === 0 ? (
                        <span className="text-xs text-neutral-500">
                          该视频没有可切换的音轨
                        </span>
                      ) : (
                        audioTracks.map((track) => (
                          <TrackChip
                            key={track.id}
                            label={audioTrackLabel(track)}
                            selected={track.id === audioId}
                            onClick={() => selectAudio(track.id)}
                          />
                        ))
                      )}

                      <button
                        type="button"
                        onClick={closeMenus}
                        className="ml-auto text-[11px] text-neutral-400 transition-colors hover:text-neutral-200"
                      >
                        收起
                      </button>
                    </div>
                  )}
                </div>
              </div>

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
                    onClick={toggleAudioMenu}
                    disabled={!playerState || !hasAudio}
                    className={`${iconButton} ${
                      audioMenuOpen
                        ? "bg-neutral-800 text-white"
                        : ""
                    }`}
                    title={
                      hasAudio
                        ? `音轨（当前：${audioLabel ?? "未知"}）`
                        : "音轨"
                    }
                  >
                    <AudioLines size={16} />
                  </button>

                  <button
                    type="button"
                    onClick={toggleSubtitleMenu}
                    disabled={!playerState}
                    className={`${iconButton} ${
                      subtitleMenuOpen
                        ? "bg-neutral-800 text-white"
                        : subtitleOn
                          ? "text-indigo-400 hover:text-indigo-300"
                          : ""
                    }`}
                    title={
                      hasSubtitles
                        ? `字幕（当前：${subtitleOn ? subtitleLabel ?? "开启" : "关闭"}）`
                        : "字幕"
                    }
                  >
                    {subtitleOn ? <Captions size={16} /> : <CaptionsOff size={16} />}
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
