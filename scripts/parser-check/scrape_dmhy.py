#!/usr/bin/env python3
"""抓取 dmhy 列表页的「标题」列，供集数解析回归测试使用。

只取每行 `<td class="title">` 里的 topic 链接文本：
- 不含左侧字幕组链接
- 不含「约 x 条评论」
- 同时记录分类，默认只保留「動畫」（sort_id=2）

dmhy 在国内通常需要代理。默认使用本机 127.0.0.1:7897（Clash 常见端口），
可通过 --proxy 指定；--no-proxy 表示直连。
"""

import argparse
import html
import json
import re
import time
import urllib.request
from pathlib import Path

DEFAULT_PROXY = "http://127.0.0.1:7897"
USER_AGENT = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) KanFanOS-parser-check/1.0"
LIST_URL = "https://dmhy.org/topics/list/page/{page}"


def build_opener(proxy: str | None) -> urllib.request.OpenerDirector:
    handlers = []
    if proxy:
        handlers.append(urllib.request.ProxyHandler({"http": proxy, "https": proxy}))
    opener = urllib.request.build_opener(*handlers)
    opener.addheaders = [("User-Agent", USER_AGENT)]
    return opener


def extract_rows(page_html: str, page: int) -> list[dict]:
    rows = []
    for row in re.findall(r"<tr[^>]*>.*?</tr>", page_html, re.S):
        category = re.search(
            r'class="sort-(\d+)"[^>]*>\s*(?:<font[^>]*>)?([^<]*)', row
        )
        cell = re.search(r'<td class="title">(.*?)</td>', row, re.S)
        if not cell:
            continue
        # 标题列里可能有多个 <a>：先找字幕组链接，再找 /topics/view/ 标题链接。
        title_link = re.search(
            r'<a\s+href="/topics/view/[^"]+"[^>]*>(.*?)</a>', cell.group(1), re.S
        )
        if not title_link:
            continue
        text = re.sub(r"<[^>]+>", "", title_link.group(1))
        text = re.sub(r"\s+", " ", html.unescape(text)).strip()
        if text:
            rows.append(
                {
                    "page": page,
                    "category": category.group(1) if category else "",
                    "title": text,
                }
            )
    return rows


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--start-page", type=int, default=1)
    parser.add_argument("--end-page", type=int, default=30)
    parser.add_argument("--delay", type=float, default=1.2, help="每页间隔秒数")
    parser.add_argument("--proxy", default=DEFAULT_PROXY)
    parser.add_argument("--no-proxy", action="store_true")
    parser.add_argument("--output", type=Path, default=Path("dmhy-titles.json"))
    args = parser.parse_args()

    proxy = None if args.no_proxy else args.proxy
    opener = build_opener(proxy)
    all_rows: list[dict] = []
    for page in range(args.start_page, args.end_page + 1):
        url = LIST_URL.format(page=page)
        try:
            body = opener.open(url, timeout=30).read().decode("utf-8", "replace")
        except Exception as error:  # noqa: BLE001 - 单页失败不应丢掉整个语料
            print(f"page {page}: ERROR {error}", flush=True)
            continue
        rows = extract_rows(body, page)
        all_rows.extend(rows)
        print(f"page {page}: {len(rows)} titles", flush=True)
        time.sleep(args.delay)

    args.output.write_text(
        json.dumps(all_rows, ensure_ascii=False, indent=1), encoding="utf-8"
    )
    print(f"total {len(all_rows)} -> {args.output}")


if __name__ == "__main__":
    main()
