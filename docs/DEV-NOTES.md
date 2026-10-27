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
- 外部工具按「主程序同目录的 sidecar → PATH」顺序查找（`src-tauri/src/tools.rs`）。打包后的程序自带 mpv / FFmpeg；开发时如果 `src-tauri/binaries/` 里没有 sidecar，就依赖 PATH，需要在启动 `pnpm tauri dev` 的终端里补上，否则缩略图、探测和播放都会失败。
- `src-tauri/binaries/` 不纳入版本管理，由 `pnpm sidecars`（`scripts/fetch-sidecars.mjs`）从本机安装目录复制。
- 安装后如果**能播放但没有缩略图、也没有播放进度百分比**，先看数据目录下的 `logs\media.log`（默认 `%LOCALAPPDATA%\KanFanOS\logs\media.log`；装到 D 盘时是 `D:\KanFanOS\data\logs\media.log`）。这通常不是播放器问题，而是 FFprobe 不可用：探测失败时 `duration_ms` 一直为空，缩略图那一步也不会执行；进度条在 `durationMs <= 0` 时直接隐藏，所以两者会一起消失。
- 旧安装包没有 `externalBin`，只依赖 PATH。在目标机器补上工具后，数据库里已经 `failed` 的任务不会自己重试（`claim_media_jobs` 只领取 `pending`）。现在启动时会把「启动 ffprobe/ffmpeg 失败」的任务重新排队，设置页的「清理缩略图缓存」也会重置所有未失效视频并立即开始重建。

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
- 番剧数量可能有几百上千部，季链接**不能**一次性加载成下拉：用 `SeriesPickerDialog` + 后端 `search_series_options` 按关键词查询，单次最多 200 条，输入去抖 180ms。选择器里的「无」不依赖搜索结果，加载中也要能点。
- 选集页的配置弹窗必须通过 `createPortal` 挂到 `document.body`：详情页外层带 transform 进入动画，`position: fixed` 子元素会以动画容器为包含块，不 portal 的话遮罩盖不住整个窗口。选择器本身也是 portal，`z-[60]` 盖在配置弹窗（`z-50`）上。
- 个人评分和奖项以 `kanfanos.toml` 为准（`rating`，-1 表示未评分；`awards` 字符串数组）。数据库 `series.rating` 只是索引：扫描时由 `scanner` 从配置同步，打分命令写配置后也会立即更新一次。注意写配置会触发文件监听重扫，这是选 toml 存储的代价（监听有去抖）。
- 评语是前端常量 `RATING_LABELS`，每档四个字，只影响显示。`SeriesConfig::default()` 的 rating 必须是 -1，不能依赖 derived Default 的 0（0 是合法评分「粪中之粪」）。
- 奖项输入框在选集页右上角；配置弹窗里的「添加奖项」通过 `onAddAward` 关闭弹窗并递增 `openSignal`，由 `SeriesAwards` 展开并聚焦输入框。选集页不提供第二个添加入口，奖项行也不带 ×；删除统一在配置弹窗的奖项列表里，由 `onRemoveAward` 写回 toml 并同步父组件状态。奖项不做数据库存储，只有 `get_series_config` 读取。
- 评分 / 奖项必须放在详情页 header 的右侧列（`flex-1` 信息列 + `w-56 shrink-0` 右列），**不要**单独加一行放在 header 上方：那会把封面和选集列表整体往下推。奖项每条一行，列表 `max-h-[92px]` + 内部滚动，保证右列总高（≈158px）不超过封面高度（160px），header 不会被撑高；输入框展开时列表临时缩到 `max-h-14`。
- 选番卡片的 hover 发光用渐变色的 `box-shadow` 三层叠加（4/10/24px），颜色由 `ratingGlowColor` 决定；卡片加 `hover:z-10` 防止被相邻卡片盖住。
- 横向裁切统一放在外层 MAIN（`overflow-x: hidden`），页面内容不再使用 `clip-x`。这样最左侧卡片的发光能一直画进 32px 的页面内边距，不需要靠 hover 临时切换 overflow（切换 overflow 会导致布局/绘制状态变化）。辉光总范围约 28px，仍在 MAIN 的裁切边界内。
- 「配置」按钮用 `absolute right-0 top-0` 放在返回行右侧，不参与流式布局，避免按钮高度把 header 往下推。
- 旧的 `kanfanos.toml` 通过设置页「检查并修复」批量升级：核心逻辑在 `series_config::repair_file`。只有「名称等于文件夹名」或「缺少标准字段」时才重写；解析失败的文件会重建。原文件备份为 `kanfanos.toml.bak`（已存在则不覆盖）。修复后由命令用 `resolve` + `upsert_series` 同步数据库，名称/别名/评分立即生效。
- 修复时要跳过“散落视频”生成的伪番剧：它们的 path 是库根目录或档期目录本身，没有独立文件夹，不应生成 `kanfanos.toml`。`is_loose_series_dir` 通过相对根目录的层级和目录名（能否解析为档期）判断。
- 上一季 / 下一季的目标以番剧文件夹绝对路径存进各自的 `kanfanos.toml`（`previous` / `next`），读取时再按路径查数据库。这样扫描不会覆盖用户设置，目标番剧暂时失效时按钮自然消失。
- 季链接是双向不变量：A.previous=B 时同步 B.next=A，A.next=C 时同步 C.previous=A。改链接时先解除旧目标中指向自己的反向链接；覆盖冲突链接前，也要解除被替换的旧配对（否则会留下 C.previous 仍指向 B 的残留）。只有**实际改动**的链接才做检查和同步，重复保存不会反复弹窗。
- 反向链接冲突由 `check_season_links` 在保存前查询，前端弹 `ConfirmDialog`；选「保留原设置」仍保存本番剧的链接，只是不覆盖目标。`update_series_config` 的 `overwrite_inverse` 决定是否覆盖。
- 最近观看页面只有 6 条是**查询上限**，`watch_history` 不裁剪；不要把它和续播数据清理混为一谈。
- 番剧卡片封面只使用 `coverPath`（`kanfanos.toml` 或文件夹内自动发现的图片）；没有封面时显示番剧名首字占位，**不能用 `cover_key`（第一集视频缩略图）顶替**。视频缩略图只用于剧集列表和播放器。封面飞入动画对首字占位同样有效，别把 flip 限制成必须有图片 URL。

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

### 音轨切换

- `track-list` 事件里同时解析 `sub`/`subtitle` 和 `audio` 轨；切音轨时 `track-list` 本身不会变，必须另外 observe `aid` 才能拿到当前音轨。
- 音轨面板与字幕面板共用一套交互：控制栏按钮 + 展开 chip 条，同一时间只开一个面板。
- 全屏自动隐藏、控制栏 FLIP 的依赖数组都要同时包含 `subtitleMenuOpen` 和 `audioMenuOpen`，漏掉会导致音轨条展开时控制栏不跟着动画。

### 字幕编码

`--sub-codepage` 会透传给 libavformat 的 `sub_charenc`，**不支持** `utf8:gb18030` 这种回退链，实际会报 `Error opening iconv with codepage 'utf8:gb18030'`，然后仍按错误编码读字幕。

现在改由 Rust 侧 `playback/subtitles.rs` 处理：扫描视频同目录下匹配的 SRT/VTT/ASS/SSA，用 Windows `MultiByteToWideChar` 按 UTF-8 → UTF-16 BOM → GB18030 的顺序识别，统一写成 UTF-8 缓存（SRT/VTT 带 BOM），再以 `--sub-auto=no` + 显式 `--sub-file` 加载。原始字幕文件不改；缓存位于 `<data_root>/cache/subtitles/<源文件路径哈希>/<原文件名>`。

缓存目录必须保留**原始文件名**，因为 mpv 用 `--sub-file` 路径的 basename 作为轨道标题；如果缓存叫 `e652deb1e3340932.srt`，字幕菜单里就会显示这串哈希。单视频目录里没有文件名匹配时（例如 `JPSC.ass`、`JPTC.ass`），会加载该目录下所有文本字幕。

### 字幕时间偏移

字幕菜单里额外提供 `-` / `+` / `复位`，每次调整 0.1 秒，只维护一个 `sub-delay` 参数（正数延后、负数提前，范围 ±60s）。点击字幕轨道不会收起面板，方便连续切换和调时间。每次换集 mpv 是新进程，前端把本地偏移重置为 0。

### 单击画面收起控制栏

原生视频窗口会吞掉网页点击，所以后端鼠标轮询里同时检测左键按下，并用 `GetWindowRect` 判断光标是否在视频子窗口内，是则发 `player-click` 事件；网页黑边区域则由 `videoRef.onClick` 直接处理。两者都只调用 `hideControls()`，不再切换播放/暂停。

`mpv-input.conf` 每次启动都会重写为 `MBTN_LEFT ignore`，否则 mpv 自带的左键绑定仍会把点击变成暂停。控制栏现在在窗口模式和全屏都允许收起；窗口模式不做静止自动隐藏，鼠标移动仍会唤出。

### 空格播放/暂停不唤出控制栏

全屏自动隐藏的 effect 不要在 `playerState.paused` 变化时无条件 `setControlsVisible(true)`——空格暂停会触发它，控制栏就会重新出现。只有鼠标活动（`playerActivity`）才主动唤出；`controlsVisible=false` 时 effect 直接返回，不再启动隐藏计时。

### 页面切换的旧页快照

整页滑动时旧页面不能直接放在 `absolute inset-0` 的内容容器里：容器高度是页面内容高度，长选集会溢出到下一页；而且重新渲染的旧页面总是从滚动顶部开始。改为把旧页快照固定到主滚动区域的 `clientWidth/clientHeight`，内部 `overflow-hidden` 并按离开时的 `scrollTop` 滚好，再对固定层做 `translateY(±100%)`。旧页快照的 `scrollTop` 必须在 layout effect 里设置，和封面飞回动画处于同一帧。

### 选集页返回的滚动位置

主滚动容器是 `AppShell` 里的 `<main>`，通过 `scrollRef` 暴露给 `App`。打开详情前保存 `scrollTop`，返回时在 `useLayoutEffect` 里恢复——必须早于封面飞回动画的 `requestAnimationFrame` 采样，否则浮层飞向的是旧的卡片位置。导航到其他页面时要清空待恢复值，避免之后误恢复。

## Tauri 2 / 窗口

- 前端的窗口 API（`setFullscreen` / `setDecorations` / `setAlwaysOnTop` / `setSkipTaskbar`）默认可能被 capability 拒绝，会报权限错误。窗口模式改动放在 Rust 侧 `apply_window_mode` 执行，不依赖前端权限。
- Windows 下窗口处于最大化时直接 `set_fullscreen(true)`，窗口会变成显示器大小，但客户区（WebView）仍保留工作区高度，全屏底部出现任务栏高度的黑边。进入全屏前必须先 `unmaximize()`；用静态 `RESTORE_MAXIMIZED` 记住进入前是否最大化，退出全屏回窗口模式时再 `maximize()`。`is_fullscreen()` 为 true 时不要重复记录/还原，否则会把全屏状态误判成最大化。
- `set_decorations(false)` 必须在 `set_fullscreen(true)` **之前**调用。顺序反过来时，在最大化状态下切换会把窗口丢到屏幕外（Windows 的 -16000 隐藏位置，尺寸缩成 157×25），切回窗口模式也不会自行恢复。回窗口模式时再加一次「是否完全在屏幕外」的检查，异常时 `set_size(1180×780)` + `center()` 兜底。
- 启动流程：`tauri.conf.json` 里窗口 `visible: false` → Rust 初始化时应用窗口模式 → `window.show()`。配合 `backgroundColor: "#0a0a0a"` 和 `index.html` 里 `html/body/#root` 的内联深色背景，避免启动白闪。
- **真正的独占全屏无法实现**：WebView2 由 DWM 合成，没有 DXGI 交换链接管显示器的能力。只保留「窗口 / 全屏」两种模式；不要提供假的「独占全屏」选项。
- `quit_app` 命令内部调用 `app.exit(0)`。退出时 WebView2 可能打印 `Failed to unregister class Chrome_WidgetWin_0`，这是关闭阶段的正常日志噪音。
- NSIS 默认 `currentUser`，默认安装目录 `%LOCALAPPDATA%\KanFanOS` 正好与旧数据目录相同；装到自定义目录（如 `D:\KanFanOS`）时数据在 `<安装目录>\data`。Tauri 卸载器只删除它安装的具体文件，`RMDir "$INSTDIR"` 是**非递归**的，所以两种布局下的 `database\`、`thumbnails\`、`config\`、`logs\`（以及 `data` 目录）都会保留；它不会读取 `settings.json`，因此不会删除番剧根目录。
- 卸载页的「删除应用数据」只执行 `RmDir /r "$LOCALAPPDATA\com.kanfanos.app"`（WebView2 数据），不会清理 `%LOCALAPPDATA%\KanFanOS` 或 `<安装目录>\data` 里的媒体库数据。不要把番剧根目录设在这些目录内部，手动清理时才会误删。

## 数据与扫描

- `watch_history` 不裁剪；最近观看的 `LIMIT 6` 只影响展示。
- 最近观看按 `series_id` 用 `ROW_NUMBER()` 取每部番最近打开的一集；不要在 SQL 里按视频逐条返回，前端要进入选集页而不是直接开播放器。
- `videos.kind` 只有 `main`（选集）和 `extra`（其他视频）。分类由每个番剧的 `kanfanos.toml` 决定：
  - `episode_mode = "auto"`（默认）：根目录视频 → `main`，子目录视频 → `extra`（旧行为）
  - `episode_mode = "manual"`：只有 `episodes` 列表里的相对路径 → `main`，其余一律 → `extra`
- 扫描、`Database::open` 里的 `reclassify_video_kinds`、以及保存配置后的 `reclassify_series_videos` 共用 `series_config::classify_kind`。启动时的重新分类会按番剧读取配置，不会覆盖手动选择。
- `episodes` 使用相对番剧目录、`/` 分隔的路径，比较时忽略大小写。文件改名后会掉出选集（变成 `extra`），需要在选择器里重新勾选；配置里残留的旧路径不会被误当作选集。
- 保存选集配置后立即调用 `reclassify_series_videos` 更新数据库，选集页通过现有的 `onChanged` 刷新，不需要重扫。
- `episode_count` 只统计 `kind='main'`；「其他视频」数量只统计 `kind='extra'`。
- `剧场版` 特判在 `catalog.rs`：`MOVIE_CATEGORY`、`is_movie_category`、`schedule_sort_key` 返回 `i64::MAX` 保证排在侧栏最上方。扫描器把顶层 `剧场版/` 当分类，子目录按影片处理。
- settings.json 新增字段都有 serde 默认值，读取和保存时都会规范化（页面动画、选集动画、窗口模式、缩略图宽度），旧配置不会产生非法值。当前默认值按用户配置：`autoWatch=false`、页面动画 `slide`、选集动画 `flip`、窗口模式 `borderless`、qBittorrent 内置实例；番剧根目录仍为 `None`，首次安装必须自己选。
- 开机自启动写在 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 的 `KanFanOS` 值，data 为带引号的当前 exe。发布版启动时按 `auto_start` 同步一次，安装路径变化后能自动修正；**调试版只保存配置、不写注册表**，避免把 `target\debug\kanfanos.exe` 注册成启动项。卸载器目前不会主动删除这个 Run 值。
- 数据目录由 `paths.rs` 选择：默认安装（exe 就在 `%LOCALAPPDATA%\KanFanOS`）用旧布局；**release 构建**安装到其他可写目录（如 `D:\KanFanOS`）时用 `<安装目录>\data`。开发模式（`target\debug`）和 Program Files 这类不可写目录一律回退 `%LOCALAPPDATA%\KanFanOS`。
- 首次使用 `<安装目录>\data` 时，如果里面有旧数据库就跳过迁移；否则把旧目录的 `database\` 和 `config\` 复制到临时目录再整体改名，缩略图不搬、由媒体队列重新生成。旧目录保留，迁移结果写进数据目录的 `logs\app.log`。迁移失败会回退旧目录，不能把用户挡在门外。
- 缩略图不在静态 asset scope 里时，`initialize` 会按实际 `data_root/thumbnails` 额外调用 `allow_directory`；新增数据目录逻辑时不要漏掉这一步。
- WebView2 的数据目录仍是 `%LOCALAPPDATA%\com.kanfanos.app`（Tauri 的 `app_local_data_dir`），不跟随 `data_root`；所以即使数据迁到 D，C 盘仍有一百多 MB 的 WebView2 缓存。
- `logs\mpv-<剧集 id>.log` 每个剧集一份，播放时覆盖写，目前不会自动清理；大库长期使用会累积，排查数据目录占用时先看 `logs\`。

## 资源页（内嵌浏览器 + qBittorrent，PoC）

- 内嵌浏览器用 Tauri 的多 webview：`Window::add_child(WebviewBuilder, ...)`，必须在 `tauri` 上启用 `unstable` feature。主 webview 的 label 是 `main`，每个标签页一个子 webview，label 为 `browser-<id>`（id 由后端自增分配）。
- 子 webview 是**原生子窗口**，永远画在主 WebView 之上，不会跟随 DOM 层级。页内其他浮层/弹窗也无法盖住它。
- 切换标签页和离开资源页都**不销毁 webview**：`browser_hide_tab` 把它移到 `(-20000,-20000)` 并缩成 1x1，焦点还给主 webview；页面状态（滚动、表单、播放）全部保留。恢复时 `browser_show_tab` 更新位置和大小，返回 `false` 才说明 webview 已不存在。
- 后端用 `HIDDEN_TABS` 记录隐藏中的标签页，`set_tab_bounds` 遇到隐藏标签直接返回。`hide` / `show` / `set_bounds` 共用一把锁串行化，否则切页时布局变化触发的迟到定位会把 webview 重新拉回可见区域（表现为浏览器一直盖在其他页面上）。
- 离开资源页时 `App::navigate` 不生成离场快照：快照会再挂载一个 `ResourcesPage`，它的恢复逻辑同样可能把刚隐藏的 webview 显示出来。
- **孤儿 webview 兜底**：界面状态丢失、关闭失败或重复实例可能留下没人跟踪的 `browser-*` webview，它们会保持全尺寸并盖住所有页面。后端提供 `browser_list_tabs` 和 `browser_close_untracked(keep)`；资源页在**进入和离开**时都按当前标签列表清理未跟踪的 webview，`closeTab` / 恢复失败时也会补一次。排查时可以通过 CDP 读 `window.__kanfanosTabId` 确认每个 webview 的标签 id。
- 资源页的界面状态（地址、标签页列表、当前标签 id、qBittorrent 版本、种子列表、下载目录）存在 `ResourcesPage` 的模块级 `sessionCache` 里；组件卸载后重新进入时从这里恢复。恢复定位要等页面切换动画结束（当前 380ms），否则 `getBoundingClientRect` 会拿到动画中的 transform 矩形。
- 隐藏的 webview 仍在后台运行，音频、网络请求不会暂停；标签页一旦创建就常驻到应用退出或手动关闭。
- 标签页标题和 URL 由后端 `browser-tab-updated` 事件推送（`on_document_title_changed` + `on_page_load(Finished)`）；`target=_blank` / `window.open` 发 `browser-new-tab` 事件，前端据此新建标签页。地址栏只在输入框没有焦点时才跟随事件更新，避免覆盖用户正在输入的内容。
- **没有默认首页**：新建标签页或空地址点「打开」时，后端把空字符串解析成 `about:blank`（不加载任何网站）。地址栏把 `about:blank` 显示为空，标签栏显示「新标签页」，空白页不能收藏。
- **不要在同步命令里调用 `add_child`**：它内部 `run_on_main_thread` + 同步等待，同步命令会占住主线程，表现为点击后一直卡在“打开中”、子 webview 停在 `about:blank`。浏览器命令必须写成 `async fn`，并用 `tauri::async_runtime::spawn_blocking` 包住，主线程才能执行创建闭包。
- `add_child` / `set_position` / `set_size` 收的是**逻辑像素**（前端 CSS px 直接传），Tauri 自己乘缩放因子；不要套用 `playback::scaled_rect` 那套物理像素换算。
- 原生窗口不随 DOM 布局移动：布局、窗口大小、滚动变化时都要重新 `browser_set_tab_bounds`。当前用 `ResizeObserver` + `resize` + 捕获阶段 `scroll`。
- 网页默认缩放改成 80%（`browser::PAGE_ZOOM_PERCENT`），工具栏可 10% 增减；`browser_set_zoom` 会应用到所有已创建标签页，创建/显示/导航/刷新时也使用当前值。右键菜单和浏览器内 Toast 用 `window.__kanfanosZoom` 反向缩放，保持 100% 物理大小。
- 工具栏的后退用 `history.back()`，刷新用 `Webview::reload()`；两者都必须走 `spawn_blocking`，避免阻塞主线程。
- 子 webview 的 `target=_blank` 在部分页面不会触发 `on_new_window`：初始化脚本捕获点击后先调用 `browser_request_new_tab`，IPC 不可用时退化为导航到 `https://kanfanos.invalid/open?url=...`，由 `on_navigation` 拦截并发 `browser-new-tab`。
- 右键菜单：主 webview 在 `main.tsx` 用捕获阶段 `contextmenu.preventDefault()`；每个浏览器子 webview 通过 `initialization_script_for_all_frames` 注入同样逻辑，覆盖 iframe。子 webview 右击 `<img>` 时额外显示 Shadow DOM 菜单「图片另存为」。
- 图片保存命令 `browser_save_image` 用 reqwest（启用 `system-proxy`，跟随 Windows 系统代理）下载，写入 `qbittorrent.save_path`；为空时写到 `<data_root>/qbittorrent/qBittorrent/downloads`。远程 webview 的 IPC 不可用时，菜单退化为 `kanfanos.invalid/save-image` 导航，由 `on_navigation` 拦截并异步保存。
- 选中文字右键菜单项「以选中文字创建番剧目录及配置文件」走 `browser_create_folder`：`< > : " / \ | ? *` 换成普通空格；换行、制表符、连续空白直接删除不留空格；首尾空格/点去掉，最多 80 字，保留名加 `_` 前缀，重名自动追加 ` (2)`。创建后在文件夹里生成 `kanfanos.toml`：原文含非法符号时把去掉换行后的原文写进 `name`；不含非法符号时 `name` 留空、显示默认文件夹名。成功发 `browser-folder-created`，主界面收到后调用 `setQbittorrentSavePath` 自动把「下载到」切到新目录。IPC 不可用时同样退化为 `kanfanos.invalid/create-folder`。
- qBittorrent 的「外部 WebUI」模式与 URL/用户名/密码由设置页的「qBittorrent 高级连接设置」总开关控制；资源页卡片只保留内置实例、WebUI 按钮和下载目录。
- 资源页通过 `AppShell` 的 `wide` 属性跳过 `max-w-6xl`，最大化窗口时左右不再留大片空白；其他页面保持原宽度。资源页自己的高度用 `calc(100vh - 3rem)` 对应 `py-6`。
- 浏览器区域不锁比例，直接铺满右侧剩余空间；右侧 qBittorrent 面板固定 `w-60`（240px）。
- 下载路径用 `truncate` + `[direction:rtl] text-left` 省略开头、保留结尾。Windows 路径里的 `\` 是中性字符，被 LTR 字母包围时不会打乱顺序。
- 只允许 http/https；`target=_blank` 和 `window.open` 通过 `on_new_window` 交给前端开新标签页（`NewWindowResponse::Deny`），避免弹出不受控的顶层窗口。
- 收藏夹的菜单和「管理收藏夹」弹窗是 HTML 层，会被原生 webview 盖住；打开时先把当前标签页隐藏（`overlayOpen`），关闭后按当前布局恢复。其他任何需要覆盖浏览器区域的 React 弹窗也要遵守这条。
- 收藏夹保存在 `<data_root>\config\bookmarks.json`。地址会补全协议、规范化后按 URL 去重；重复收藏同一地址只更新标题。新增/修改/删除接口都返回完整列表，前端直接替换状态。
- 磁力链接（`magnet:`）和 URL 以 `.torrent` 结尾的链接分别由 `on_navigation` / `on_new_window` / `on_download` 拦截：回调返回 `false` 取消 WebView2 自己的导航/下载，再在后台任务里调用 qBittorrent API 添加，完成后发 `qbittorrent-torrent` 事件给前端刷新列表。`add_child` 的构建回调必须是同步的，所以网络请求只能另起异步任务，不能在回调里 `await`。
- 带查询串或跳转的种子下载（例如 `download.php?id=...`）无法仅凭 URL 判断，目前不拦截，仍由 WebView2 下载。
- 下载目录存在 `settings.json` 的 `qbittorrent.savePath`：内置实例启动后通过 `/app/setPreferences` 应用；运行中修改目录时直接调用同一个接口。为空表示使用 qBittorrent 默认目录。
- qBittorrent 默认走**内置 sidecar**：`qbittorrent/managed.rs` 启动随包分发的 `qbittorrent.exe`，使用 `--profile=<data_root>\qbittorrent`、`--webui-port=<随机空闲端口>`、`--no-splash --confirm-legal-notice`。实例只监听 `127.0.0.1`，退出应用时先 POST `/api/v2/app/shutdown`，5 秒未退出再 kill。
- qBittorrent 的 WebUI **即使开启 localhost 免登录也必须配置非空密码哈希**才会启动（日志里的“未设置凭据”）。托管配置 `<profile>\qBittorrent\config\qBittorrent.ini` **只在首次启动时写入**，之后保留 qBittorrent 自己的配置，这样用户在 WebUI 里改的代理、限速等设置不会在下次启动时被覆盖。`Password_PBKDF2` 的格式是 `base64(salt):base64(PBKDF2-HMAC-SHA512(password, salt, 100000, 64B))`；`LocalHostAuth=false`，所以内置模式不需要登录（登录接口会返回 204 空响应）。
- WebUI 端口每次启动随机分配（`--webui-port`），qBittorrent 会把它写回自己的配置；前端通过 `qbittorrent_webui_url` 命令取当前地址，右侧「WebUI」按钮用 `newTab` 在内嵌浏览器里**新开标签页**打开完整界面，不顶掉当前页面。因为 `LocalHostAuth=false`，任何本机进程都能控制这个 WebUI，所以只监听 `127.0.0.1`。
- 右侧列表只显示**当前下载目录下的任务**：每次轮询先读 `/app/preferences` 的 `save_path`，再用 `/torrents/info` 拉取全部任务，在 Rust 侧只按 `save_path` 过滤（忽略大小写、斜杠和末尾分隔符差异）。不区分状态，已停止、暂停、出错的任务也会显示；也不使用 qBittorrent 的 `filter=active`（它只返回当前有传输速率的任务）。
- `qbittorrent_list_torrents` 返回 `{ savePath, torrents }`。用户在界面里换目录时先立即清空列表、暂停轮询并递增一个 epoch，丢弃切换前发出、切换后才返回的旧结果；等新目录的列表读回来再填充。外部（WebUI）改了目录时，轮询发现 `savePath` 变化也会同步「下载到」显示并替换列表。
- 每条任务的启动/停止走 `/api/v2/torrents/start` 与 `/stop`（form 字段 `hashes`）。停止态（`stoppedDL/stoppedUP/pausedDL/pausedUP/error/missingFiles`）显示「启动」，其余显示「停止」。点击后先乐观更新状态，600ms 再轮询校正；失败用面板顶部的提示条显示，不替换整个任务列表。
- 右侧种子列表在页面可见时每 2 秒轮询一次；面板上的状态、进度和速度不是事件推送，离开资源页就停止轮询。
- 资源页每次进入都会自动连接 qBittorrent（`autoConnect` → `refresh`）：内置模式会顺手拉起 sidecar。已经连接过（`sessionCache.qbVersion` 有值）时只做后台刷新，不闪「正在连接…」。
- 连接按钮同时是状态指示：`正在连接…`（转圈、禁用）→ `已连接`（绿色勾）或 `重试连接`（失败后）。轮询连续 3 次失败会清掉已连接状态并显示错误，让用户能看出连接断了。
- qBittorrent 是 GUI 程序，官方 Windows 包没有无界面模式：启动后通过 Win32 `EnumWindows` 按 PID 隐藏它的顶层窗口，并在一段时间内重复隐藏（窗口可能晚于 WebUI 就绪创建）。
- 外部 WebUI 模式仍然保留（`settings.json` 的 `qbittorrent.mode = "external"`）。登录是 `POST /api/v2/auth/login`（form），必须带 `Origin` / `Referer`，否则 403；SID 由 reqwest 的 cookie store 保存。密码明文存在 `settings.json`，与 qBittorrent 自身配置的做法一致。
- 会话缓存放在 `QbState`，连接参数或模式变化时 `invalidate`；请求遇到 403 视为 SID 过期，丢弃缓存后重试一次。
- 下载目录默认是 `<data_root>\qbittorrent\qBittorrent\downloads`（qBittorrent 自定义 profile 的默认下载位置）。卸载器不会删 `qbittorrent\`，和数据库一样保留。
- 内置 `qbittorrent.exe` 来自官方 Release 5.3.0，分发需遵守 GPLv3+：许可证随包放在 `qbittorrent-COPYING.txt`，源码地址写在 README。升级版本时同步更新 `scripts/fetch-sidecars.mjs` 里的版本号和 SHA256。

## 1.2.2 细节

- 浏览器子 webview 开启 `enable_clipboard_access()`；自定义右键菜单对选中的文字提供「复制文字」，对可编辑控件提供「粘贴文字」，均只处理纯文本。复制优先用 `navigator.clipboard.writeText`，失败回退 `execCommand("copy")`；粘贴用 `navigator.clipboard.readText`，读取失败时提示按 Ctrl+V。
- 最近观看的下一步由前端读取主选集并按 `sortEpisodes` 自然排序推导：已完成且非最后一集显示「播放下一集」，最后一集显示「重新播放」并从第一集打开。
- 播放器通过 mpv 的 `audio-device-list` / `audio-device` 属性提供输出设备列表和切换，UI 与字幕/音轨的 chip 面板一致；只改变 mpv 播放输出，不修改 Windows 全局默认设备。

## 图标

- 源图放在项目根目录 `icon.png`（正方形、建议 1024×1024 以上、透明背景）。
- 运行 `pnpm icon`（即 `tauri icon icon.png`）重新生成 `src-tauri/icons/`；侧栏 logo 直接引用 `src-tauri/icons/128x128@2x.png`，换图标后无需再手动拷贝。
- `tauri icon` 会顺带生成 `icons/android/` 和 `icons/ios/`，本项目只面向 Windows，这两个目录已在 `.gitignore` 中忽略。
- 图标生成后需要重新编译（Rust 的 build script 会重新嵌入 `.ico`）；Windows 任务栏有图标缓存，必要时重启资源管理器。

## 调试

- 运行时检查可以给 Tauri 设置 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`，然后用 CDP 读取 DOM 矩形/动画。正常的用户测试不需要开。
- 原生窗口的实际矩形可以用 Win32 `EnumChildWindows` 找 `KanFanOSVideoSurface` 再 `GetWindowRect` 采样。
- 常用检查：`pnpm build`、`cd src-tauri && cargo test`、`cargo clippy --all-targets`。
