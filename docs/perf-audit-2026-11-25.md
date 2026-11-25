# Clippi 性能审计（2026-11-25）

审计基线：`wait` @ `896df8a`（v0.5.9 之后）
审计方式：源码静态分析 + 在本机真实库副本/合成库上用**项目原句 SQL、原 schema 与索引**复现并计时。仓库代码零改动。
与既有文档的关系：`docs/list-render-perf-report.md` 证明「列表渲染与条目数解耦」，本文不重复渲染矩阵，只补它没覆盖的**刷新/搜索/捕获链路**与**每帧渲染工作**。

---

## 0. 结论摘要

1. **最贵的不是列表渲染，而是每次刷新都要付的全表聚合。** `reload_items()` 里有一次 `count_clipboard_items()`，它算的值和同函数随后调用的、**已经按 `change_revision` 缓存过的**标题栏统计完全一样。2 万条历史下这一条 SQL 实测 53 ms；5 千条 17 ms；且每次按键、每次唤出、每次复制都会跑。
2. **每次复制额外付两次全表聚合**：标题栏统计被写入击穿缓存（2 万条 59 ms），`max_items > 0` 时上限裁剪再做一次全表 `COUNT`（2 万条 60–110 ms）。默认 `max_items = 0` 时裁剪不跑，但历史因此无上限增长，最终必然踩到前两条。
3. **列表查询的瓶颈是 `rich_data` 的重复 JSON 解析，不是行数。** 真实库 200 行的列表页要 17–29 ms；而 `json_valid` + 十余次 `json_extract`（html 一个表达式自身就要 4–5 次）把同一条 `rich_data` 解析了 10 余遍（本机库 `rich_data` 共 2.6 MB，其中 html 2.55 MB、单条最大 1.88 MB，而列表只需要前 4 KB）。只取 id 的同一查询是 0.05 ms。
4. **搜索/快速窗口的成本由 `max_items` 直接放大。** 关键字扫描上限 = `query_limit × 8`（夹在 1000–10000），每字符最多读 1 万条整行；真实载荷下 1600 行已实测 **112 ms**。快速窗口不走分页、直接按 `query_limit` 取行，且逐字符更新**没有防抖**。
5. **每帧渲染里有可观的重复工作**（每卡每帧 2–4 次 serde 解析、5 次 String clone、字体整形、`stat`/`open`）。它目前被 160 Hz 的 vsync 预算掩盖（既有帧追踪四档都是 5.81–5.88 ms），属于 CPU 浪费与低端机风险，而不是当前已观测到的掉帧。

按「修完之后用户能感觉到」排序：**第 2 节的三个聚合 > 第 3 节的列表载荷 > 第 4 节的搜索与快速窗口 > 第 8 节的 cache_size/pragma > 第 6 节的每帧派生**。第 2、5、7、8 节的主要项已经修复，状态与未做项见第 11 节。

---

## 1. 方法、数据与可信度

| 项 | 值 |
|---|---|
| 机器 | Windows 11，Ryzen 5 3600（6C/12T），32 GB，NVMe，3840×2160 @160 Hz |
| 真实库 | 用户 dev 库副本：272 条 / 10.8 MB / `rich_data` 2.6 MB（`html` 2.55 MB，平均 17 KB，最大 1.88 MB；`html > 4096` 的 9 条）；24 个来源应用；272 行中 151 行的 `rich_data` 是合法 JSON |
| 合成库 | 同 schema/索引：5 000 条（27 MB）、20 000 条（108 MB），随机英文词、20 个来源应用、约 10% 富文本 |
| 「真实载荷 ×20」库 | 真实库每行复制 20 份（重算 hash）：5 440 条 / 88 MB，用于在真实字段分布下测扫描成本 |
| SQLite | Python 3.49.1；项目 bundled 为 3.46。同 schema、同索引、同 SQL，规划器结论一致，绝对毫秒有出入 |
| 应用侧佐证 | 应用自身日志：`wm reload after show ms=33 items=200`（272 条库，与本文 17–29 ms 查询 + 行转换 + 列表重建吻合） |

可信度约定：

- **实测**：在上述库上运行项目原句 SQL 得到的中位耗时（每项预热一次，重复 5–20 次）。
- **估算**：从 5k/20k 线性外推到 100k/1M，或把合成载荷换算成真实载荷。标了「估算」的数字不要当验收基线。
- 每帧类结论来自**调用链可证**（render 内构造卡片、每卡 `from_json` 等），量级未用 profiler 定量；既有帧追踪报告显示当前机器没有掉帧。

> 复现脚本只在 `%TEMP%\clippi-perf\`（含真实库副本，审计结束已删除），不进仓库；关键 SQL 见附录 A。

---

## 2. 每次刷新/捕获的固定开销（最高优先级）

### 2.1 `reload_items()` 到底做了什么

`src/state/app.rs:762` 是「唤出窗口、每次防抖按键、每次可见捕获、每次删除/编辑、每次后台清理」的统一入口，按顺序：

| # | 步骤 | 代码 | 272 条 | 5k 条 | 20k 条 |
|---|---|---|---|---|---|
| 1 | `refresh_source_apps()` → 来源名单/图标聚合 | `app.rs:769` → `db.rs:532` | 1.9–3.6 ms | ~0.5 ms | ~2–3 ms |
| 2 | 当前页查询（200 行 + 标签） | `app.rs:778` → `db.rs:1105` | **17–29 ms** | 3–5 ms | 3–5 ms |
| 3 | `count_clipboard_items()` | `app.rs:816` → `db.rs:816` | 0.08 ms | **17 ms** | **53 ms** |
| 4 | `refresh_titlebar_filter_availability()` | `app.rs:730` → `db.rs:783` | 0.10 ms | **17 ms** | **56–59 ms**（每次写后重算） |
| 5 | 快速窗口页（仅其可见时） | `app.rs:830` | — | — | — |
| 6 | 事件 → `visible_items()` 深拷贝 + `set_items()` 几何重建 | `root.rs:276`、`app.rs:4146`、`clipboard_list.rs:707` | +2–5 ms | +2–5 ms | +2–5 ms |

第 3、4 步在同一函数里对**整张表**各扫一遍，而第 4 步的结果里已经含有第 3 步要的那个数。

### 2.2 `count_clipboard_items()` 是纯重复计算【实测】

`src/state/app.rs:816` 每轮无条件执行；`src/core/db.rs:816`：

```rust
"SELECT COUNT(*) FROM clipboard_items WHERE meta_type != 'transfer'"
```

同一函数 5 行之后（`app.rs:821`）调的 `load_titlebar_stats()`（`db.rs:783`）算的第三个值：

```rust
COUNT(CASE WHEN meta_type != 'transfer' THEN 1 END) FROM clipboard_items
```

两者**语义完全相同**（`main.rs:256` 也是拿它当 `history_rows` 用的）。区别只是后者按 `change_revision()`（`db.rs:524`）缓存，前者完全没有缓存。

实测（2 万条、`cache_size=-2000`）：**53 ms/次**（重复 9 次中位；区间 53–133 ms，取决于 VFS 缓存状态）。5 千条 17 ms。按键路径 150 ms 防抖，即最多每秒 6 次 → 20k 库约 320 ms CPU/s，从「打字」变成「拖着打字」。

**修法（一行，零行为变化）**：删掉这次查询，`refresh_titlebar_filter_availability` 里顺手赋值：

```rust
// app.rs:730
fn refresh_titlebar_filter_availability(&mut self) {
    match self.db.load_titlebar_stats() {
        Ok(stats) => {
            self.has_hotkey_items = stats.has_hotkey_items;
            self.has_favorite_items = stats.has_favorite_items;
            self.clearable_history_count = stats.clearable_history_count;
            self.total_item_count = stats.clearable_history_count; // 同一个数
        }
        ...
    }
}
```

若坚持保留独立查询，则加部分索引 `CREATE INDEX idx_non_transfer ON clipboard_items(id) WHERE meta_type != 'transfer'`，实测 **53 ms → 0.44 ms**（仍应优先做上面的复用）。

### 2.3 标题栏统计被每次写入击穿【实测】

`db.rs:783` 的缓存键是 `change_revision() = (total_changes, data_version)`。**任何写入都会变**（捕获的 upsert、使用计数触达、标签、清理……），所以每次复制后第一次 `reload_items()` 必然重算这条 `MAX/MAX/COUNT` 全表聚合：

```
2 万条：56–59 ms/次复制     5 千条：17 ms/次复制     （估算 10 万：250–300 ms）
```

`MAX(custom_hotkey <> '')` 这类聚合无法用普通索引加速（要扫全表）；`COUNT(...)` 才是可索引的部分。

**修法**：拆成两个可早停的 `EXISTS` + 一个计数，配两个部分索引。实测（2 万条）**59 ms → 0.48 ms**：

```sql
CREATE INDEX IF NOT EXISTS idx_hotkey ON clipboard_items(id) WHERE custom_hotkey <> '';
CREATE INDEX IF NOT EXISTS idx_fav    ON clipboard_items(id) WHERE is_favorite = 1;
-- 查询：
SELECT EXISTS(SELECT 1 FROM clipboard_items WHERE custom_hotkey <> ''),
       EXISTS(SELECT 1 FROM clipboard_items WHERE is_favorite = 1),
       (SELECT COUNT(*) FROM clipboard_items WHERE meta_type != 'transfer');
```

（计数值即 2.2 的复用；两个 `EXISTS` 走部分索引，命中首行即停。）

### 2.4 上限裁剪在主线程做全表 `COUNT`【实测，既有文档已知仍未修】

`src/services/gpui_clipboard.rs:388` 每次捕获：

```rust
if state.settings.max_items > 0 {
    state.db.prune_items_over_limit(state.settings.max_items)   // 主线程
}
```

`src/core/db.rs:1812` 先 `COUNT(*)` 判超限，谓词 `MAINTENANCE_DELETABLE_PREDICATE`（`db.rs:103`）含 `NOT EXISTS(item_tags)`，查询计划是 `idx_is_favorite` + 每行一次相关子查询（选择性≈0，等于全表）：

```
2 万条：60–110 ms/次复制     5 千条：17 ms/次复制
```

日常配置下结果永远是「没超限、什么都没删」，这 60–110 ms 白付。100 万条时即既有报告实测的 3.5 s 冻结。

**修法（语义等价，不随 n 增长）**：把「先 COUNT 再比较」换成存在性探测：

```sql
SELECT id FROM clipboard_items
WHERE <MAINTENANCE_DELETABLE_PREDICATE>
ORDER BY created_at ASC, id ASC
LIMIT 1 OFFSET ?max_items
```

实测（2 万条）**63 ms → 0.31 ms**（`OFFSET 0` 时 0.01 ms）。更彻底的做法是把裁剪挪到后台线程/空闲期，捕获路径只打脏标记（既有文档 P1 建议，尚未实施）。

### 2.5 三个聚合加起来的效果

| 历史条数 | 每次复制的主线程阻塞（三个聚合 + 列表查询） | 每次唤出 |
|---|---|---|
| 272（本机现状） | 约 20–35 ms（与日志 `reload ms=33` 一致） | 约 33 ms |
| 5 000 | 约 55–75 ms | 约 40 ms |
| 20 000 | **约 120–220 ms** | 约 70–90 ms |
| 100 000（估算） | 约 550–1 100 ms | 约 350–450 ms |

第 2.2–2.4 三项修完，20k 档的「每次复制」可回到 **10–20 ms** 量级；这是本次审计里性价比最高的一组改动（约 15–25 行 + 一个迁移）。

---

## 3. 列表查询的载荷成本

### 3.1 真实开销在 JSON 重复解析【实测】

`src/core/db.rs:201` 的 `list_item_select_columns()` 为每行构造预览 JSON：`json_valid(rich_data)` 一次 + `json_extract(rich_data, '$.html')` 4 次（`length`/`instr`/`substr` 及 `substr` 内层 `instr`）+ 另外 7 个键各一次 ≈ **同一份 `rich_data` 被解析 11–12 遍**，只为产出最长 4 KB 的 html 和各 2 KB 的辅助字段。

本机真实库（200 行/页）逐列拆解：

| 查询 | 中位耗时 |
|---|---|
| 只取 `id` | 0.05 ms |
| `substr(full_text,1,8192)` | 0.74 ms |
| `CASE ... source_app_icon` | 3.86 ms |
| 原样 `rich_data` | 4.90 ms |
| `json_valid(rich_data)` | 4.39 ms |
| 完整列表列（现状） | **17–29 ms** |

`cache_size` 调到 200 MB 后仍是 17–29 ms → **CPU 密集（JSON 解析），不是 I/O**。

### 3.2 每行还带 2.4 KB 的来源图标

`source_app_icon` 平均 2.4 KB/行、24 个应用只有 24 份不同内容，却按行重复下发（3.86 ms/页）。`AppState` 已经有 `source_app_icons: HashMap<name, PathBuf>`（`app.rs:149`）与 `cache_app_icon` 的文件缓存，卡片渲染（`clipboard_card.rs:807`）最终也是拿 `(name, base64)` 去查同一份文件。

**修法（低风险，先做这个）**：列表查询把该列置 `''`（`item_scan_columns()` 已有现成写法），卡片按 `source_app_name` 从状态取路径；`fill_source_app_icons` 只给没有路径的兜底。实测 **列表页 5.09 ms → 3.42 ms**（合成库）；真实库约 −3.9 ms。

### 3.3 已实施：把预览算一次，而不是每次查询算十几次

`list_preview TEXT` + `list_preview_len INTEGER`（v13 迁移）在写入时算好截断后的预览，长度戳是源 payload 的字节长度；列表投影只在 `list_preview_len = length(cast(rich_data AS BLOB))` 时直接读它，否则退回下面的 SQL 表达式。实测本机真实库 200 行页 **31.5 ms → 8.0 ms**（无 `ANALYZE` 条件下 19.6 → 6.1 ms）；旧行不需要回填，新捕获逐步覆盖，写路径漏更新只会退回慢路径而不会显示过期预览。

实现前的两个备选方案（保留在此供参考）：

1. **写入时预计算一列** `list_preview TEXT`（截断后的同一份 JSON，写入/更新 `rich_data` 的路径一起写）。列表查询退化成「读一个小列」，实测等价成本接近 `substr(full_text)`（< 1 ms/页），且 `AppState.items` 内存也会从 2.6 MB/200 条降到 ~1 MB。代价：一次 schema 迁移 + 所有写 `rich_data` 的地方（`upsert`、`update_rich_data`、`update_link_detection`、sync/merge）同步维护。
2. **减少解析次数**：SQL 改成一次 `json_extract(rich_data, '$.html','$.rtf', ...)` 多路径取值，在 Rust 里组装。实测（无效 JSON 被过滤后）8 次单独 extract 10.5 ms → 单次多路径 8.4 ms；收益有限，但改动只有 SQL + `row_to_item`。

不建议「改回 Rust 里 `serde_json` 解析整份 `rich_data`」：2.6 MB/页的 serde 解析量级与现在的 SQL 侧相当，且会把整份富文本搬进 UI 线程。

---

## 4. 搜索与快速窗口

### 4.1 关键字扫描：每字符最多读 1 万条整行【实测】

`src/state/app.rs:29`：

```rust
const KEYWORD_SEARCH_PAGE_SIZE: usize = 128;
const KEYWORD_SEARCH_MAX_SCAN_LIMIT: usize = 10_000;
const KEYWORD_SEARCH_SCAN_MULTIPLIER: usize = 8;
let scan_limit = result_limit * 8 /* clamp 1000..10000 */;
```

`load_keyword_page()`（`app.rs:1238`）每页读 128 行 **整行全字段**（`db.rs:184` 的 `item_scan_columns()` 只把 `source_app_icon` 置空，`full_text`/`rich_data`/`file_data` 都是全量），然后逐行 `RichData::from_json` + `visible_text`/`preview_html` 在 Rust 里匹配——关键字条件**没有下推**到 SQL（`filters.rs:266` 的注释明说）。

实测：

| 场景 | 耗时 |
|---|---|
| 1 600 行扫描（默认 `max_items=0` → `query_limit=200` → scan 1 600） | 合成载荷 17–26 ms；**真实载荷 112 ms** |
| 关键字无命中时按页推进 | 13 条 SQL，每页一条 `LIMIT 128 OFFSET n` |
| `max_items=5000` 时 scan 上限 10 000 → 约 78 条 SQL、约 120 MB 载荷（真实载荷估算） | 估算 **0.5–1.5 s/键** |

这就是 `KEYWORD_SEARCH_SLOW_LOG = 100 ms` 存在的原因；真实载荷下**默认配置已经触线**。

**修法**：
1. 扫描页只取匹配需要的字段：`full_text` 可以 `substr` 到与列表一致的上限，`rich_data` 只取 html 可见文本（或干脆不参与匹配，改为只匹配 `full_text`+`note`+标签，产品上可接受再谈）。
2. 把廉价预筛下推到 SQL：`WHERE instr(lower(full_text), ?) > 0 OR ...`，先缩小候选集再取整行。
3. 让扫描跑在后台线程（项目已有 worker 模式），主线程只收结果。
4. 不要随 `max_items` 放大 scan 上限（`10000` 的夹取对象是 `query_limit`，而 `query_limit = 2 × max_items`）。

### 4.2 `max_items` 直接放大每个查询，且没有上限校验【实测】

`src/state/app.rs:2189`：

```rust
fn query_limit(&self) -> usize {
    if self.settings.max_items == 0 { 200 }
    else { (self.settings.max_items as usize).saturating_mul(2).max(200) }
}
```

而设置页（`src/ui/settings/data.rs:547`）对输入**没有 clamp**：`parse::<u32>().unwrap_or(0)`，用户填 `100000` 就得到 `query_limit = 200000`。

两处直接吃这个值：

- **主列表**：分页从 `LIST_PAGE_STEP = 500` 起翻倍，滚到底才到 `query_limit`（可接受，但有内存峰值）。
- **快速窗口**（`app.rs:937`）：`load_quick_items()` **不做分页**，一次 `LIMIT query_limit` 取全量。`max_items = 5000` → 10 000 行 × 列表列。实测（合成载荷）10 000 行 = **188 ms / 44 MB 字符串**；真实载荷（约 12 KB/行）估算 **~0.5 s / 120 MB**，并且是窗口打开时同步跑在主线程上。

**修法**：给 `max_items` 一个合理上限（如 100 000）并在保存时 clamp；快速窗口改走与主列表同一套 `loaded_limit` 分页（或至少夹到 1000 行）。

### 4.3 快速窗口逐字符刷新，没有防抖【代码可证】

主窗口搜索框有 150 ms trailing-edge 防抖 + generation 令牌（`search_box.rs:86-121`，做对了）。快速窗口没有：键盘钩子每收到一个字符就 `set_query` → `state.set_quick_keyword` → `load_quick_items()` **同步查库**（`quick_paste.rs:585`、`app.rs:965`）。

后果：快速窗口里每敲一个字符 = 一次完整页查询（默认 200 行 ≈ 15–26 ms，真实库）= 键入 10 字符/秒时主线程占用 15%–25%；叠加上面的 `max_items` 放大就成了灾难。

**修法**：复用主窗口同一套 150 ms 防抖；或先只过滤内存快照、防抖后再查库。

---

## 5. 筛选后的排序成本

`db_where()`（`filters.rs:180`）给收藏/标签/来源加了谓词，但主查询恒定 `ORDER BY pinned DESC, {col} DESC`（`db.rs:649`）。这些谓词的索引（`idx_is_favorite`、`idx_source_app`、标签子查询）**都不含排序键**，所以每一页都要把整个筛选集塞进临时 B 树排序：

| 场景（20 000 条） | 耗时 |
|---|---|
| 无筛选（走 `idx_pinned_updated`） | 3–5 ms |
| 来源应用筛选 | 14.7 ms |
| 收藏筛选（10%–33% 命中） | **113 ms** |
| 标签筛选 | 同形（子查询 + TEMP B-TREE） |

筛选越「宽」越慢，因为这等于「把命中的全部行排序后取前 200」。而关键字分页会把同样的排序重复 13–78 次。

**修法**：给筛选 + 排序建复合索引，例如 `(is_favorite, pinned DESC, updated_at DESC)`、`(source_app_name, pinned DESC, updated_at DESC)`；标签路径先从 `item_tags` 取 id 集再按键集走主索引。若不想加索引，至少不要让关键字分页重复付排序（先取候选 id，再一次性取行）。

注：`validate_order_by()`（`db.rs:499`）里允许的 `content_type`/`size`/`image_width`… 目前没有任何 UI 入口（只有 `sort_by_created` 一个开关），属于**潜在**问题，不是当前可触发的缺陷；`(pinned, content_type)` 这类索引等真要开放更多排序项时再加。

---

## 6. 每帧渲染工作（代码可证，当前被 vsync 掩盖）

虚拟列表只渲染可见行（既有报告已证），但**每一帧重建可见卡片**时，`clipboard_card.rs` 的构造代码会重做一批本可缓存的工作：

| 项 | 位置 | 每帧量 |
|---|---|---|
| `RichData::from_json(&item.rich_data)` | `clipboard_card.rs:1664` | 每卡 1 次 |
| `StaleItemCandidate` 5 个 `String` clone | `:1679-1692` | 每卡 5 次分配（含 `full_text` 全量拷贝） |
| `FileData::from_json` | `:1712` | 文件/中转卡每卡 1 次 |
| `rich_preview()`：`from_json` + `normalize` + `visible_text` + 样式解析 | `:2371` → `:1083-1116` | 富文本卡 3–5 遍全文扫描 |
| `favicon_cache_path` / `cache_app_icon` / `image_thumbnail_path` | `:1851`、`:807`、`:816` | 每卡 2–3 次 `stat`，图片卡一次 `open+read` |
| `info_pill_width` → `TextSystem::shape_line` | `:103-110`，调用点 `:2759-2818` | 每卡 2–4 次（tag/时间/尺寸/hotkey），每次都是字体整形 + String 分配 |
| 行内 `item.clone()` 再 `Rc::new` | `clipboard_list.rs:4103`、`:4277` | 每可见行每帧一次深拷贝（`full_text`/`rich_data`/`file_data`） |
| 动画键 `format!("card-lit-{}")` | `:1761`、`:3079` | 每卡每帧 2 次 String 分配 |

量级：既有帧追踪（10 万/100 万条、滚动 10 s + 5 s）四档都是 5.81–5.88 ms、p95 ≤ 8 ms ——**说明这些工作目前没有突破 6.25 ms 的 vsync 预算**，所以它不是「现在就在掉帧」的证据。它的意义是：每帧有大量纯浪费的 CPU/分配，低端机、杀软实时扫描（`stat` 被 hook）或悬停动画连帧时最容易先崩。

**修法（按性价比）**：
1. `set_items()` 时一次性算好派生字段（`has_qr`/`has_links`/`remote_host`/`is_transfer`/图标路径/`RichPreview`），卡片只读字段；`RichPreview` 按 `(id, updated_at)` 做有界缓存。
2. 列表项存 `Rc<ClipboardItem>`，行内 `Rc::clone`；`selected_order` 等等已经用 `HashMap` 的地方保持。
3. `info_pill_width` 加 `(text, size) → width` 有界缓存（同屏 tag 文案高度重复）。
4. 渲染期不碰文件系统：缩略图/图标/favicon 的「已就绪」状态由现有的 `ThumbnailReady`/`FaviconReady` 事件写进卡片模型。
5. 动画键改用 `ElementId::from(("card-lit", item.id))`，去掉 `format!`。

---

## 7. 后台与异步路径

| 项 | 位置 | 问题 | 量级 |
|---|---|---|---|
| `Database::open()` 每次分析任务都跑迁移 + 全表 `UPDATE` | `db.rs:247` → `:427`（`run_db_migrations`）→ `:434`（`prune_oversized_source_icons`） | OCR/QR/链接分析**每个 job** 各开一次连接（`gpui_clipboard.rs:479/509/529`），每抓一个网页标题也开一次（`url_title.rs:21`）；每次 open 都做 `init_schema` + 迁移检查 + `UPDATE ... WHERE length(source_app_icon) > 256 KB`（全表扫描的写事务） | 内存库实测 2 万条 **9.5 ms/job** 是下限；真实磁盘/写锁下更大（估算 10 万 ~50 ms、100 万 ~500 ms），且持写锁 |
| 呼出面板同步等 UIA | `focus_restore.rs:151-196`（`recv_timeout(150ms)`），调用点 `window_manager.rs:3589` | `show_and_focus` 先同步 `capture()` 再显示窗口，UIA 慢时直接推迟首帧 | 0–150 ms，通常几毫秒 |
| 同步整份历史 | `core/sync.rs:333`、`:218`（`to_vec_pretty` 全量） | 开启同步后每次复制置脏，5 s 内可再跑一轮「全表查询 + 全量序列化 + 整文件重写」；历史默认无上限 | opt-in；条数越大越接近「每次复制卡一下」 |
| 同步重压所有图片 | `gpui_sync.rs:832-860` | 先压缩再判断远端是否已有，`sync_include_images=true` 时每轮全量重压 | opt-in，几十张图即几十秒 CPU |
| 缩略图队列满即丢弃 | `platform/clipboard.rs:78-103` | 并发上限 2，满员直接 return；线程 panic 会永久占槽 | 批量粘图时缩略图缺失 |
| QR 每图两趟 | `core/qr.rs:8-27` | 正色一趟 + 反色一趟（各一次全图 `prepare`） | 默认开启；后台 worker |
| favicon 每链接新建 Agent、串行、失败不记 | `favicon.rs:82`、`url_assets.rs:231` | 无连接复用，离线时每链接 3 s 连接超时，下一轮 sync 重试 | 同步期间 `is_running` 长期为真 |

这些不影响「本地复制/唤出」的主路径，但会抢写锁、占 CPU、拖长同步；其中 **`Database::open()` 带写操作**这条影响面最大（任何一个后台 job 都可能让主线程的 upsert 排队），建议拆成 `open_readonly_for_job()`：只校验 schema 版本，不跑迁移、不清超大图标。

---

## 8. 存储与 pragma

`src/core/db.rs:252`：

```rust
conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA cache_size = -2000;")?;
```

| 项 | 现状 | 实测/影响 | 建议 |
|---|---|---|---|
| `cache_size` | 2 MB/连接 | 本文所有全表聚合的最大放大器：2 万条 / 108 MB 库上，2 MB → 53 ms，200 MB → 8 ms（**6.6×**）；5 万条以上差异更明显 | 提到 `-16000`～`-32000`（16–32 MB）。与「隐藏时 trim 工作集」不冲突（页缓存是 SQLite 堆，可被 `sqlite3_release_memory`/重开连接释放） |
| `synchronous` | 未设 = WAL 下 `FULL` | `upsert()` 是 `UPDATE` + 可能 `INSERT` 两条语句 = 两次提交 = 最多两次 fsync（`db.rs:450-476`）；一次复制还有 `touch_items` 一次 | `PRAGMA synchronous = NORMAL`（WAL 官方推荐组合，崩溃只可能丢最后一个事务）；`content_hash` 建 UNIQUE 后改单语句 `ON CONFLICT DO UPDATE` |
| `ANALYZE` | 从未跑过 | **实测：跑过 `ANALYZE` 反而会让 `COUNT(*) WHERE meta_type != 'transfer'` 和裁剪计数退回全表扫描**（20k 库上 0.5 ms → 58–99 ms）。规划器在有 `sqlite_stat1` 时认为宽表扫描更便宜 | **不要**在启动或维护里跑 `ANALYZE`。若将来需要统计信息，先用 `EXPLAIN QUERY PLAN` 回归测试确认第 2 节的聚合仍走索引 |
| 冗余索引 | `idx_item_tags_tag`（PK 前缀）、`idx_del_items_hash`（被 UNIQUE 覆盖）、`idx_uf_items_hash`（同）、`idx_del_tags_name`（被部分 UNIQUE 覆盖） | 每次写入多维护一棵 B 树 | 迁移里 `DROP INDEX` |
| 缺失索引 | 见第 2、5 节 | 全表聚合与筛选排序 | 部分索引（`WHERE` 精确匹配计数/探测）；筛选复合索引 |

---

## 9. 已确认「不是问题」的部分（避免误改）

- **列表渲染与条目数解耦**：虚拟列表只渲染可见行；1 千/1 万/10 万/100 万档帧时间 5.81–5.88 ms（既有报告）。
- **深分页 OFFSET 本身不慢**：`LIMIT 128 OFFSET 1000` 实测 0.5–1.1 ms（只跳索引项）；慢的是「每页取整行 + 筛选后排序」。
- **主列表分页**：首屏 500 行、接近页尾翻倍，启动不再 O(历史)（既有报告 P0 已实施）。
- **标签查询已分批 + `prepare_cached`**（`db.rs:960`，8 000/批，绕开 SQLite 32 766 变量上限）。
- **隐藏窗口捕获不回填列表**：`gpui_clipboard.rs:393` 已按 `windows_visible` 走 `needs_reload` 延迟路径（既有文档 P1 已实施）。
- **搜索防抖存在且位置正确**：`search_box.rs:86-121`，150 ms trailing-edge + generation。
- **滚动/悬停动画没有常驻重绘**：`smooth_scroll.rs` 只在 `glide.running()` 时请求动画帧；面板淡出、flyout、帧追踪都带退出条件。
- **批量写都有正确的事务边界**：`touch_items`、`set_pinned_many`、`delete_items_in_chunks`、保留期清理。
- **列表排序索引方向正确**：`(pinned DESC, updated_at DESC)` / `(pinned DESC, created_at DESC)`（`migration.rs:209`），且迁移里有 EXPLAIN 回归测试。
- **后台 checkpoint 不在 UI 线程**（`db.rs:297`），隐藏时释放内存的设计有效。

---

## 10. 建议的修复顺序

| 优先级 | 改动 | 预期收益 | 风险/规模 |
|---|---|---|---|
| **P0** | ① `total_item_count` 复用 `TitlebarStats.clearable_history_count`；② 标题栏统计拆 `EXISTS`+计数；③ 裁剪改存在性探测 | 20k 历史：每次复制 120–220 ms → **10–20 ms**；每次按键 53 ms → **≈0** | 低；约 20 行 + 2 个部分索引（迁移） |
| **P1** | ④ 列表查询不下发 `source_app_icon`；⑤ 写入时预计算 `list_preview` 列 | 每次唤出 17–29 ms → **1–3 ms**（本机真实库）；`AppState` 每页内存 2.6 MB → ~1 MB | ④ 低；⑤ 中（schema 迁移 + 所有写 `rich_data` 的路径同步） |
| **P2** | ⑥ 快速窗口加防抖 + 分页；⑦ `max_items` 上限校验；⑧ 关键字扫描减列/下推预筛 | 搜索/快窗：112 ms/键 → 十毫秒级；`max_items=5000` 的 0.5 s/字符风险消除 | 低–中；会改动搜索语义（减列需产品确认） |
| **P3** | ⑨ 卡片派生字段在 `set_items` 算一次；`Rc<ClipboardItem>`；字体宽度缓存；渲染期去 I/O | 每帧省 1–3 ms 与大量分配；低端机/悬停动画更稳 | 中；纯重构，需 UI 冒烟 |
| **P4** | ⑩ `cache_size`/`synchronous`/`ANALYZE`/冗余索引；⑪ `Database::open()` 拆只读版本；⑫ 筛选复合索引 | 后台 job 不再抢写锁；全表扫描再快 2–6× | 低–中；⑫ 需按真实筛选组合验证 EXPLAIN |

建议实施顺序即上表顺序；每一档都能独立交付、独立复测。P0 主要解决 5k 以上历史的「复制/按键卡顿」，P1 才解决本机 272 条的唤出成本（列表页 17–29 ms 的大头）；两者都落地后，本机 272 条的 `reload after show` 预计从 33 ms 降到 10 ms 以内，20k 条时从 150 ms+ 降到 20 ms 量级。

---

## 附录 A：关键 SQL（可直接复现）

```sql
-- 1) 未缓存的重复计数（app.rs:816）
SELECT COUNT(*) FROM clipboard_items WHERE meta_type != 'transfer';

-- 2) 已缓存的标题栏统计（db.rs:797）——两者第三列同义
SELECT COALESCE(MAX(custom_hotkey <> ''), 0),
       COALESCE(MAX(is_favorite = 1), 0),
       COUNT(CASE WHEN meta_type != 'transfer' THEN 1 END)
FROM clipboard_items;

-- 3) 上限裁剪的全表 COUNT（db.rs:1812）
SELECT COUNT(*) FROM clipboard_items
WHERE meta_type != 'transfer' AND pinned = 0 AND is_favorite = 0
  AND custom_hotkey = '' AND note = '' 
  AND NOT EXISTS (SELECT 1 FROM item_tags WHERE item_tags.item_id = clipboard_items.id);

-- 4) 列表页（db.rs:201 的 22 列，rich_data 被解析 11–12 遍）
SELECT <list_item_select_columns()> FROM clipboard_items
ORDER BY pinned DESC, updated_at DESC LIMIT 200;

-- 5) 关键字扫描的一页（app.rs:1238，整行全字段）
SELECT <item_scan_columns()> FROM clipboard_items
ORDER BY pinned DESC, updated_at DESC LIMIT 128 OFFSET ?;
```

## 附录 B：实测汇总

| 查询 | 272 条（真实） | 5k（合成） | 20k（合成） | 5.4k（真实载荷 ×20） |
|---|---:|---:|---:|---:|
| 列表页 200 行 | 17–29 ms | 3.1 ms | 3.3–5.1 ms | 1.3–6.1 ms（受排序命中行影响） |
| `count_clipboard_items` | 0.08 ms | 17 ms | 53 ms | 14 ms |
| 标题栏统计 | 0.10 ms | 17 ms | 56–59 ms | 15.5 ms |
| 裁剪全表 COUNT | 0.18 ms | 17 ms | 60–110 ms | 17 ms |
| 来源应用聚合 | 1.9–3.6 ms | 0.5 ms | 2–3 ms | — |
| 关键字扫描 1 600 行 | 18.6 ms（只有 272 行可扫） | 17 ms | 26 ms | **112 ms** |
| 收藏筛选页 | 0.03 ms | 15 ms | 113 ms | 18 ms |
| 来源筛选页 | 0.09 ms | 5.8 ms | 14.7 ms | 0.06 ms（重复行命中同一应用） |

修复后实测（同一「真实载荷」2 万条库，207 MB，`mmap_size=256MB` + v12 索引）：

| 查询 | 修复前 | 修复后 |
|---|---:|---:|
| `count_clipboard_items` | 17–53 ms | **删除**（复用 titlebar 计数，0 ms） |
| 标题栏统计 | 17–59 ms | **0.68 ms** |
| 裁剪全表 COUNT | 47–110 ms | **10.6 ms**（窄索引 + mmap） |
| 收藏筛选页 | 15–113 ms | **3.3 ms**（部分排序索引） |
| 关键字扫描 1 600 行 | 65–112 ms | **18 ms**（mmap） |
| 列表页 200 行 | 13–16 ms | **8.0 ms**（真实库 31.5 → 8.0 ms；写时预览 + 长度戳） |

---

## 附录 C：与既有文档的衔接

- `docs/list-render-perf-report.md` 的 5.4「每次捕获的上限裁剪会在主线程做全表 COUNT」在本文复测仍然成立（2 万条 60–110 ms），并且又找到两条同源、同量级的聚合（2.2、2.3），建议一起修。
- `docs/code-quality-performance-review.md` 的「第三批」仍按原计划：P1（隐藏捕获不回填）已在 `gpui_clipboard.rs:393` 落地；P2/P3（捕获增量合并）在本文数据下**优先级下降**——把 2.2–2.4 修完，可见捕获的主线程成本已回到 10–20 ms，不必再引入增量列表协议。
- 本文不改变任何既有行为，也不涉及上游合并；实施时按 AGENTS.md 的 `wait:` 前缀与校验清单执行。

---

## 11. 修复状态

审计后按上表顺序落地，已经完成的改动：

| 项 | 改动 | 提交 |
|---|---|---|
| 2.2 未缓存的重复计数 | `reload_items` / 启动不再发 `COUNT(*)`，直接复用 `TitlebarStats.clearable_history_count`（同值） | `0885cde` |
| 2.3 标题栏统计 | 两个 `EXISTS` 探测 + 覆盖索引计数替代 `MAX/MAX/COUNT`；v12 建 `idx_hotkey` / `idx_fav_sort` / `idx_non_transfer` | `0885cde` |
| 2.4 上限裁剪 | 计数改走 `idx_deletable`（恒真范围谓词 `created_at >= ''`），并删除无选择性的 `idx_is_favorite` | `0885cde` |
| 5 收藏筛选排序 | `idx_fav_sort (pinned DESC, updated_at DESC) WHERE is_favorite = 1`，2 万条 113 ms → 3.3 ms | `0885cde` |
| 7 后台连接 | `Database::open_for_job`：OCR/QR/链接/网页标题/同步/中转站刷新不再重跑迁移链与启动全表图标清理（写锁） | `0885cde` + 本次 |
| 8 读路径 | `PRAGMA mmap_size = 256MB`：关键字扫描 65–112 ms → 18 ms，不增加 SQLite 页缓存堆 | `0885cde` |
| 3 列表页载荷 | v13 新增 `list_preview` / `list_preview_len`（写时预览 + 字节长度戳）：列表页 31.5 ms → 8.0 ms；旧行自动回退、无启动回填 | 本次 |
| 防止回退 | 新增执行计划测试（聚合必须走索引）+ 预览一致性/过期回退/写路径刷新测试 | `0885cde` + 本次 |

按本次实测，2 万条历史下「每次复制」的主线程成本从 **120–220 ms 降到约 10–20 ms**，「每次唤出」从 70–90 ms 降到约 20–30 ms（其中 13–16 ms 仍是列表页载荷，见下）。

尚未实施（按性价比排序，均需单独评估）：

1. **P2 快速窗口**：弹窗的独立搜索/筛选每字符走一次 `load_keyword_filtered_items`（最多 1 600 行整行扫描，`max_items` 越大越差），且没有防抖。**注意**：不能照搬主窗口的 150 ms 防抖——`set_query` 的注释说明「一次轮询可能整批处理连续字符 + 回车」，延迟重建会让回车粘到旧列表上。正确解法是把扫描移出 UI 线程（需要第二连接与请求代际），改动较大。默认设置下弹窗与主列表共用同一页，只有开启「独立筛选」或输入搜索词时才走这条路径。
2. **P3 每帧渲染**：卡片每帧 2–4 次 serde 解析、5 次 `String` clone、字体整形、`stat`/`open`（第 6 节）。既有帧追踪显示当前没有突破 vsync 预算，属于 CPU 浪费与低端机风险；改动集中在 `clipboard_card.rs` / `clipboard_list.rs`，需要 UI 冒烟验证。
3. **`synchronous = NORMAL`**：能把每次复制的 2–3 次提交从 `FULL` 降到 `NORMAL`，但这是耐久性语义变更（掉电可能丢最后一个事务），未在未经确认的情况下改动。
4. **`ANALYZE` / 冗余 tombstone 索引**：实测 `ANALYZE` 会让聚合退回全表扫描，明确不做；`idx_item_tags_tag`、`idx_del_items_hash`、`idx_uf_items_hash`、`idx_del_tags_name` 属冗余，收益仅常数级，暂不迁移。
5. **同步/合并路径的预览**：`insert_sync_item_raw` / `update_sync_item` / `merge_from` 仍写原始 `rich_data` 而不写 `list_preview`，这些行按设计走 SQL 兜底（正确但慢）；若要同步历史也享受快路径，需要在这三处补算预览。

维护提示：改动第 2 节涉及的查询或索引名时，`capture_aggregates_are_index_served` 会拦截「退回全表扫描」；改动 `list_item_select_columns()` 时，附录 B 的列表页数字与第 11 节 P1 的结论需要重新测量。
