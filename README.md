# KanFanOS

面向 Windows 的本地个人视频媒体库。扫描、索引并组织已有视频文件，不改动原始视频；每部番剧文件夹内会生成一份可手动编辑的 `kanfanos.toml`。

技术栈：Tauri 2 + React + TypeScript + Tailwind CSS + Rust + SQLite。

完整设计见 [KanFanOS.md](./KanFanOS.md)，环境配置见 [docs/SETUP.md](./docs/SETUP.md)。

## 开发

```bash
pnpm install
pnpm tauri dev
```

首次运行会在 `%LOCALAPPDATA%\KanFanOS\` 下创建数据目录：

```text
%LOCALAPPDATA%\KanFanOS\
├── database\library.sqlite
├── thumbnails\
├── logs\
└── config\
```

## 构建

```bash
pnpm tauri build --bundles nsis
```

安装包输出：

```text
src-tauri/target/release/bundle/nsis/KanFanOS_0.1.0_x64-setup.exe
```

> 安装包不包含 FFmpeg / FFprobe / MPV，目标机器需要自行安装并加入 PATH。
> MSI 打包需要下载 WiX，网络受限时可能失败，可只用 NSIS。

## 检查

```bash
pnpm build                     # 前端类型检查 + 打包
cd src-tauri
cargo build
cargo test                     # 扫描器集成测试
cargo clippy --all-targets     # Lint
```

## 目录约定

KanFanOS 读取一个番剧根目录，支持两种结构：

```text
番/
├── 2026年7月新番/          # 档期目录（文件夹名包含 YYYY年M月）
│   └── 番剧名/             # 番剧
│       ├── 第1集.mp4
│       ├── 第2集.mkv
│       ├── kanfanos.toml   # 自动生成，可手动编辑
│       └── cover.jpg       # 可选，封面图片
├── 2025年10月新番/         # 暂时为空也没关系，会保留在侧栏
└── 进击的巨人第一季/        # 没有档期信息 → 归入「未知时间」
    └── 第1集.mp4
```

- 视频放在番剧文件夹内，可以再嵌套子目录。
- 档期目录名中的年份和月份会被解析，最新的排在最上面。
- 首次扫描会在每部番剧文件夹内生成 `kanfanos.toml`，之后**不会被覆盖**。
- `kanfanos.toml` 支持：
  - `name`：显示名称（留空用文件夹名）
  - `aliases`：搜索别名数组，例如 `["英文名", "简称"]`
  - `cover`：封面图片相对路径，例如 `"cover.jpg"`；留空时自动查找 `cover/poster/folder` 图片
- 修改配置或替换封面后，文件监听会自动同步，无需重新扫描。
- 番剧卡片优先显示文件夹内的封面，没有封面时使用第一集缩略图。

## 当前进度

**第 1 步：项目基础**

- Tauri 2 项目骨架与 IPC 命令
- SQLite 连接（WAL、外键、busy_timeout）与版本化迁移
- 版本化迁移：videos / libraries / series / categories / tags / favorites / watch_history
- 导航骨架与设置页

**第 2 步：扫描与索引**

- 通过系统目录选择对话框设置番剧根目录
- 按「档期 → 番剧 → 剧集」层级建立索引，每批 1000 条写入
- 增量扫描：文件大小或修改时间变化时清空派生元数据
- 缺失判定：完整扫描后把未发现的文件标记为 `missing`
- 扫描进度事件、取消扫描、单文件读取失败计数

**第 3 步：番剧媒体库界面**

- 侧栏：最近观看、档期列表（含空档期与「未知时间」）、设置
- 档期页：番剧卡片网格，显示集数
- 番剧详情：按集数自然排序的剧集列表
- 已配置目录但无索引时自动扫描一次

**第 4 步：元数据与缩略图**

- FFprobe 提取时长、分辨率、视频/音频编码、帧率
- FFmpeg 在时长 20% 处截帧，生成 480px JPEG 缩略图
- 后台处理队列：最多 3 个并发进程，失败重试 3 次
- 番剧卡片使用第一集缩略图作为封面，剧集列表显示缩略图与时长
- 扫描完成后自动启动，应用重启后继续未完成任务

**第 5 步：浏览与搜索**

- 侧栏全局搜索：番剧名或剧集文件名，250ms 防抖
- 搜索结果展示命中的剧集文件名，每条结果附带档期
- 档期内排序：名称 / 集数 / 更新时间
- 分页查询：每页 60 条，支持「加载更多」

**第 6 步：播放与观看记录**

- 外部 MPV 播放，自动加载同名或近似名字幕
- 上次看到 5 秒以上且未看完时，从上次位置继续播放
- 通过 mpv JSON IPC 每 5 秒记录播放位置，退出时写入最终进度
- 播放进度显示在剧集列表和最近观看中
- 最近观看按打开时间排序，显示进度并支持续播

**第 7 步：标签与收藏（跳过）**

- 当前需求不需要，暂不实现；相关表结构已在初始迁移中保留

**第 8 步：文件监听与同步**

- 递归监听番剧目录（Windows `ReadDirectoryChangesW`）
- 1.5 秒防抖，合并复制大文件产生的连续事件
- 新增、删除、重命名后自动增量扫描，无需手动刷新
- 新文件自动进入元数据与缩略图队列
- 目录离线时暂停同步并提示，不误标为缺失

**第 9 步：设置、缓存与打包**

- 设置页：自动同步开关、缩略图宽度（320 / 480 / 640 / 960 px）
- 缩略图缓存统计与一键清理，清理后下次启动自动重建
- Windows 安装包构建

**第 10 步：番剧文件夹内的元数据**

- 每部番剧自动生成 `kanfanos.toml`，可编辑显示名、别名、封面
- 别名参与搜索，支持中文名之外的简称或外文名
- 封面图片直接放在番剧文件夹内，通过 asset 协议读取
- 配置或封面修改后由文件监听自动同步

## 已知限制

- 尚未用真实的大规模视频库做完整验收，目前测试数据是 FFmpeg 生成的短视频。
- 文件重命名后观看记录不会迁移（按路径建立索引，旧记录会变成 `missing`）。
- 旧版本在 `%LOCALAPPDATA%\KanFanOS\config\series\` 生成的 JSON 配置已废弃，可以手动删除。
- 安装包不包含 FFmpeg / FFprobe / MPV，需要目标机器已安装并加入 PATH。
- MSI 打包依赖 WiX 下载，网络受限时使用 `--bundles nsis`。
- 列表使用分页查询，暂未引入虚拟滚动。

## 目录说明

```text
src/                    React 前端
├── api/                Tauri 命令封装
├── components/         通用组件与布局
├── pages/              页面
├── types/              共享类型
└── lib/                工具函数

src-tauri/              Rust 后端
├── migrations/         SQL 迁移脚本（按版本追加）
└── src/
    ├── commands/       IPC 命令（catalog / library / media / settings）
    ├── database/       连接、迁移、libraries / series / videos
    ├── media/          FFprobe 元数据与 FFmpeg 缩略图队列
    ├── scanner/        目录扫描与番剧配置文件生成
    ├── watcher/        文件系统监听与自动同步
    ├── catalog.rs      档期目录名解析
    ├── settings.rs     settings.json 读写
    ├── paths.rs        数据目录解析
    └── state.rs        应用状态
```
