import { convertFileSrc } from "@tauri-apps/api/core";

/// 把数据库里的缩略图键转成 asset 协议 URL。
export function thumbnailUrl(
  dataRoot: string | null,
  key: string | null,
): string | null {
  if (!dataRoot || !key) return null;
  return convertFileSrc(`${dataRoot}/thumbnails/${key}`);
}

interface SeriesCoverSource {
  coverPath: string | null;
  coverVersion: number | null;
  coverKey: string | null;
}

/// 优先使用番剧文件夹内的封面，其次使用第一集缩略图。
export function seriesCoverUrl(
  series: SeriesCoverSource,
  dataRoot: string | null,
): string | null {
  if (series.coverPath) {
    const base = convertFileSrc(series.coverPath);
    // 加上修改时间，封面文件替换后可以绕过浏览器缓存。
    return series.coverVersion != null
      ? `${base}?v=${series.coverVersion}`
      : base;
  }
  return thumbnailUrl(dataRoot, series.coverKey);
}
