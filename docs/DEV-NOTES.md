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
- 启动流程：`tauri.conf.json` 里窗口 `visible: false` → Rust 初始化时应用窗口模式 → `window.show()`。配合 `backgroundColor: "#0a0a0a"` 和 `index.html` 里 `html/body/#root` 的内联深色背景，避免启动白闪。
- **真正的独占全屏无法实现**：WebView2 由 DWM 合成，没有 DXGI 交换链接管显示器的能力。只保留「窗口 / 全屏」两种模式；不要提供假的「独占全屏」选项。
- `quit_app` 命令内部调用 `app.exit(0)`。退出时 WebView2 可能打印 `Failed to unregister class Chrome_WidgetWin_0`，这是关闭阶段的正常日志噪音。
- NSIS 默认 `currentUser`，默认安装目录 `%LOCALAPPDATA%\KanFanOS` 正好与旧数据目录相同；装到自定义目录（如 `D:\KanFanOS`）时数据在 `<安装目录>\data`。Tauri 卸载器只删除它安装的具体文件，`RMDir "$INSTDIR"` 是**非递归**的，所以两种布局下的 `database\`、`thumbnails\`、`config\`、`logs\`（以及 `data` 目录）都会保留；它不会读取 `settings.json`，因此不会删除番剧根目录。
- 卸载页的「删除应用数据」只执行 `RmDir /r "$LOCALAPPDATA\com.kanfanos.app"`（WebView2 数据），不会清理 `%LOCALAPPDATA%\KanFanOS` 或 `<安装目录>\data` 里的媒体库数据。不要把番剧根目录设在这些目录内部，手动清理时才会误删。

## 数据与扫描

- `watch_history` 不裁剪；最近观看的 `LIMIT 6` 只影响展示。
- 最近观看按 `series_id` 用 `ROW_NUMBER()` 取每部番最近打开的一集；不要在 SQL 里按视频逐条返回，前端要进入选集页而不是直接开播放器。
- `videos.kind` 区分 `main`（番剧根目录）和 `extra`（子目录 CM/PV）。扫描时按 `path.parent() == series_dir` 分类；旧库在 `Database::open` 后用 `reclassify_video_kinds` 按实际路径修正，避免升级后必须重扫。
- `episode_count` 只统计 `kind='main'`；「其他视频」数量只统计 `kind='extra'`。
- `剧场版` 特判在 `catalog.rs`：`MOVIE_CATEGORY`、`is_movie_category`、`schedule_sort_key` 返回 `i64::MAX` 保证排在侧栏最上方。扫描器把顶层 `剧场版/` 当分类，子目录按影片处理。
- settings.json 新增字段都有 serde 默认值，读取和保存时都会规范化（页面动画、选集动画、窗口模式），旧配置不会产生非法值。
- 开机自启动写在 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 的 `KanFanOS` 值，data 为带引号的当前 exe。发布版启动时按 `auto_start` 同步一次，安装路径变化后能自动修正；**调试版只保存配置、不写注册表**，避免把 `target\debug\kanfanos.exe` 注册成启动项。卸载器目前不会主动删除这个 Run 值。
- 数据目录由 `paths.rs` 选择：默认安装（exe 就在 `%LOCALAPPDATA%\KanFanOS`）用旧布局；**release 构建**安装到其他可写目录（如 `D:\KanFanOS`）时用 `<安装目录>\data`。开发模式（`target\debug`）和 Program Files 这类不可写目录一律回退 `%LOCALAPPDATA%\KanFanOS`。
- 首次使用 `<安装目录>\data` 时，如果里面有旧数据库就跳过迁移；否则把旧目录的 `database\` 和 `config\` 复制到临时目录再整体改名，缩略图不搬、由媒体队列重新生成。旧目录保留，迁移结果写进数据目录的 `logs\app.log`。迁移失败会回退旧目录，不能把用户挡在门外。
- 缩略图不在静态 asset scope 里时，`initialize` 会按实际 `data_root/thumbnails` 额外调用 `allow_directory`；新增数据目录逻辑时不要漏掉这一步。
- WebView2 的数据目录仍是 `%LOCALAPPDATA%\com.kanfanos.app`（Tauri 的 `app_local_data_dir`），不跟随 `data_root`；所以即使数据迁到 D，C 盘仍有一百多 MB 的 WebView2 缓存。
- `logs\mpv-<剧集 id>.log` 每个剧集一份，播放时覆盖写，目前不会自动清理；大库长期使用会累积，排查数据目录占用时先看 `logs\`。

## 图标

- 源图放在项目根目录 `icon.png`（正方形、建议 1024×1024 以上、透明背景）。
- 运行 `pnpm icon`（即 `tauri icon icon.png`）重新生成 `src-tauri/icons/`；侧栏 logo 直接引用 `src-tauri/icons/128x128@2x.png`，换图标后无需再手动拷贝。
- `tauri icon` 会顺带生成 `icons/android/` 和 `icons/ios/`，本项目只面向 Windows，这两个目录已在 `.gitignore` 中忽略。
- 图标生成后需要重新编译（Rust 的 build script 会重新嵌入 `.ico`）；Windows 任务栏有图标缓存，必要时重启资源管理器。

## 调试

- 运行时检查可以给 Tauri 设置 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`，然后用 CDP 读取 DOM 矩形/动画。正常的用户测试不需要开。
- 原生窗口的实际矩形可以用 Win32 `EnumChildWindows` 找 `KanFanOSVideoSurface` 再 `GetWindowRect` 采样。
- 常用检查：`pnpm build`、`cd src-tauri && cargo test`、`cargo clippy --all-targets`。
