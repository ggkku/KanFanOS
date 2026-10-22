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
}

/// 番剧封面只使用番剧文件夹内配置的封面图片；
/// 不再自动拿第一集缩略图顶替，没有封面时由卡片显示番剧名首字占位。
export function seriesCoverUrl(series: SeriesCoverSource): string | null {
  if (!series.coverPath) return null;

  const base = convertFileSrc(series.coverPath);
  // 加上修改时间，封面文件替换后可以绕过浏览器缓存。
  return series.coverVersion != null
    ? `${base}?v=${series.coverVersion}`
    : base;
}
