#!/usr/bin/env python3
"""从 dmhy 标题池里挑选「番剧名不重复、标题含多段数字」的回归样本。

输入是 scrape_dmhy.py 产出的 JSON；输出一份标题列表（每行一条）和一份带
分类/候选名的 JSON。去重是启发式的，繁体/简体或不同字幕组的不同译名
仍可能漏判，最终清单建议人工过一遍。
"""

import argparse
import json
import re
import unicodedata
from collections import defaultdict
from pathlib import Path

GENERIC = {
    "第一季", "第二季", "第三季", "第四季",
    "第1季", "第2季", "第3季",
    "月新番", "10月新番", "新番", "season", "season1", "season2",
    "part", "part1", "part2", "cour", "cour1", "cour2", "合集", "剧场版",
}

MARKERS = [
    r"\s+-\s+0*\d{1,4}(?:v\d+)?",
    r"\bS\d{1,2}\s*[._-]?\s*E\s*0*\d{1,4}",
    r"\bS\d{1,2}\b",
    r"第\s*0*\d{1,4}\s*[话話集回页頁]",
    r"\[\s*0*\d{1,4}(?:v\d+)?\s*\]",
    r"[★❘|｜]\s*0*\d{1,4}(?:v\d+)?\s*[★❘|｜]",
    r"\s+0*\d{1,4}(?:v\d+)?\s*(?:\[|\(|\||$)",
]
MARKER_RE = re.compile("|".join(MARKERS), re.I)


def strip_first_tag(value: str) -> str:
    return re.sub(r"^\s*(?:\[[^\]]*\]|【[^】]*】|★[^*]*★)\s*", "", value, count=1)


def strip_leading_tags(value: str) -> str:
    while True:
        stripped = strip_first_tag(value)
        if stripped == value:
            return value.strip()
        value = stripped


def split_name(title: str) -> str:
    rest = strip_leading_tags(title)
    found = MARKER_RE.search(rest)
    name = rest[: found.start()] if found else re.split(r"[\[【]", rest, 1)[0]
    return name.strip(" -/|❘★")


def norm(value: str) -> str:
    value = unicodedata.normalize("NFKC", value).lower()
    return "".join(ch for ch in value if ch.isalnum())


def name_candidates(name: str) -> set[str]:
    """把双语名拆成英文/中文候选，用于判断两条标题是不是同一部番。"""
    parts = re.split(r"\s+/\s+|\s*\|\s*|❘|★", name)
    out: set[str] = set()
    for part in parts:
        part = re.sub(r"\[[^\]]*\]|【[^】]*】", " ", part)
        part = re.sub(r"(?:season|part|cour)\s*\d+", " ", part, flags=re.I)
        part = re.sub(r"\d+(?:st|nd|rd|th)\s*season", " ", part, flags=re.I)
        part = re.sub(r"第\s*\d+\s*[季期部]", " ", part)
        part = re.sub(r"\b\d+\s*[季期部]", " ", part)
        part = re.sub(r"\bS\d{1,2}\b", " ", part, flags=re.I)
        for match in re.finditer(r"[A-Za-z][A-Za-z0-9:!?' .&½-]{2,}", part):
            candidate = norm(match.group())
            if len(candidate) >= 4:
                out.add(candidate)
        for match in re.finditer(
            r"[一-鿿][一-鿿0-9：·！？ー…―\-]{1,}", part
        ):
            candidate = norm(match.group())
            if len(candidate) >= 3:
                out.add(candidate)
    return {item for item in out if item not in GENERIC and not item.isdigit()}


def conflicts(
    candidates: set[str], prefix: str, chosen: set[str]
) -> bool:
    if prefix in chosen:
        return True
    for candidate in candidates:
        for existing in chosen:
            if candidate == existing:
                return True
            if (
                len(candidate) >= 6
                and len(existing) >= 6
                and (candidate in existing or existing in candidate)
            ):
                return True
    return False


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=Path("dmhy-selected.txt"))
    parser.add_argument("--output-json", type=Path, default=None)
    parser.add_argument("--target", type=int, default=100)
    parser.add_argument("--categories", default="2", help="逗号分隔，默认 2=動畫")
    args = parser.parse_args()

    categories = {item.strip() for item in args.categories.split(",") if item.strip()}
    rows = json.loads(args.input.read_text(encoding="utf-8"))

    pool = []
    for row in rows:
        if categories and row.get("category") not in categories:
            continue
        title = row["title"]
        content = strip_leading_tags(title)
        total_numbers = len(re.findall(r"\d+", content))
        if total_numbers < 2:  # 标题必须包含多段数字
            continue
        name = split_name(title)
        name_numbers = len(re.findall(r"\d+", name))
        candidates = name_candidates(name)
        if not candidates:
            continue
        prefix = norm(strip_first_tag(title))[:18]
        if len(prefix) < 6:
            continue
        pool.append(
            {
                **row,
                "name": name,
                "name_numbers": name_numbers,
                "total_numbers": total_numbers,
                "candidates": sorted(candidates),
                "prefix": prefix,
            }
        )

    # 先挑标题里数字最多的样本，更容易覆盖季号/年份/标题数字。
    pool.sort(key=lambda item: (-item["name_numbers"], -item["total_numbers"], item["page"]))
    selected = []
    chosen: set[str] = set()
    for row in pool:
        if conflicts(set(row["candidates"]), row["prefix"], chosen):
            continue
        selected.append(row)
        chosen.update(row["candidates"])
        chosen.add(row["prefix"])
        if len(selected) >= args.target:
            break

    args.output.write_text(
        "\n".join(item["title"] for item in selected), encoding="utf-8"
    )
    if args.output_json:
        args.output_json.write_text(
            json.dumps(selected, ensure_ascii=False, indent=1), encoding="utf-8"
        )
    print(f"selected {len(selected)} from {len(pool)} -> {args.output}")
    print("请人工检查繁体/简体、不同字幕组译名造成的重复后再用于回归测试。")


if __name__ == "__main__":
    main()
