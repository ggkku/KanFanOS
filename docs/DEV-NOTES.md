# 开发备忘与踩坑记录

记录实现过程中确认过的约定、踩过的坑和不再采用的方案，避免下次重复试错。

## 环境

- 需要 Node.js ≥ 20、pnpm、Rust stable（MSVC）、FFmpeg / FFprobe、MPV。
- 国内网络下 `rustup` 官方源可能只有几十 KB/s，换 `rsproxy.cn` 镜像：
  ```bash
  export RUSTUP_DIST_SERVER=https://rsproxy.cn
  export RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup
  rustup default stable
  ```
- 开发时 FFmpeg / MPV 如果不在 PATH 中，需要在启动 `pnpm tauri dev` 的终端里补上，否则缩略图、探测和播放都会失败。

## React / 前端

### StrictMode 下 effect 会执行两次

`main.tsx` 开着 `React.StrictMode`。**不要用「在 effect 里改 ref、下一次 effect 读取」来判断第一次挂载**：第一次 effect 已经把 ref 改掉，紧接着的第二次 effect 会误以为「不是第一次」，导致本该等抽屉动画结束再取坐标的逻辑变成 0 延迟执行，原生视频窗口会被定位到动画中间的坐标上。

正确做法：用 `transitionend` + 超时兜底这类和渲染次数无关的信号。

### Tailwind v4 的 translate

`translate-x-*` 在 Tailwind v4 里写的是独立的 **`translate`** CSS 属性，不是 `transform`。

- `transition-transform` 展开后是 `transform, translate, scale, rotate`，所以抽屉滑动正常
- 手写 `transition-[width,transform]` **不会**过渡 `translate`，抽屉动画会直接消失

需要同时过渡宽度和滑动时用 `transition-[width,translate,transform]`（或至少把 `translate` 带上）。

### 页面切换与返回动画

- 右侧页面容器按 `view/category` 生成 key，导航时旧页面保留在绝对定位的退场层，新页面做进入动画。
- 方向只看左侧导航的上下顺序：目标在下方 → 新页面从上方进入，旧页面向下退出。
- 番剧详情返回时使用进入动画的倒放；`detailReturn` 状态要保留到下一次真正的导航或重新打开番剧，否则它变回 `null` 会让进入动画的 class 改变，从而**重播一次页面切换动画**。
- 番剧详情的四种进入动画（上浮 / 缩放 / 右侧滑入 / 封面飞入）由 `seriesTransition` 设置控制；返回时反向播放。封面飞入记录卡片封面的屏幕矩形，详情页封面带 `data-series-cover` 作为目标。

### 其他

- 原生 `<select>` 的下拉列表在 Windows 上很难统一风格，统一换成 `components/common/SelectMenu.tsx` 自定义下拉。
- 最近观看页面只有 6 条是**查询上限**，`watch_history` 不裁剪；不要把它和续播数据清理混为一谈。

## 原生 MPV / Win32 子窗口

### 层级

MPV 通过 `--wid` 渲染到一个 Win32 子窗口，**永远在 WebView 之上**。HTML 元素无法覆盖视频画面：字幕菜单、提示浮层等都不能放在视频区域上。字幕选项条因此放在底部控制栏内部，展开时由视频区域让出高度。

尝试过但放弃的方案：

- **冻结帧 + 黑屏淡入淡出**：需要 mpv 截图。mpv 在处理精确 seek / 着色器编译时截图可能延迟约 2 秒，前端等待窗口（400ms）几乎必然超时，最终仍然黑屏；改为 data URL 后显示问题解决了，但截图延迟无法接受，整套方案撤掉。
- **分层窗口 alpha（WS_EX_LAYERED）**：没有继续验证 D3D11 flip-model 与分层子窗口的兼容性，最终放弃。

不要再用 mpv 截图做交互式 UI 的临时背景，除非能保证截图在几十毫秒内完成。

### 视频窗口尺寸必须按视频宽高比适配

`syncVideoRect` 不再把 mpv 窗口设成整个视频容器，而是调用 `fitToAspect`，按 `episode.width / episode.height` 在容器内居中适配：

- 容器比视频更宽时，mpv 窗口只占中间的 16:9 区域，左右黑边由 HTML 容器提供
- 这样 mpv 内部不再做 letterbox，任何窗口缩放都是宽高同比例变化
- 如果窗口按容器尺寸设置，缩小时会先变高度、画面宽度由 mpv 后续重绘才跟上，主观上就是「控制栏/画面速度不一致」

### 播放器全屏与系统窗口解耦

播放器的「全屏」只改变抽屉布局（铺满程序窗口），**不要**调用系统窗口的 `set_fullscreen`。程序窗口是否全屏由设置里的「窗口模式」控制，两者互不影响。

旧实现把播放器全屏接到系统窗口上，导致在「窗口模式」里点播放器全屏会把整个程序拉成全屏，退出时又会把程序窗口恢复成小窗；已彻底拆开，`set_player_fullscreen` 命令也已删除。

退出播放器全屏时，要把 `setControlsVisible(true)` 和 `setFullscreen(false)` 放在同一批状态更新里，否则自动隐藏逻辑会在之后再次更新状态，触发第二次 layout effect，把退出动画取消或覆盖。

### FLIP 动画约定

- 全屏切换：记录前后矩形，对视频元素做一次 `translate + scale`（300ms，`cubic-bezier(0.16,1,0.3,1)`），原生窗口每帧采样变换后的矩形同步。
- 控制栏/字幕面板：变化范围小，原生窗口采样 CSS 合成动画会落后几帧，改为 JS 逐帧插值（250ms，ease-out cubic），直接算矩形调用 `resize_player`。
- 起始对齐：控制栏的临时宽度/位移要在 layout effect 里、**第一次绘制前**设置好，否则第一帧会先显示终态布局，下一帧才跳回起点。
- 收尾对齐：在视频动画的 `onfinish` 里清掉控制栏的内联宽度/transform，并同步一次原生窗口（80ms 后再补一次）。不要等 rAF 循环下一帧再清理。
- 控制栏宽度跟的是**画面适配框**（退出方向）或视频区域（进入方向，已调好的版本），不要随意改方向：跟错矩形时宽度和边缘的收缩速度会和画面不一致。
- 不要加「控制栏提前到达终点」的进度补偿。慢放时能看到控制栏先停住、画面还在动，反而更明显。现在没有 lead 补偿。

### 进度条回跳

`seekPreview` 在松手后不能立即清空，否则进度条会回落到播放器上一帧回报的旧位置，等 mpv seek 完才跳到目标。用 `seekTarget` 保持目标位置，等 `playerState.positionMs` 接近目标（1 秒内）再交回实时跟随，另加 1.2 秒兜底。

## Tauri 2 / 窗口

- 前端的窗口 API（`setFullscreen` / `setDecorations` / `setAlwaysOnTop` / `setSkipTaskbar`）默认可能被 capability 拒绝，会报权限错误。窗口模式改动放在 Rust 侧 `apply_window_mode` 执行，不依赖前端权限。
- 启动流程：`tauri.conf.json` 里窗口 `visible: false` → Rust 初始化时应用窗口模式 → `window.show()`。配合 `backgroundColor: "#0a0a0a"` 和 `index.html` 里 `html/body/#root` 的内联深色背景，避免启动白闪。
- **真正的独占全屏无法实现**：WebView2 由 DWM 合成，没有 DXGI 交换链接管显示器的能力。只保留「窗口 / 全屏」两种模式；不要提供假的「独占全屏」选项。
- `quit_app` 命令内部调用 `app.exit(0)`。退出时 WebView2 可能打印 `Failed to unregister class Chrome_WidgetWin_0`，这是关闭阶段的正常日志噪音。

## 数据与扫描

- `watch_history` 不裁剪；最近观看的 `LIMIT 6` 只影响展示。
- `剧场版` 特判在 `catalog.rs`：`MOVIE_CATEGORY`、`is_movie_category`、`schedule_sort_key` 返回 `i64::MAX` 保证排在侧栏最上方。扫描器把顶层 `剧场版/` 当分类，子目录按影片处理。
- settings.json 新增字段都有 serde 默认值，读取和保存时都会规范化（页面动画、选集动画、窗口模式），旧配置不会产生非法值。

## 图标

- 源图放在项目根目录 `icon.png`（正方形、建议 1024×1024 以上、透明背景）。
- 运行 `pnpm icon`（即 `tauri icon icon.png`）重新生成 `src-tauri/icons/`；侧栏 logo 直接引用 `src-tauri/icons/128x128@2x.png`，换图标后无需再手动拷贝。
- `tauri icon` 会顺带生成 `icons/android/` 和 `icons/ios/`，本项目只面向 Windows，这两个目录已在 `.gitignore` 中忽略。
- 图标生成后需要重新编译（Rust 的 build script 会重新嵌入 `.ico`）；Windows 任务栏有图标缓存，必要时重启资源管理器。

## 调试

- 运行时检查可以给 Tauri 设置 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`，然后用 CDP 读取 DOM 矩形/动画。正常的用户测试不需要开。
- 原生窗口的实际矩形可以用 Win32 `EnumChildWindows` 找 `KanFanOSVideoSurface` 再 `GetWindowRect` 采样。
- 常用检查：`pnpm build`、`cd src-tauri && cargo test`、`cargo clippy --all-targets`。
