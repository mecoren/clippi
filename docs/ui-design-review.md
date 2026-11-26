# Clippi UI 改造评估（参考 Argus）

> **历史快照（2026-11）**：本文记录改造前的基线。当前 `src/ui/design/mod.rs` 的门禁测试会拦截新增颜色字面量，硬编码颜色已大幅减少；下文数量与行号仅用于对照，不代表现状。

> 仅分析，未改动任何代码。行号基于当前工作区。

## 一、结论先说

Clippi 的 UI 已经不是"没有设计"，而是**有像素级还原的意图，但没有设计系统**：

- `src/ui/theme.rs` 有 30 个颜色令牌，看起来很完整；
- 但主题文件之外仍有 **289 处硬编码颜色字面量**，卡片、筛选条、快捷窗各自发明颜色；
- 圆角出现 **12 种取值**（1/2/3/4/5/6/7/8/9/10/11/12），间距 **16 种取值**，字号 **10 种取值**；
- 暗色判断靠 `theme.bg == rgb(0x191a1b)` 这种比较，散落在 **14 个文件、26 处**；
- 缓动函数 / 颜色混合函数被复制了 **9 份**，分布在 7 个文件里。

Argus 的做法正好相反：**先有令牌表，再有一层控件工厂，页面代码里几乎不出现颜色、圆角和字号**。它把复杂度收在前面两层，所以页面看起来统一、克制、有层次。

下面按"Argus 有什么 → Clippi 现状 → 可执行改法"逐项对照。

---

## 二、Argus 的设计系统结构（可借鉴的骨架）

| 层 | Argus 位置 | 作用 |
|---|---|---|
| ① 令牌表 | `resources/theme.json`（143 行） | 字号 13、行高 1.4、布局尺寸、字体栈、浅/暗两套颜色、动效时长 |
| ② 语义层 | `src-gpui/src/frontend/theme.rs` | 把令牌解析成 `Palette`（带线程缓存），并提供 `font()/dimension()/ease_*/fluent()/mix()` |
| ③ 控件层 | `frontend/widgets.rs` | `btn/btn_small/toggle/segments/keycaps/spinner/state_pill/tooltip/select/focus_ring/control_edge/flyout_shadow` |
| ④ 页面层 | `frontend/pages/*` | 只写 `p.ink`、`p.hover`、`px(12.)`，不写颜色值 |

关键细节：

- **颜色是语义不是数值**：`ink / ink-2 / ink-3`（三级文字）、`layer / layer-strong`（两层表面）、`stroke / stroke-strong`（两级描边）、`control / control-hover / control-checked`（控件三态）、`hover / press`（交互叠加）、`on-accent / on-danger`（反白）。浅色里 `layer` 是 `rgb(255 255 255 / 0.5)` 半透明，靠"叠加"而不是"给每个面一个实色"。
- **只有浮层有阴影**：`flyout_shadow()` 只给对话框用；菜单、列表、卡片一律靠 1px 描边分层（`widgets.rs:1772`）。Clippi 反过来：卡片 `shadow_sm/shadow_md`、菜单 `shadow_lg`、hover 工具条自定义阴影 —— 阴影到处是，层次反而糊。
- **形态统一**：控件半径 4、菜单/浮层 8、设置卡片 6、胶囊 13；控件高度只有 32 和 24 两档（`widgets.rs:917`）。
- **焦点可见**：`focus_ring()` 是"外扩 3px 的 2px 描边"，只给 `:focus-visible`（`widgets.rs:443`）。
- **字体有兜底**：`sans/display/mono/tooltip` 四组字体栈 + CJK 回退链（避免英文系统把中文渲染成日文字体）、等宽数字 `tnum`（`theme.rs:341`）。
- **动效集中**：只 4 条曲线（`ease_out/ease_in/ease_move/ease_enter`）+ 167/250ms 两个时长，且全部过 `reduce_motion`（`theme.rs:512`）。

---

## 三、Clippi 可以处理的事情（按收益排序）

### P0-1 把颜色收进语义令牌

现状：`src/ui` 内除 `theme.rs` 外还有 289 处 `rgb(0x…)` / `rgba(0x…)`。重复最多的：

```
rgb(0xffffff) 45 次   rgb(0x191a1b) 29 次   rgba(0x00000000) 24 次
rgb(0x7ECBA3) 14 次   rgba(0x00000030) 10 次  rgba(0x00000066) 8 次
```

典型重复写法（`filter_bar.rs:155`、`filter_bar.rs:435`、`quick_paste.rs`、`hover_toolbar.rs:193`）：

```rust
let inactive = if theme.bg == rgb(0x191a1b) { rgba(0xffffff0a) } else { rgba(0x00000008) };
```

建议：

1. `ClippiTheme` 增加 `is_dark: bool` 字段，删掉 26 处背景色比较。
2. 补充缺失的角色：`hover`、`hover_strong`、`press`、`stroke_strong`、`control_face`、`control_hover`、`on_accent`、`on_danger`、`faint`、`scroll_thumb`。
3. 现有 `accent_overlay()` 之外，把"accent 45% 描边"（`filter_bar.rs:513`）、"标签底色"、"图片占位底"都提成方法。
4. 目标：主题文件外的颜色字面量降到接近 0（只允许 `transparent`）。

### P0-2 补状态：hover / press / focus / disabled

现状（`clipboard_card.rs:1632` 附近）：卡片只有 `selected ? accent : divider` 的描边，**指针悬停时卡片本身没有任何变化**（只有 hover 工具条淡入）；全仓几乎不用 `.active()`，按钮按下无反馈；没有可见焦点环。

建议：

- 卡片：默认 `surface + divider`；hover 叠加 4~6% 前景；选中改为 **accent 15% 底 + 左侧 3px accent 竖条**（比现在整圈 accent 描边安静得多，且和卡片内的"收藏色条"形成体系）。
- 所有可点元素统一三态：hover → press（更深/下沉 1px）→ disabled（0.5 透明度 + 不响应）。
- 键盘导航（这是剪贴板工具的主路径）必须有 `focus-visible` 环：2px accent，外扩 3px，圆角 = 圆角+3。

### P0-3 收敛尺寸：4pt 网格 + 有限档位

现状统计（`src/ui` 内）：

| 维度 | 现有取值 | 建议档位 |
|---|---|---|
| 圆角 | 1,2,3,4,5,6,7,8,9,10,11,12 | 4 / 6 / 8 / 10(面板) / 999(胶囊) |
| 间距 | 0,1,2,3,4,5,6,7,8,9,10,12,14,16,18,56 | 2 / 4 / 6 / 8 / 12 / 16 / 24 |
| 字号 | 8,9,10,11,12,13,14,15,16,26 | 10(极小幅) / 12(正文) / 13(强调) / 16(标题) / 26(大标题) |

注意 Clippi 有 `fs()/sc()` 缩放体系（`font.rs:180`），改档位时**只改调用处的基准值**，缩放链路不动；但 `sc()`（布局随字号缩放）现在被卡片内部大量使用，建议只保留在卡片内部，其余布局尺寸不跟字号缩放（Argus 就是"字号跟设置、间距不跟"）。

### P1-1 排版层级

现状：`text_size(fs(10.))` 出现 68 次、`fs(11)` 56 次、`fs(12)` 68 次；`FontWeight::BOLD` 用于 12px 标签（`settings/mod.rs:876`、`titlebar.rs:222`），在 1x 屏上显得糊且重。

建议（对齐 Argus）：

- 基准 13px，正文 12px，元信息 11px，说明文字 10px 且用 `text_3` 而非 BOLD。
- 强调用 `FontWeight::MEDIUM`（Argus 的 `LABEL_WEIGHT`），BOLD 只留给数值/标题。
- 统一 `line_height`（1.35~1.4）：现在只在个别地方显式设置（如 `settings/mod.rs:884`），长描述换行后行距不一致。
- 时间戳、尺寸、计数加 `tabular` 等宽数字，列表滚动时数字不跳动。

### P1-2 动效收口

现状：9 份重复实现

```
settings/mod.rs:764 ease_out      settings/sync.rs:1269 ease_out
root.rs:3475 ease_out             add_backend.rs:217 ease_out
search_box.rs:55 ease_out_damped  toggle.rs:138 ease_in_out
reorder.rs:271 ease_in_out        search_box.rs:42 mix   settings/mod.rs:1564 mix
```

建议新建 `ui/motion.rs`：`ease_out / ease_in / ease_move / ease_in_out / mix / duration_fast(167) / duration_slow(250) / reduce_motion(cx)`，全仓引用。顺带把 `root.rs` 里 57 处 transition 的时长统一到两个常量。

### P1-3 图标统一

现状：`font_family("iconfont")` + PUA 码位（`\u{e688}`、`\u{e6b6}`…）和 `assets/picker-icons/*.svg` 混用；同一行里字号 12/14/15/16 都有，粗细不可控，也无法跟随主题描边。

建议：全部换成单色 SVG（Lucide 风格，`currentColor`），只保留 12/16/20 三档尺寸；标题栏 6 个按钮、筛选条 8 个类型、底栏 2 个按钮优先迁移。这样 hover 变色、深色适配、DPI 都自动正确。

### P1-4 空状态与加载

现状：`ListNoItems`（"暂无条目"）+ `ListEmptyHint` 两行纯文本（`clipboard_list.rs`）。

建议：图标（描边风）+ 标题 + 一句说明 + 一个主操作（"新建条目" / "打开同步"）。同类位置统一：搜索无结果、标签为空、后端为空、Emoji 无结果，现在各自不同。

### P2-1 分段控件 / 筛选条

现状：`filter_chip()`（`filter_bar.rs:150`）只换底色，选中态靠 accent 13% 底 + 45% 描边；切换没有位移。

建议：做成 Argus 的 segmented：浅色轨道 + 抬高滑块，选中滑块 250ms 滑到目标（`widgets.rs:650`）。这是剪贴板工具里最高频的交互，收益最大。

### P2-2 工具提示统一

现状：`Tooltip::element(...)` 在 13 个文件里各写一遍，字号 10px，圆角/颜色各异。

建议：`ui/components/tooltip.rs` 提供 `tooltip(text) / tooltip_with_shortcut(text, key)`，统一 12px、系统 tooltip 字体、无阴影、深色用 `tip` 色。顺带解决快捷键提示（现在 `shortcut_hint.rs` 是另一套）。

### P2-3 层次改"描边分层"

现状：卡片 `shadow_sm`（暗色 `shadow_md`）、菜单 `shadow_lg`、hover 工具条自绘 `BoxShadow`。

建议：卡片去阴影，改 1px `stroke`（7~8% alpha）+ 顶部 1px 高光；菜单用 `stroke_strong` + 8px 圆角无阴影；只有确认框/浮层面板用双层阴影（参考 `widgets.rs:1774`）。层次感来自亮度差，不来自投影数量。

### P2-4 滚动条

现状：`Scrollbar::vertical(...).scrollbar_show(ScrollbarShow::Scrolling)`（`settings/mod.rs:729`），宽度写死 16px 区域。

建议：thumb 用令牌（暗色 20% / 悬停 34%），悬停加宽 3→5px 并淡入，全仓统一（主列表、快捷窗、设置页现在三套参数）。

### P3-1 窗口材质（可选，收益大但改动面广）

Argus 在 `platform::supports_backdrop()` 为真时把窗口背景做成全透明 + Mica（`theme.rs:154`）。Clippi 现在 Windows 只做 `DWMWA_WINDOW_CORNER_PREFERENCE`（`window_manager.rs:4815`），面板是实心 `#191a1b`。

可选方案：Win11 打开 `DWMWA_SYSTEMBACKDROP_TYPE`，浅色 `#f2f3f8` / 暗色 `#191a1b` 改为 92% 不透明，卡片改为半透明白/白 4%。这会让"简陋感"下降最明显，但需要处理 `SYSTEM_DRAWN_FRAME`、标签窗口、快捷窗三处的圆角与透明边界，建议放最后。

### P3-2 可访问性

Argus 每个控件都带 `role()` / `accessibility_label()` / `tab_stop()`（例如 toggle 是 `Role::Switch`）。Clippi 目前完全没有。对剪贴板工具，屏幕阅读器支持可以后置，但 `tab_stop` + `focus_visible` 应随 P0-2 一起做。

---

## 四、建议的落地顺序

| 阶段 | 内容 | 预估改动面 | 风险 |
|---|---|---|---|
| 1 | `is_dark` + 补语义令牌 + 清 289 处硬编码（先做 `filter_bar / hover_toolbar / quick_paste` 三个重灾区） | 中，机械改动 | 低 |
| 2 | 尺寸/字号/行高收敛到档位表；`ui/motion.rs` 收口 9 份重复 | 中 | 低（会整体位移 1~2px） |
| 3 | 卡片 hover/选中/焦点三态重做 + 按钮 press/disabled 统一 | 小，集中在 `clipboard_card.rs` `components/` | 中（影响截图与测试基准） |
| 4 | 图标换 SVG、tooltip 收口、空状态 | 大（图标逐个替换） | 低 |
| 5 | 滑块式分段控件、滚动条、阴影层级 | 中 | 低 |
| 6 | Mica 材质（可选） | 大（平台层 + 三个窗口） | 高 |

---

## 五、必须注意的差异（别直接搬）

1. **依赖不通用**。Argus 用 `gpui-kit 0.9.1` + `gpui-component 0.9.1` + `gpui-pre`（gpui 的 fork）；Clippi 是 `gpui 0.2.2` + `gpui-component 0.5.1`。Argus 的 `widgets.rs` 不能直接编译进 Clippi，**只能搬设计规则与纯 GPUI 的绘制手法**（`focus_ring`、`control_edge`、`segments` 的测量+滑块、双层阴影参数、`bezier`/`mix` 数学）。
2. **Clippi 有字号缩放**（`fs/sc/sx` + `rem_base`），Argus 是"字号跟设置、间距固定"。移植间距档位时别让卡片内部布局跟着字号放大导致裁切。
3. **Clippi 有三个窗口形态**：主面板（卡片列表）、快捷粘贴窗（紧凑行）、标签侧栏。任何令牌改动要同时过一遍 `quick_paste.rs` 和 `sidebar*.rs`，否则会出现"主窗精致、快捷窗仍是旧样式"的割裂。
4. **Windows 自绘/DWM 双路径**。`SYSTEM_DRAWN_FRAME` 为真时圆角与描边交给 DWM（`titlebar.rs:191`、`root.rs:1032`），改圆角令牌时这两个分支都要覆盖。
5. **测试基准**。`clipboard_card.rs` 里有按缩放计算高度的逻辑（`sc()` + `sx()`），收敛尺寸后需要重跑相关单测。

---

## 六、参考索引

**Argus**

- `resources/theme.json` — 令牌表（13px / 1.4 行高 / 双套颜色 / 字体栈 / 时长）
- `src-gpui/src/frontend/theme.rs:10` — `Palette` 语义色 + 缓存
- `src-gpui/src/frontend/theme.rs:198` — `apply()` 把令牌灌进组件库主题（含 radius、table、tab 全套）
- `src-gpui/src/frontend/theme.rs:419` — `dimension()/font()/line_height()/tabular()`
- `src-gpui/src/frontend/theme.rs:512` — `transition_slow/transition/bezier/ease_*`
- `src-gpui/src/frontend/widgets.rs:415` — `control_edge()` 控件下沿加粗
- `src-gpui/src/frontend/widgets.rs:443` — `focus_ring()`
- `src-gpui/src/frontend/widgets.rs:650` — `segments()` 滑动选中
- `src-gpui/src/frontend/widgets.rs:917` — `btn()` 三态 + 4px 圆角
- `src-gpui/src/frontend/widgets.rs:1028` — `toggle()` 40×20、悬停放大滑块
- `src-gpui/src/frontend/widgets.rs:1774` — `flyout_shadow()`
- `src-gpui/src/frontend/pages/settings.rs:217` — 设置行 68px / 20px 图标列 / 16px 间距
- `docs/design/README.md` — 设计原则（颜色语义、动效、键盘）

**Clippi 现状**

- `src/ui/theme.rs:11` — 30 个颜色令牌（缺 hover/press/stroke 分级）
- `src/ui/font.rs:180` — `fs/lh/sc/sx` 缩放体系（应保留）
- `src/ui/filter_bar.rs:150` — `filter_chip()` 只有两态
- `src/ui/filter_bar.rs:435` — 典型 `bg == 0x191a1b` 判暗
- `src/ui/clipboard_card.rs:1632` — 卡片只有选中描边，无 hover
- `src/ui/settings/mod.rs:793` — 设置分组卡（10px 圆角、无行首图标）
- `src/ui/hover_toolbar.rs:193` — 自绘阴影 + 自判暗色
- `src/ui/search_box.rs:42` — 重复的 `mix()` / `ease_out_damped()`
