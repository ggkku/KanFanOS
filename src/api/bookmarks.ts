import { invoke } from "@tauri-apps/api/core";

export interface Bookmark {
  id: number;
  title: string;
  url: string;
  createdAt: number;
}

export function listBookmarks(): Promise<Bookmark[]> {
  return invoke<Bookmark[]>("list_bookmarks");
}

export function addBookmark(
  title: string,
  url: string,
): Promise<Bookmark[]> {
  return invoke<Bookmark[]>("add_bookmark", { title, url });
}

export function updateBookmark(
  id: number,
  title: string,
  url: string,
): Promise<Bookmark[]> {
  return invoke<Bookmark[]>("update_bookmark", { id, title, url });
}

export function removeBookmark(id: number): Promise<Bookmark[]> {
  return invoke<Bookmark[]>("remove_bookmark", { id });
}
