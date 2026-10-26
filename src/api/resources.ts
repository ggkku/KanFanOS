import { invoke } from "@tauri-apps/api/core";

/** 内嵌浏览器占位区域，CSS 像素，相对主窗口视口。 */
export interface BrowserRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** 新建标签页，返回后端分配的标签 id。 */
export function browserOpenTab(
  url: string,
  rect: BrowserRect,
): Promise<number> {
  return invoke<number>("browser_open_tab", { url, rect });
}

/** 显示标签页并更新位置；返回 false 表示 webview 已不存在。 */
export function browserShowTab(
  tabId: number,
  rect: BrowserRect,
): Promise<boolean> {
  return invoke<boolean>("browser_show_tab", { tabId, rect });
}

/** 隐藏标签页但保留页面状态。 */
export function browserHideTab(tabId: number): Promise<void> {
  return invoke<void>("browser_hide_tab", { tabId });
}

export function browserSetTabBounds(
  tabId: number,
  rect: BrowserRect,
): Promise<void> {
  return invoke<void>("browser_set_tab_bounds", { tabId, rect });
}

export function browserNavigateTab(
  tabId: number,
  url: string,
): Promise<void> {
  return invoke<void>("browser_navigate_tab", { tabId, url });
}

export function browserGoBack(tabId: number): Promise<void> {
  return invoke<void>("browser_go_back", { tabId });
}

export function browserReloadTab(tabId: number): Promise<void> {
  return invoke<void>("browser_reload_tab", { tabId });
}

export function browserGetZoom(): Promise<number> {
  return invoke<number>("browser_get_zoom");
}

export function browserSetZoom(percent: number): Promise<number> {
  return invoke<number>("browser_set_zoom", { percent });
}

export function browserCloseTab(tabId: number): Promise<void> {
  return invoke<void>("browser_close_tab", { tabId });
}

export interface BrowserTabInfo {
  tabId: number;
  url: string;
}

/** 后端实际存在的标签页 webview（用于核对孤儿窗口）。 */
export function browserListTabs(): Promise<BrowserTabInfo[]> {
  return invoke<BrowserTabInfo[]>("browser_list_tabs");
}

/** 关闭不在 keep 里的标签页 webview。 */
export function browserCloseUntracked(keep: number[]): Promise<void> {
  return invoke<void>("browser_close_untracked", { keep });
}

/** 标签页标题或 URL 变化事件。 */
export interface BrowserTabUpdate {
  tabId: number;
  title: string | null;
  url: string;
}

/** 页面请求打开新标签页（target=_blank / window.open）。 */
export interface BrowserNewTab {
  url: string;
}

export type QbittorrentMode = "sidecar" | "external";

export interface QbittorrentConfig {
  mode: QbittorrentMode;
  url: string;
  username: string;
  password: string;
  savePath: string | null;
  advanced: boolean;
}

export interface QbittorrentPreferences {
  savePath: string;
}

/** 内嵌浏览器添加种子后由后端发出的事件。 */
export interface TorrentAddedEvent {
  ok: boolean;
  kind: "magnet" | "torrent";
  url: string;
  error: string | null;
}

export interface QbittorrentStatus {
  version: string;
}

export interface TorrentInfo {
  hash: string;
  name: string;
  size: number;
  progress: number;
  dlspeed: number;
  upspeed: number;
  state: string;
  eta: number;
  numSeeds: number;
  numLeechs: number;
  category: string;
  addedOn: number;
  savePath: string;
}

export function getQbittorrentConfig(): Promise<QbittorrentConfig> {
  return invoke<QbittorrentConfig>("get_qbittorrent_config");
}

export function setQbittorrentConfig(
  mode: QbittorrentMode,
  url: string,
  username: string,
  password: string,
): Promise<void> {
  return invoke<void>("set_qbittorrent_config", {
    mode,
    url,
    username,
    password,
  });
}

export function setQbittorrentAdvanced(
  advanced: boolean,
): Promise<QbittorrentConfig> {
  return invoke<QbittorrentConfig>("set_qbittorrent_advanced", {
    advanced,
  });
}

export function getQbittorrentPreferences(): Promise<QbittorrentPreferences> {
  return invoke<QbittorrentPreferences>("get_qbittorrent_preferences");
}

export function setQbittorrentSavePath(
  path: string,
): Promise<QbittorrentPreferences> {
  return invoke<QbittorrentPreferences>("set_qbittorrent_save_path", { path });
}

export function qbittorrentTest(): Promise<QbittorrentStatus> {
  return invoke<QbittorrentStatus>("qbittorrent_test");
}

export function qbittorrentWebUiUrl(): Promise<string> {
  return invoke<string>("qbittorrent_webui_url");
}

/** 启动或停止一个任务。 */
export function setTorrentRunning(
  hash: string,
  running: boolean,
): Promise<void> {
  return invoke<void>("qbittorrent_set_torrent_running", { hash, running });
}

/** 当前下载目录及其下的全部任务。 */
export interface TorrentListing {
  savePath: string;
  torrents: TorrentInfo[];
}

export function qbittorrentListTorrents(): Promise<TorrentListing> {
  return invoke<TorrentListing>("qbittorrent_list_torrents");
}
