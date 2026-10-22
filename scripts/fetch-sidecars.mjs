// 收集打包所需的 mpv / FFmpeg / FFprobe。
//
// 默认从本机已安装的版本复制；文件已齐全时直接跳过，--force 可重新复制。
// 用法：
//   node scripts/fetch-sidecars.mjs
//   node scripts/fetch-sidecars.mjs --force --ffmpeg-dir="C:\Tools\ffmpeg\bin"

import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { delimiter, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const triple = "x86_64-pc-windows-msvc";
const root = dirname(dirname(fileURLToPath(import.meta.url)));
const dest = join(root, "src-tauri", "binaries");

const args = process.argv.slice(2);
const force = args.includes("--force");
const ffmpegDirArg = args.find((arg) => arg.startsWith("--ffmpeg-dir="));
const ffmpegDir = ffmpegDirArg?.slice("--ffmpeg-dir=".length);

const outputs = [
  `mpv-${triple}.exe`,
  `ffmpeg-${triple}.exe`,
  `ffprobe-${triple}.exe`,
  "d3dcompiler_43.dll",
];

mkdirSync(dest, { recursive: true });

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
