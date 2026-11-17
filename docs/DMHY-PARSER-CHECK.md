# dmhy 标题批量回归测试

用于验证 `episode_names` / `rss::parse_release_title` 在真实发布标题上的表现。
2026-11-17 用本流程回放了 101 部名称不重复的番剧标题，发现并修复了 4 位集数、
`BIG5`、`1920x1080`、多段字幕组标签等问题。

数据量很小，可以随时扩大规模；扩规模时优先保证「番剧名不重复」和「标题包含
多段数字」这两个约束。

## 1. 依赖与准备

- Python 3.11+，只用标准库。
- 能访问 dmhy 的网络。国内一般需要代理；脚本默认 `127.0.0.1:7897`
  （Clash 常见端口），可用 `--proxy` / `--no-proxy` 覆盖。
- 仓库里的脚本：
  - `scripts/parser-check/scrape_dmhy.py`
  - `scripts/parser-check/select_titles.py`

## 2. 抓取标题

```bash
python scripts/parser-check/scrape_dmhy.py \
  --start-page 1 --end-page 30 \
  --output C:/dmhy-check/titles.json
```

脚本行为：

- 请求 `https://dmhy.org/topics/list/page/{n}`，每页间隔 1.2 秒。
- 只取每行 `<td class="title">` 里的 `/topics/view/...` 标题链接文本；
  **不包含**左侧字幕组链接，也**不包含**「约 x 条评论」。
- 同时记录分类 `sort_id`，默认选样只用 `2`（動畫）。
- 单页失败只打印错误、继续下一页，不会丢掉整份语料。

抓取后建议先看总数：

```bash
python -c "import json;print(len(json.load(open('C:/dmhy-check/titles.json'))))"
```

## 3. 选样

```bash
python scripts/parser-check/select_titles.py \
  --input C:/dmhy-check/titles.json \
  --output C:/dmhy-check/selected.txt \
  --output-json C:/dmhy-check/selected.json \
  --target 100
```

脚本做的事：

1. 只保留指定分类（默认 動畫）。
2. 要求标题里至少有 **2 段数字**。
3. 从标题里切出番剧名，提取英文/中文候选名。
4. 用候选名重叠、标题前缀做启发式去重。
5. 优先选择番剧名部分数字更多的样本（更容易覆盖季号/年份/标题数字）。

注意：繁简转换和不同字幕组的译名仍可能漏判重复。脚本输出后会提示人工检查；
2026-11-17 的 101 条样本就是人工过了一遍去重后的结果。

## 4. 批量解析

### 方案 A：临时 ignored 测试（最快）

在 `src-tauri/src/rss.rs` 的 `mod tests` 里临时加入：

```rust
#[test]
#[ignore = "manual dmhy corpus check"]
fn dump_dmhy_corpus() {
    let path = std::env::var("KF_TITLES_FILE").unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    for (index, line) in text.lines().enumerate() {
        let parsed = parse_release_title(line);
        println!(
            "{}\t{}\tgroup={:?}\ttitle={:?}\tep={:?}\tres={:?}\tsrc={:?}\tsub={:?}\tpkg={:?}\tver={:?}\tcodec={:?}\tcontainer={:?}",
            index + 1,
            line,
            parsed.group,
            parsed.title,
            parsed.episode,
            parsed.resolution,
            parsed.source,
            parsed.subtitle,
            parsed.subtitle_packaging,
            parsed.version,
            parsed.codec,
            parsed.container,
        );
    }
}
```

运行：

```bash
KF_TITLES_FILE='C:\dmhy-check\selected.txt' \
cargo test --manifest-path src-tauri/Cargo.toml --lib dump_dmhy_corpus \
  -- --ignored --nocapture > C:/dmhy-check/parsed.tsv
```

用完记得删掉这个测试，避免污染常规测试集。

### 方案 B：独立解析器对比（检查回归）

如果需要比较改动前后的差异，可以复制两份 `episode_names.rs` 到仓库外的小
crate，各自提供 `parse_episode` / `parse_episode_from_filename`，从 stdin
读标题、输出集数。2026-11-17 的做法：

- `git show HEAD:src-tauri/src/episode_names.rs` 作为旧版本。
- 当前工作区文件作为新版本。
- 两边读取同一份标题/文件名列表，逐行 diff。

旧版和新版对比时，把 dmhy 标题、bangumi RSS 标题、真实视频文件名分别跑一遍；
文件名的 API 要用 `parse_episode_from_filename`，标题用 `parse_episode`。

## 5. 校验什么

除了看 `episode`，还要独立核对其它字段：

| 字段 | 独立判断方式 |
|---|---|
| `resolution` | 从标题里找 `\d{3,4}[pPiI]` 或 `1920x1080`，比较归一化后的 `1080p` |
| `container` | 标题里的 `MKV` / `MP4` / `AVI` / `WEBM` / `M2TS` |
| `version` | `(?:^|[^a-z])v(\d+)` 取最大值 |
| `group` | 开头的 `[字幕组]` / `【字幕组】` |
| `title` | 不能含 `1080p`、`WebRip`、`MKV`、字幕组名等元数据 |
| `episode` | 优先按 `SxxEyy`、`第N话/集/回/页`、` - NN`、`Episode NN`、`[NN]`、`NN-` 提取 |

`episode` 是唯一难以完全自动核对字段。区间（`02-03`）、合集（`01-12`）、
特别篇（`第30号杀人事件`）、半集（`15.5`）都需要人工确认。

## 6. 当前结果（2026-11-17）

样本：101 部名称不重复的番剧标题，全部含至少两段数字。

| 字段 | 不一致 |
|---|---|
| 分辨率 | 0 |
| 容器 | 0 |
| 版本号 | 0 |
| 字幕组 | 0 |
| 番剧名 | 0 |
| 集数 | 3 条人工确认，均符合预期 |

3 条集数确认：

1. `Dark Summoner - 02-03` → `2`：区间取第一集。
2. 柯南 `第30号杀人事件` → `None`：特别篇没有集数。
3. `Ultraman Teo - 15.5` → `None`：半集无法用 `i64` 表示。

## 7. 新旧解析器对比

同一批数据用旧版（HEAD 的 `episode_names.rs`）和新版逐行比较：

| 数据 | 行数 | 结果变化 |
|---|---|---|
| dmhy 101 条标题 | 101 | 5 条，全部是改进（4 位集数、`第177页`、`Episode 43`、区间取第一集） |
| bangumi 110 条 RSS 标题 | 110 | 13 条：区间由取最后一个变成取第一个；`[04]`、`第177页`、`★07★` 等改进 |
| 真实视频文件名 50 条 | 50 | 0 条 |

没有发现旧样本被解析成错误集数。需要留意的行为变化：

- **区间/合集改取第一集**：`01-12` 旧版取 `12`，新版取 `1`。对文件名排序更
  合理，但和旧版行为不同。
- **4 位数集数**：`Show 06 - 1234` 这类合成样本旧版取 `6`，新版取 `1234`。
  真实数据里 4 位数只出现在 `[1361]`、`[1546]` 这种明确括号中；如果以后遇到
  4 位数 ID/时间戳，需要再收紧规则。
- **`Show 2 [3]`**：旧版取 `2`，新版取 `3`（括号数字更像集数）；属于歧义样本。

## 8. 已知限制

- 半集/小数集（`15.5`）返回 `None`，数据模型是 `i64`。
- 合集/区间只取第一个数字，无法表示「包含 01–12」。
- 4 位数集数和 4 位数 ID/时间戳仍可能混淆。
- 番剧名去重依赖英文/中文候选名；繁简或不同译名需要人工复核。
- dmhy 抓取依赖代理；代理端口变化时用 `--proxy` 指定。

## 9. 扩充规模建议

1. 增加页数或按分类（`--categories 2,7`）扩大池子；保持 1–2 秒间隔，不要并发轰炸。
2. 把最终样本固化成 `docs/parser-corpus/*.txt`（标题 + 期望集数），在 CI 里跑
   一个普通的 `#[test]`，不再依赖 ignored 测试和人工看输出。
3. 自动生成期望集数时优先使用明确标记（`SxxEyy`、`第N话`、`- NN`），把
   区间、合集、特别篇标成「人工确认」。
4. 去重可以继续引入繁简转换或直接使用 topic 的英文 slug，减少人工过榜。
5. 旧/新对比建议保留成一次性脚本，而不是长期测试，避免版本号/字段变化后维护两套解析器。

抓取和回放数据放在仓库外的 `C:\dmhy-check`，不纳入版本管理。
