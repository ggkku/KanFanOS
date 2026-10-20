# KanFanOS 开发环境配置指南

> 目标：从一台已经装好 Node.js 的 Windows 11 机器出发，配置出可以运行 `pnpm tauri dev` 的完整 Tauri 2 + Rust 开发环境。
>
> 预计总耗时：**40～70 分钟**（其中 Rust 安装和首次编译占大头）。
> 每一步都附带「验证命令」和「预期结果」，**验证不通过就不要进入下一步**。

---

## 0. 环境目标

| 组件 | 目标版本 | 作用 |
|---|---|---|
| Node.js | ≥ 20（本机已有 v24.18.0） | 前端构建 |
| pnpm | ≥ 9 | 包管理与锁文件 |
| Rust | stable MSVC | 后端编译 |
| MSVC C++ 工具链 | VS 2022/2026 + C++ 工作负载 | Rust 链接器 `link.exe` |
| WebView2 Runtime | ≥ 100 | Tauri 窗口运行时 |
| FFmpeg / FFprobe | ≥ 6 | 视频元数据与缩略图 |
| MPV | ≥ 0.37 | 外部播放器 |
| Tauri CLI | 2.x | 项目构建（项目内安装） |

> **本机已具备**：Node.js v24.18.0、npm 11.16.0、Git 2.55.0、Visual Studio 2026 Community（含 MSVC x64 工具链与 Windows 11 SDK）、WebView2 155.0.4283.62。
> 下面第 1 步只是复核，确认无变化即可跳过安装。

---

## 1. 前置复核（约 2 分钟）

打开 **PowerShell**（普通权限即可），逐条执行：

```powershell
node -v            # 期望 v20 以上
npm -v
git --version
```

检查 Visual Studio 的 C++ 工作负载是否存在：

```powershell
& "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" `
  -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
  -property installationPath
```

- ✅ 输出了类似 `C:\Program Files\Microsoft Visual Studio\18\Community` → 通过
- ❌ 无输出 → 打开 **Visual Studio Installer → 修改 → 勾选「使用 C++ 的桌面开发」→ 安装**
  （同时确保右侧「Windows 11 SDK」被勾选）

检查 WebView2：

```powershell
reg query "HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" /v pv
```

- ✅ 有 `pv REG_SZ 155.x` → 通过
- ❌ 报错 → 安装：<https://developer.microsoft.com/microsoft-edge/webview2/>

---

## 2. 安装 Rust（约 10～15 分钟）

### 2.1 安装 rustup

```powershell
winget install --id Rustlang.Rustup -e
```

如果 winget 下载缓慢或失败，改用官方安装器：
<https://www.rust-lang.org/tools/install> → 下载 `rustup-init.exe` → 双击运行 → 输入 `1` 选择默认安装。

### 2.2 **关闭当前终端，重新打开一个新的 PowerShell**

这一步很关键。安装器把 `%USERPROFILE%\.cargo\bin` 写进了用户 PATH，只有新开的终端才会读到。

### 2.3 锁定 MSVC 工具链

```powershell
rustup default stable-x86_64-pc-windows-msvc
rustup show
```

### 2.4 验证

```powershell
rustc --version
cargo --version
```

预期输出类似：

```
rustc 1.9x.0 (xxxxxxx 2026-xx-xx)
cargo 1.9x.0 (xxxxxxx 2026-xx-xx)
```

> **为什么必须是 MSVC 而不是 GNU**：Tauri 在 Windows 上的默认构建目标是 `x86_64-pc-windows-msvc`，GNU 工具链会出现链接和 DLL 兼容问题。

---

## 3. 安装 pnpm（约 2 分钟）

```powershell
npm install -g pnpm
```

验证：

```powershell
pnpm -v      # 期望输出 9.x 或 10.x
```

> 如果 `pnpm` 提示「无法加载文件，因为在此系统上禁止运行脚本」，执行：
> ```powershell
> Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
> ```

---

## 4. 安装 FFmpeg / FFprobe（约 5 分钟）

```powershell
winget install --id Gyan.FFmpeg -e
```

**关闭并重新打开终端**，然后验证：

```powershell
ffmpeg -version
ffprobe -version
```

预期各输出一行版本号（`ffmpeg version 7.x ...`）。

若 winget 失败，手动方式：

1. 打开 <https://www.gyan.dev/ffmpeg/builds/>，下载 `ffmpeg-release-essentials.zip`
2. 解压到 `C:\Tools\ffmpeg`
3. 把 `C:\Tools\ffmpeg\bin` 加入系统 PATH（`Win + R` → `sysdm.cpl` → 高级 → 环境变量 → Path → 新建）
4. 重开终端后验证

> **开发阶段**用 PATH 里的系统 FFmpeg 即可，不必放进项目。
> **打包分发阶段**涉及 GPL/LGPL 许可问题，那时再决定是否内置到 `resources/media-tools/`，并替换为许可明确的构建版本（详见设计文档 §11.1）。

---

## 5. 安装 MPV（约 3 分钟）

winget 里有 shinchiro 维护的构建，直接用：

```powershell
winget install --id shinchiro.mpv -e
```

安装程序会把 `mpv.exe` 放到 `C:\Program Files\MPV Player\`，但**不会自动加入 PATH**，需要手动加一次：

```powershell
$p = [Environment]::GetEnvironmentVariable("Path","User")
if ($p -notlike "*MPV Player*") {
    [Environment]::SetEnvironmentVariable("Path", $p.TrimEnd(";") + ";C:\Program Files\MPV Player", "User")
}
```

重开终端后验证：

```powershell
mpv --version
```

预期输出 `mpv v0.41.0 ...`。

若 winget 安装失败，改用手动方式：

1. 打开 <https://mpv.io/installation/>，找到 Windows builds（shinchiro 维护）
2. 下载 64 位压缩包（如 `mpv-x86_64-20xxxxxx-git-xxxx.7z`）
3. 解压到 `C:\Tools\mpv`
4. 把 `C:\Tools\mpv` 加入系统 PATH
5. 重开终端验证

> 第 5 步播放器功能会用到。如果暂时不做到那一步，可以跳过，不影响前 4 步开发。

---

## 6. 系统级配置（可选但建议）

### 6.1 开启长路径支持

视频目录层级深时，路径容易超过 260 字符，导致扫描报错。

**以管理员身份**打开 PowerShell：

```powershell
reg add "HKLM\SYSTEM\CurrentControlSet\Control\FileSystem" /v LongPathsEnabled /t REG_DWORD /d 1 /f
```

验证：

```powershell
reg query "HKLM\SYSTEM\CurrentControlSet\Control\FileSystem" /v LongPathsEnabled
```

### 6.2 国内网络镜像（仅当下载慢时配置）

npm：

```powershell
npm config set registry https://registry.npmmirror.com
pnpm config set registry https://registry.npmmirror.com
```

Cargo：新建文件 `%USERPROFILE%\.cargo\config.toml`，内容：

```toml
[source.crates-io]
replace-with = 'rsproxy-sparse'

[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"

[registries.rsproxy]
index = "https://rsproxy.cn/crates.io-index"

[net]
git-fetch-with-cli = true
```

> 如果公司或学校有代理，优先用代理，不要混用镜像。

### 6.3 关闭 Defender 对 `target` 目录的实时扫描（可选）

Rust 首次编译会产生数万个文件，实时扫描可能让编译时间翻倍。**仅建议对项目目录和 `target` 目录做排除**，不要全局关闭：

Windows 安全中心 → 病毒和威胁防护 → 管理设置 → 排除项 → 添加文件夹 → 选择 `C:\Users\xzc54\Desktop\KanFanOS\src-tauri\target`

---

## 7. 全量验证（约 1 分钟）

新开一个 PowerShell，跑一遍这张表，**全部通过再往下走**：

| # | 命令 | 期望结果 |
|---|---|---|
| 1 | `node -v` | v20+ |
| 2 | `pnpm -v` | 9+ |
| 3 | `rustc --version` | rustc 1.7x+ |
| 4 | `cargo --version` | cargo 1.7x+ |
| 5 | `ffmpeg -version` | 输出版本行 |
| 6 | `ffprobe -version` | 输出版本行 |
| 7 | `mpv --version` | 输出版本行（可选） |
| 8 | `git --version` | 任意版本 |

> 前 4 项是硬性要求，缺任意一项都会导致 `tauri dev` 失败。

---

## 8. 创建 Tauri 2 项目（约 10 分钟）

### 8.1 初始化 Git 仓库

```powershell
cd C:\Users\xzc54\Desktop\KanFanOS
git init
```

### 8.2 生成脚手架

因为当前目录里已经有 `KanFanOS.md` 和 `docs/`，直接原地生成容易覆盖文件。用临时子目录生成后再挪出来：

```powershell
cd C:\Users\xzc54\Desktop\KanFanOS
mkdir _scaffold
cd _scaffold
npm create tauri-app@latest kanfanos -- --manager pnpm --template react-ts --identifier com.kanfanos.app --yes
```

> 交互式写法（等价）：`npm create tauri-app@latest`，依次选择
> React → TypeScript → pnpm → 项目名 `kanfanos`。
>
> 项目名直接用 `kanfanos`（全小写），是为了让 `Cargo.toml` 的包名、`package.json` 的 name、`lib.rs` 的 crate 名一次到位，避免后面改包名还要连带修改 `main.rs` 里的 `kanfanos_lib::run()`。

### 8.3 把脚手架移到项目根目录

Git Bash：

```bash
cd /c/Users/xzc54/Desktop/KanFanOS
shopt -s dotglob
mv _scaffold/kanfanos/* .
rm -rf _scaffold
```

PowerShell：

```powershell
cd C:\Users\xzc54\Desktop\KanFanOS
Get-ChildItem -Path .\_scaffold\kanfanos -Force | Move-Item -Destination . -Force
Remove-Item .\_scaffold -Recurse -Force
```

移动后项目根目录应包含：

```
KanFanOS/
├── KanFanOS.md
├── docs/
├── src/                 # React 前端
├── src-tauri/           # Rust 后端
├── package.json
├── pnpm-lock.yaml       # pnpm install 后生成
├── vite.config.ts
└── .gitignore
```

### 8.4 安装依赖

```powershell
pnpm install
```

### 8.5 修改项目标识

编辑 `src-tauri/tauri.conf.json`，把产品名改为 KanFanOS（`identifier` 已在脚手架命令里指定）：

```json
{
  "productName": "KanFanOS",
  "identifier": "com.kanfanos.app"
}
```

顺便确认 `src-tauri/Cargo.toml` 与 `package.json` 的 name 均为 `kanfanos`（脚手架已生成，通常无需改动）：

```toml
# src-tauri/Cargo.toml
[package]
name = "kanfanos"
```

### 8.6 首次运行

```powershell
pnpm tauri dev
```

**预期**：前 5～15 分钟没有任何输出（正在编译 500+ 个 crate），然后弹出应用窗口，显示 Tauri 默认页面。

> 首次编译时间长是正常的，之后增量编译只需几秒到几十秒。
> 窗口出现即代表第 1 步验收的第一条「应用可启动」通过。

---

## 9. 加入 SQLite 依赖（第 1 步剩余工作）

Tauri 模板默认不带数据库。用 `rusqlite` 的 `bundled` 特性，**无需单独安装 SQLite**，C 代码会随 crate 一起编译（依赖第 1 步的 MSVC）。

用 `cargo add` 安装，避免手写版本号过期：

```powershell
cd src-tauri
cargo add rusqlite --features bundled
cargo add serde --features derive
cargo add serde_json anyhow thiserror
cargo add tokio --features full
```

`Cargo.toml` 的 `[dependencies]` 大致会变成：

```toml
rusqlite = { version = "0.3x", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
thiserror = "2"
tokio = { version = "1", features = ["full"] }
```

然后：

```powershell
cargo build        # 单独编译 Rust 侧，比 tauri dev 更快看到错误
cd ..
pnpm tauri dev
```

> 版本号在项目启动时统一锁定，之后由 `Cargo.lock` 管理。
> 加完依赖后，把 `Cargo.lock` 和 `pnpm-lock.yaml` 一起提交到 git。

---

## 10. 常见问题排查

| 现象 | 原因 | 处理 |
|---|---|---|
| `rustc : 无法将"rustc"项识别为 cmdlet…` | 终端是安装前打开的 | 关闭所有终端重新打开 |
| `error: linker 'link.exe' not found` | 缺少 MSVC C++ 工作负载 | VS Installer 勾选「使用 C++ 的桌面开发」 |
| `error MSB8020: 找不到 v143 生成工具` | 只装了 IDE，没装工具集 | 同上，安装 C++ 工具集与 Windows SDK |
| `failed to get … response from crates.io` | 网络问题 | 配置第 6.2 节的 rsproxy 镜像 |
| 首次编译卡在 `Compiling …` 很久 | 正常现象 | 等待，期间不要重复执行命令 |
| 端口 1420 被占用 | 上次 dev 进程未退出 | 任务管理器结束 `node.exe` / `cargo.exe`，或重启终端 |
| `pnpm : 无法加载文件 … 禁止运行脚本` | PowerShell 执行策略 | `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned` |
| 窗口空白 | 前端 dev server 未启动 | 看终端是否有 Vite 报错，检查 1420 端口 |
| `ffprobe` 找不到 | PATH 未刷新 | 重开终端；或重启系统 |

---

## 11. 完成清单

全部勾选即代表环境就绪：

- [ ] `rustc` / `cargo` 可用，工具链为 MSVC
- [ ] `pnpm` 可用
- [ ] `ffmpeg` / `ffprobe` 可用
- [ ] `mpv` 可用（可延后）
- [ ] 长路径已开启
- [ ] `pnpm tauri dev` 能弹出窗口
- [ ] `rusqlite` 能编译通过（`cargo build` 无错）
- [ ] `Cargo.lock`、`pnpm-lock.yaml` 已提交

---

## 12. 建议的下一步

环境就绪后，按设计文档第 1 步的验收标准推进：

1. 建立 `src-tauri/src/database/`，实现 SQLite 连接（WAL、外键、busy_timeout）
2. 写第一个迁移文件（`videos` 表），启动时执行
3. 暴露一个 `get_video_count` IPC 命令，前端调用并显示
4. 搭建导航栏骨架（媒体库 / 收藏 / 最近观看 / 设置）

此时第 1 步验收项「应用可启动，数据库可以正常读写」即完成。

---

## 附录 A：本机安装结果（2026-10-20）

| 组件 | 实际版本 | 位置 |
|---|---|---|
| Node.js | 24.18.0 | 系统安装 |
| pnpm | 12.14.0 | `%APPDATA%\npm` |
| Rust / Cargo | 1.99.1 | `C:\Users\xzc54\.cargo\bin` |
| 默认工具链 | `stable-x86_64-pc-windows-msvc` | — |
| FFmpeg / FFprobe | 9.0.2 | `%LOCALAPPDATA%\Microsoft\WinGet\Packages\Gyan.FFmpeg_...\ffmpeg-9.0.2-full_build\bin` |
| MPV | 0.41.0 | `C:\Program Files\MPV Player` |
| WebView2 | 155.0.4283.62 | 系统已装 |
| Tauri CLI | 2.12.3 | 项目 devDependencies |
| tauri (Rust) | 2.12.4 | `src-tauri/Cargo.toml` |
| rusqlite | 0.40.2（bundled） | `src-tauri/Cargo.toml` |
| 长路径 | 已启用（`LongPathsEnabled=1`） | — |

性能记录：

- 首次 `cargo build`（约 460 个 crate）：**2 分 47 秒**
- `pnpm tauri dev` 增量编译：**9.21 秒**
- `pnpm install`：**3.9 秒**

> 若日后重装系统，直接按本附录的版本组合还原即可。
