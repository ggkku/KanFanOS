// 收集打包所需的 mpv / FFmpeg / FFprobe / qBittorrent。
//
// mpv / FFmpeg / FFprobe 从本机已安装的版本复制；qBittorrent 从官方
// GitHub Release 下载固定版本并校验 SHA256，然后用 7-Zip 解包。
// 文件已齐全时直接跳过，--force 可重新复制本机工具（qBittorrent 已存在时不重下）。
// 用法：
//   node scripts/fetch-sidecars.mjs
//   node scripts/fetch-sidecars.mjs --force --ffmpeg-dir="C:\Tools\ffmpeg\bin"

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const triple = "x86_64-pc-windows-msvc";
const root = dirname(dirname(fileURLToPath(import.meta.url)));
const dest = join(root, "src-tauri", "binaries");

// qBittorrent 官方 Windows 包只有安装程序；校验值来自 GitHub Release 的 asset digest。
const QBT_VERSION = "5.3.0";
const QBT_SHA256 =
  "aff5b3bdc00d6e5773d6099ba30f9696dd3dfd1669ab0f64ce5c4e7168486436";
const qbtTarget = `qbittorrent-${triple}.exe`;

const args = process.argv.slice(2);
const force = args.includes("--force");
const ffmpegDirArg = args.find((arg) => arg.startsWith("--ffmpeg-dir="));
const ffmpegDir = ffmpegDirArg?.slice("--ffmpeg-dir=".length);

const outputs = [
  `mpv-${triple}.exe`,
  `ffmpeg-${triple}.exe`,
  `ffprobe-${triple}.exe`,
  "d3dcompiler_43.dll",
  qbtTarget,
];

mkdirSync(dest, { recursive: true });

await ensureQbittorrent();

if (!force && outputs.every((name) => existsSync(join(dest, name)))) {
  console.log(`sidecar 已齐全：${dest}`);
  process.exit(0);
}

function findInPath(name) {
  const extensions = process.platform === "win32" ? [".exe", ".com", ".bat", ".cmd", ""] : [""];
  const entries = (process.env.PATH ?? "")
    .split(delimiter)
    .map((entry) => entry.replace(/^"(.*)"$/, "$1"))
    .filter(Boolean);

  for (const entry of entries) {
    for (const extension of extensions) {
      const candidate = join(entry, name + extension);
      if (existsSync(candidate)) {
        return candidate;
      }
    }
  }
  return null;
}

function copyTool(source, target) {
  if (!existsSync(source)) {
    throw new Error(`找不到 ${source}`);
  }
  copyFileSync(source, join(dest, target));
  console.log(`已复制 ${target}`);
}

// 下载官方 qBittorrent 安装包，校验 SHA256 后用 7-Zip 解出 qbittorrent.exe。
async function ensureQbittorrent() {
  const target = join(dest, qbtTarget);
  if (existsSync(target)) {
    return;
  }

  const installerName = `qbittorrent_${QBT_VERSION}_x64_setup.exe`;
  const url = `https://github.com/qbittorrent/qBittorrent/releases/download/release-${QBT_VERSION}/${installerName}`;
  const work = mkdtempSync(join(tmpdir(), "kanfanos-qbt-"));

  try {
    console.log(`下载 qBittorrent ${QBT_VERSION}…`);
    const response = await fetch(url);
    if (!response.ok) {
      throw new Error(`下载 qBittorrent 失败：HTTP ${response.status}`);
    }

    const buffer = Buffer.from(await response.arrayBuffer());
    const digest = createHash("sha256").update(buffer).digest("hex");
    if (digest !== QBT_SHA256) {
      throw new Error(`qBittorrent 校验失败：期望 ${QBT_SHA256}，实际 ${digest}`);
    }

    const installer = join(work, installerName);
    writeFileSync(installer, buffer);

    const extract = join(work, "extract");
    mkdirSync(extract);
    execFileSync(findSevenZip(), ["x", installer, `-o${extract}`, "-y"], {
      stdio: "ignore",
    });

    const exe = join(extract, "qbittorrent.exe");
    if (!existsSync(exe)) {
      throw new Error("安装包里没有找到 qbittorrent.exe");
    }
    copyFileSync(exe, target);
    console.log(
      `已解包 ${qbtTarget}（GPLv3+，许可证见 src-tauri/resources/qbittorrent-COPYING.txt）`,
    );
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

function findSevenZip() {
  const candidates = [
    join(process.env.ProgramFiles ?? "C:\\Program Files", "7-Zip", "7z.exe"),
    join(
      process.env["ProgramFiles(x86)"] ?? "C:\\Program Files (x86)",
      "7-Zip",
      "7z.exe",
    ),
    findInPath("7z"),
  ].filter(Boolean);

  const found = candidates.find((candidate) => existsSync(candidate));
  if (!found) {
    throw new Error(
      "解包 qBittorrent 需要 7-Zip：winget install --id 7zip.7zip -e",
    );
  }
  return found;
}

// mpv：优先使用默认安装目录，其次回退到 PATH。
const programFiles = process.env.ProgramFiles ?? "C:\\Program Files";
const mpvExe =
  [join(programFiles, "MPV Player", "mpv.exe"), findInPath("mpv")].find(
    (candidate) => candidate && existsSync(candidate),
  ) ?? null;

if (!mpvExe) {
  throw new Error("找不到 mpv。请先安装：winget install --id shinchiro.mpv -e");
}

const mpvDll = join(dirname(mpvExe), "d3dcompiler_43.dll");
if (!existsSync(mpvDll)) {
  throw new Error(`找不到 d3dcompiler_43.dll（应位于 ${dirname(mpvExe)}）`);
}

copyTool(mpvExe, `mpv-${triple}.exe`);
copyTool(mpvDll, "d3dcompiler_43.dll");

// FFmpeg / FFprobe：默认取 PATH 中的版本，也可用 --ffmpeg-dir 指定 bin 目录。
const ffmpegExe = ffmpegDir ? join(ffmpegDir, "ffmpeg.exe") : findInPath("ffmpeg");
const ffprobeExe = ffmpegDir ? join(ffmpegDir, "ffprobe.exe") : findInPath("ffprobe");

if (!ffmpegExe || !ffprobeExe || !existsSync(ffmpegExe) || !existsSync(ffprobeExe)) {
  throw new Error("找不到 ffmpeg / ffprobe。请先安装：winget install --id Gyan.FFmpeg -e");
}

copyTool(ffmpegExe, `ffmpeg-${triple}.exe`);
copyTool(ffprobeExe, `ffprobe-${triple}.exe`);

console.log(`完成：${dest}`);
