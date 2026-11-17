# Clippi UI 重设计 · 需求文档

> 版本：v1.0（2026-11-17）
> 配套文档：[design.md](./design.md)（设计与设计系统方案）
> 参考实现：`C:\Develop\project\00_AI\Argus`（下称 Argus）

---

## 1. 背景

Clippi 当前 UI 由 Slint 版逐像素移植而来，功能完整但观感"简陋"。经对 `src/ui` 全量扫描，根因不是缺少设计，而是**缺少设计系统**：

| 指标 | 现状 | Argus 对照 |
|---|---|---|
| 主题文件外硬编码颜色 | **289 处**（`rgb(0xffffff)` 45 次、`rgb(0x191a1b)` 29 次…） | 页面层 0 处，全部走 `Palette` 语义色 |
| 暗色模式判断 | `theme.bg == rgb(0x191a1b)`，14 个文件 26 处 | `Palette.dark: bool` 一处定义 |
| 圆角取值 | 12 种（1~12 逐个都有） | 4 档（4/6/8/胶囊） |
| 间距取值 | 16 种（含 3/5/7/9 等 4pt 网格外值） | 4pt 网格 |
| 字号取值 | 10 种（8~16、26） | 5 档，基准 13px，行高 1.4 |
| 缓动/混色函数 | 9 份重复实现，散在 7 个文件 | 集中在 `theme.rs`，4 条曲线 |
| 交互状态 | 卡片 hover 无变化；按钮无 press；无 focus-visible | hover/press/focus-ring/disabled 全覆盖 |
| 图标 | iconfont PUA 码位与 SVG 混用 | Lucide 全集，单表登记 |
| 层级 | 卡片 `shadow_sm/md`、菜单 `shadow_lg` 到处投影 | 描边分层，仅浮层有阴影 |
| 工具提示 | 13 个文件各写一份 | 单一 `tooltip()` 工厂 |

同时，Argus 的组件生态已升级到 `gpui-kit 0.9.1`（内含 `gpui-component 0.9.1`）+ `gpui-pre 0.3.12`，提供 `base::Button / motion / animation`、`component::{Theme, TitleBar, Input, Tooltip, …}`、Lucide 图标资产（`gpui-kit-assets`）与主题语义令牌接口（`semantic_tokens()`）。Clippi 停留在 `gpui 0.2.2 + gpui-component 0.5.1 + gpui_transitions`，无法复用这些能力。

## 2. 目标与非目标

### 2.1 目标

1. **G1 设计系统**：建立"令牌表 → 语义调色板 → 控件工厂 → 页面语义"四层结构，对齐 Argus 的 `resources/theme.json → frontend/theme.rs → frontend/widgets.rs → pages/*`。
2. **G2 视觉升级**：以 Fluent 视觉语言（Argus 同款）重绘全部界面：描边分层、状态完整、动效克制、图标统一。
3. **G3 依赖迁移**：`gpui 0.2.2 → gpui-pre 0.3.12`、`gpui-component 0.5.1 → 0.9.1`、新增 `gpui-kit 0.9.1`，删除 `gpui_transitions`。
4. **G4 三窗口一致**：主面板、快捷粘贴窗、标签侧栏共享同一套令牌与控件。
5. **G5 质量内建**：验收指标可机器检查（lint 门禁 + 截图对照）。

### 2.2 非目标

- 不改动任何非 UI 逻辑：剪贴板监听、SQLite 存储、同步后端、热键、OCR、托盘、更新器全部不动。
- 不新增产品功能（本项目的"功能"仅限 UI 层能力：状态、动效、可访问性）。
- 不支持 Linux wayland/x11（维持现状：Windows + macOS）。
- 不做多主题/换肤市场；只有 浅色 / 暗色 / 跟随系统 ×（默认绿 / 系统强调色）。

## 3. 约束

| 编号 | 约束 |
|---|---|
| C1 | 配置兼容：`theme`（light/dark/system）、`theme_color`（default/system）、字体族与字号档位设置的含义不变，旧配置文件直接可用 |
| C2 | 保留现有字体缩放能力（`fs/sc/sx` + rem 同步），视障用户的大字号档位行为不回退 |
| C3 | i18n：现有 `I18nKey` 全保留；gpui-kit 组件自带文案（输入框右键菜单等）需按 Argus `AGENTS.md` 第 6 条的方式显式本地化 |
| C4 | 性能预算：主列表滚动 ≥ 当前帧率；重绘范围不得因 hover 扩大到整窗（Argus `AGENTS.md` 第 11 条同款要求） |
| C5 | 平台：Windows 11 Mica 为可选增强，Win10 与 macOS 不受影响；`SYSTEM_DRAWN_FRAME` 双路径继续工作 |
| C6 | `gpui-pre` 的 lib 名为 `gpui`，源码 `use gpui::*` 不变；但其为 Zed 快照，**版本必须用 `=0.3.12` 锁死** |

## 4. 功能需求

需求编号规则：FR-<域>-<序号>。优先级 P0（必须）→ P2（应该）→ P3（可选）。

### 4.1 FR-TOK 设计令牌

- **FR-TOK-01 (P0)** 新建 `assets/theme.json` 令牌表，字段结构对齐 Argus `resources/theme.json`：`fontSize`、`lineHeight`、`layout`（尺寸）、`fonts`（字体栈）、`light`/`dark`（双色板）、动效时长与曲线。
- **FR-TOK-02 (P0)** 令牌一次解析、全局缓存（Argus `theme.rs:98` 的 `PaletteKey` 缓存模式），禁止每帧解析 JSON。
- **FR-TOK-03 (P0)** `ClippiTheme` 重构为语义结构（见 design.md §4），新增 `is_dark`、`hover`、`hover_strong`、`press`、`stroke`、`stroke_strong`、`control_face`、`control_hover`、`control_checked`、`on_accent`、`on_danger`、`faint`、`scroll_thumb(_hover)`、`tip`、`flyout`、`scrim`。
- **FR-TOK-04 (P0)** 删除全部 26 处 `bg == rgb(0x191a1b)` 判断。
- **FR-TOK-05 (P0)** 清理 289 处硬编码颜色，目标：`src/ui/design/` 之外颜色字面量出现次数为 0（`transparent` 除外）。
- **FR-TOK-06 (P1)** 品牌绿保留为默认强调色；亮色模式提供对比度达标的深绿变体（AA 4.5:1）；`theme_color = "system"` 时沿用现有系统强调色读取逻辑。

### 4.2 FR-STATE 交互状态

- **FR-STATE-01 (P0)** 所有可交互元素具备 hover / press / disabled 三态；禁用态 0.5 透明度且不响应。
- **FR-STATE-02 (P0)** 键盘焦点环：2px 强调色、外扩 3px、圆角 = 元素圆角 +3，仅在键盘导航时显示（`:focus-visible` 语义，Argus `widgets.rs:443`）。
- **FR-STATE-03 (P0)** 剪贴板卡片：hover 有 4~6% 亮度反馈；选中态 = 强调色 15% 底 + 左侧 3px 竖条（替换现在的整圈强调色描边）；多选徽标保留。
- **FR-STATE-04 (P1)** 筛选 chips（类型 / 来源应用）升级为分段控件：轨道 + 抬高滑块，滑块 250ms 滑移（Argus `widgets.rs:650`）。
- **FR-STATE-05 (P1)** 行内编辑、热键录制、标签选择器打开时，其所属卡片保持可见的"激活"描边。

### 4.3 FR-MOTION 动效

- **FR-MOTION-01 (P0)** 新建 `ui/motion.rs`：`ease_out / ease_in / ease_move / ease_enter / ease_in_out / mix` 各一份，二分采样 CSS cubic-bezier（移植 Argus `theme.rs:525` 的 `bezier()`）。
- **FR-MOTION-02 (P0)** 时长只允许两档：快速 167ms、慢速 250ms；删除散落的 120/160/200/240ms 常量。
- **FR-MOTION-03 (P0)** 全部动效尊重 `cx.reduce_motion()`（系统"减少动态效果"）。
- **FR-MOTION-04 (P1)** 保留并收口现有动效：搜索框"新建"按钮展开（240→250ms）、Tab 切换淡入（160→167ms）、Toggle 滑块、卡片工具条淡入、侧栏折叠、Toast 进出。
- **FR-MOTION-05 (P2)** 删除 `gpui_transitions` 依赖，13 个调用文件改用 `gpui_kit::base::motion`。

### 4.4 FR-TYPE 排版

- **FR-TYPE-01 (P0)** 字号收敛为 5 档：10（辅助）/ 12（正文）/ 13（基准·强调）/ 16（标题）/ 26（大标题，仅设置页）。
- **FR-TYPE-02 (P0)** 行高统一 1.4（相对值），删除按像素逐处设置行高。
- **FR-TYPE-03 (P0)** 强调文字用 `FontWeight::MEDIUM`；`BOLD` 仅用于数值与标题；12px 以下不再使用 BOLD。
- **FR-TYPE-04 (P1)** 数字（时间戳、尺寸、计数、序号）启用等宽数字特性（`tnum`），滚动/刷新时不横向抖动。
- **FR-TYPE-05 (P1)** 建立 CJK 字体回退链（Argus `theme.rs:341` 模式），避免英文系统把中文渲染成日文字体；macOS 沿用现有 `.SystemUIFont` 逻辑。
- **FR-TYPE-06 (P0)** 三级文字色 `ink / ink-2 / ink-3` 替换 `text_1/2/3` 的一切散用。

### 4.5 FR-ICON 图标

- **FR-ICON-01 (P0)** 删除 iconfont（TTF 及 macOS 的 CTFontManager 注册逻辑），全部图标改为 `gpui-kit-assets` 内置 Lucide。
- **FR-ICON-02 (P0)** 建立 `ui/assets.rs` 的 `product_icons!` 单表登记（Argus `frontend/assets.rs:11` 模式）：名称 → Lucide 图标 → 嵌入集合，未登记名显示圆圈。
- **FR-ICON-03 (P0)** 图标尺寸 3 档：12 / 16 / 20；颜色一律 `currentColor` 语义（跟随文字色）。
- **FR-ICON-04 (P2)** 迁移清单：标题栏 6 按钮、筛选条 8 类型 + 标签按钮、底栏 2 按钮、右键菜单全部、设置页 chevron 与分组图标、空状态插图、`assets/picker-icons/*.svg` 中重复项。

### 4.6 FR-COMP 组件库

在 `ui/widgets.rs`（控件工厂）提供以下统一构造器，页面代码不得手写这些控件的样式（对齐 Argus `widgets.rs`）：

| 编号 | 组件 | 规格要点 |
|---|---|---|
| FR-COMP-01 (P0) | `btn / btn_small` | 32/24 高、6px 圆角、1px 描边 + 下沿加重（`control_edge`）、Tone 四色（Plain/Accent/Danger/Ghost）三态 |
| FR-COMP-02 (P0) | `toggle` | 40×20，off 仅描边、on 强调色实底；滑块 12px，悬停 14px、按下 17×14（Argus `widgets.rs:1028`） |
| FR-COMP-03 (P0) | `segments` | 分段控件：轨道 + 滑块 250ms 滑移（FR-STATE-04 载体） |
| FR-COMP-04 (P0) | `input` | 32 高；聚焦 = 底部 2px 强调线（Boxed）或 2px 外扩焦点环（Bare）；占位符统一 `ink-3` |
| FR-COMP-05 (P0) | `tooltip / tooltip_with_shortcut` | 12px、系统 tooltip 字体、无阴影；全仓唯一入口，替换 13 处重复 |
| FR-COMP-06 (P0) | `card / group` | 卡片 6px 圆角、1px `stroke`、无阴影；分组卡内行间 hairline |
| FR-COMP-07 (P0) | `menu / menu_item` | 8px 圆角、`stroke_strong` 描边、**无阴影**、30px 行高、4px 行圆角 |
| FR-COMP-08 (P0) | `dialog` | 8px 圆角、scrim 遮罩、双层投影（唯一允许投影的地方之二） |
| FR-COMP-09 (P1) | `scrollbar` | thumb 令牌化（暗 20%→悬停 34%），悬停加宽，主列表/快捷窗/设置三处统一 |
| FR-COMP-10 (P1) | `empty_state` | 图标 + 标题 + 说明 + 主操作，覆盖：无条目、搜索无结果、标签为空、后端为空、Emoji 无结果 |
| FR-COMP-11 (P1) | `keycaps` | 快捷键键帽（Argus `widgets.rs:1130`），统一现有 `shortcut_hint` |
| FR-COMP-12 (P2) | `toast` | 保留现有实现，换令牌 + 进出动效走 motion 模块 |
| FR-COMP-13 (P2) | `state_pill / spinner` | 状态胶囊与加载环（Argus `widgets.rs:1167/1218`） |

### 4.7 FR-SCREEN 界面重绘

逐屏需求（视觉规格见 design.md §10）：

- **FR-SCREEN-01 (P0)** 主面板：标题栏（30px 拖拽区 + 28px 图标按钮）、搜索框（28 高）、筛选条（22 高 chips→segments）、卡片列表、底栏（26px）。
- **FR-SCREEN-02 (P0)** 剪贴板卡片：10px 圆角、左侧 36px 类型区、缩略图/色板/文件图标规范化；收藏色条保留。
- **FR-SCREEN-03 (P0)** 快捷粘贴窗：紧凑行样式与主面板令牌一致。
- **FR-SCREEN-04 (P0)** 标签侧栏（独立窗口）：与主面板同套令牌。
- **FR-SCREEN-05 (P0)** 设置页：分组卡片（Argus `pages/settings.rs:217` 模式：68px 行高、20px 行首图标列、16px 间距、6px 圆角）；6 个 Tab 改为分段或下划线滑移指示器。
- **FR-SCREEN-06 (P1)** 编辑面板、Emoji 面板、标签选择器、类型筛选配置、右键菜单、确认框、Toast 全部换新组件。
- **FR-SCREEN-07 (P2)** 空状态全套（FR-COMP-10）。

### 4.8 FR-DEP 依赖迁移

- **FR-DEP-01 (P0)** 依赖变更：

  ```toml
  # 移除
  gpui = "0.2.2"            # 两个 target 段的三处引用
  gpui-component = "0.5"
  gpui_transitions = "0.1.5"

  # 新增
  gpui-pre = { version = "=0.3.12", default-features = false, features = ["font-kit", "windows-manifest"] }
  gpui-kit = "=0.9.1"       # 传递引入 gpui-component 0.9.1 / gpui-base / gpui-kit-assets
  ```

- **FR-DEP-02 (P0)** API 映射（50 处 `gpui_component::` 调用逐项迁移）：

  | 现调用（0.5.1） | 迁移后 |
  |---|---|
  | `gpui_component::input::{Input, InputEvent, InputState}` | `gpui_kit::component::input::{…}` |
  | `gpui_component::tooltip::Tooltip` | `gpui_kit::component::tooltip::Tooltip`（配 `widgets::tooltip` 工厂） |
  | `gpui_component::scroll::{Scrollbar, …}` | `gpui_kit::component::scroll`；或移植 Argus `frontend/scrollbar.rs` |
  | `gpui_component::{Theme, ThemeMode}` | `gpui_kit::component::{Theme, ThemeMode}` + `semantic_tokens()` |
  | `gpui_component::Root::new` | `gpui_kit::base::Root` + `ProductSurface` 插件（Argus `theme.rs:290`） |
  | `gpui_component::init / set_locale` | `gpui_kit::init(cx)` |
  | `gpui_component::text`（3 处） | `gpui_kit::component::text`（迁移期核对签名） |
  | `gpui_component::v_virtual_list`（2 处） | 迁移期核对 0.9 对应模块；不可用则保留 `uniform_list` 兜底 |
  | `gpui_transitions::use_keyed_transition`（13 文件） | `gpui_kit::base::motion::transition(id, target, Transition, window, cx)` |

- **FR-DEP-03 (P0)** `main.rs` 引导改造：`Application::new()` → `gpui_kit::application()`；`gpui_component::init` → `gpui_kit::init` + `design::install_surface`；窗口创建回调内先 `theme::apply()` 再建视图（Argus `frontend.rs:742` 顺序）。
- **FR-DEP-04 (P1)** 若 crates.io 的 `gpui-pre 0.3.12` 在 Windows/macOS 出现行为差异，按 Argus 根 `Cargo.toml` 的 `[patch.crates-io]` 方式 vendor（`vendor/gpui-pre`），并在补丁说明文档中记录差异。
- **FR-DEP-05 (P1)** 对齐 `windows`/`windows-sys` 版本，消除重复链接（Clippi 现 0.58/0.59，允许多版本共存但需验证 HWND 交互路径）。
- **FR-DEP-06 (P0)** 迁移期 `cargo build` 与现有单测全绿后再开始视觉改造（M0 门禁）。

### 4.9 FR-A11Y 可访问性

- **FR-A11Y-01 (P1)** 交互控件标注 `role()` / `accessibility_label()`（toggle 为 `Role::Switch`，分割器为 `Role::Splitter`）。
- **FR-A11Y-02 (P1)** 主路径控件 `tab_stop(true)`，Tab 顺序：搜索框 → 列表 → 筛选 → 标题栏按钮。
- **FR-A11Y-03 (P2)** 文字对比度：正文 ≥ 4.5:1，辅助 ≥ 3:1；亮色强调色变体须达标（FR-TOK-06）。

### 4.10 FR-MAT 材质（可选）

- **FR-MAT-01 (P3)** Windows 11 启用 `DWMWA_SYSTEMBACKDROP_TYPE`（Mica），窗口底色降为半透明，卡片改 `layer` 半透明白/黑（Argus `theme.rs:154` 模式）。Win10、`SYSTEM_DRAWN_FRAME` 路径与三窗口边界全部验证后才可发布。

## 5. 验收标准（可机器检查）

| 编号 | 标准 |
|---|---|
| AC-1 | `rg "rgba?\(0x" src/ui --glob '!design/**'` 仅剩 `transparent` 类命中 |
| AC-2 | `rg "191a1b" src/ui` 为 0 |
| AC-3 | `rounded` 取值 ⊆ {4, 6, 8, 10, 999（胶囊）}（允许 fs/sc 缩放包装） |
| AC-4 | 间距取值 ⊆ 4pt 网格 {2,4,6,8,12,16,24} |
| AC-5 | `text_size` 基准值 ⊆ {10,12,13,16,26} |
| AC-6 | 动效时长常量 ⊆ {167, 250}ms，且全部经过 `reduce_motion` |
| AC-7 | `font_family("iconfont")` 出现 0 次；图标统一走 `product_icons!` 表 |
| AC-8 | `Tooltip::element` 直接调用为 0（全部经 `widgets::tooltip`） |
| AC-9 | 缓动/混色函数定义仅存在于 `ui/motion.rs` |
| AC-10 | `cargo test` 全绿；`cargo build --release` 体积增幅 ≤ 15%（gpui-kit 引入 Lucide 资产所致） |
| AC-11 | 三窗口（主面板 / 快捷窗 / 侧栏）在浅/暗 × 100%/125%/150% DPI 下截图对照通过 |
| AC-12 | 主列表滚动帧率不低于改造前（同机对比，滚动 1000 条） |

## 6. 里程碑

| 阶段 | 内容 | 出口条件 |
|---|---|---|
| M0 依赖迁移 | FR-DEP-01~03/06；`gpui_transitions` 删除；三窗口可编译可运行（视觉暂不变） | AC-10 编译项 + 全部单测绿 |
| M1 设计系统底座 | FR-TOK-01~05、FR-MOTION-01~03、FR-TYPE-01~03/06、`ui/design/` 模块 | AC-1/2/6/9 |
| M2 控件工厂 | FR-COMP-01~08、FR-STATE-01~03 | AC-3/4/5/8 |
| M3 主面板重绘 | FR-SCREEN-01~02、FR-ICON-01~03 | 截图对照（主面板） |
| M4 设置与浮层 | FR-SCREEN-05~06、FR-COMP-09~12、FR-ICON-04、FR-A11Y | 截图对照（设置/菜单/对话框） |
| M5 快捷窗与侧栏 | FR-SCREEN-03~04 | AC-11 |
| M6 收尾 | FR-TYPE-04/05、FR-A11Y-03、性能核对、（可选）FR-MAT | AC-12 全量 |

## 7. 风险与对策

| 风险 | 影响 | 对策 |
|---|---|---|
| `gpui-pre` 与现 `gpui 0.2.2` 存在行为差异（同为 0.2.2 快照，但快照点不同） | 平台层（HWND、IME、托盘）回归 | M0 独立成阶段；vendor 补丁通道（FR-DEP-04） |
| `gpui-component 0.5→0.9` 破坏性 API | 50 处调用迁移工作量超预期 | 先迁移、后重构：M0 只做机械映射，样式改造放 M1+ |
| `v_virtual_list` 在 0.9 缺失或签名变化 | 主列表重写 | 兜底 `uniform_list`；列表高度计算已有单测保护 |
| 字号缩放链路（`sc/sx`）与新间距档冲突 | 大字号下裁切 | 间距不随字号缩放（对齐 Argus），仅卡片内部保留 `sc()`；125%/150% 截图门禁 |
| Lucide 资产体积 | 二进制变大 | 只嵌入 `product_icons!` 登记项（gpui-kit 宏已保证）；AC-10 体积门禁 |
| Win11 合成器光影 bug（Argus 曾遇，见其 `AGENTS.md` 第 8 条） | 浮层光晕 | 菜单/提示显式 `shadow_none`，仅对话框保留投影 |
| 视觉回归无基准 | 改完才发现退化 | 每阶段结束固化截图基准（浅/暗 × DPI），后续逐像素对照 |

## 8. Argus 参考索引

| 主题 | 位置 |
|---|---|
| 令牌表 | `Argus/resources/theme.json` |
| 语义层与缓存 | `Argus/src-gpui/src/frontend/theme.rs:10/98/198/419/512` |
| 控件工厂 | `Argus/src-gpui/src/frontend/widgets.rs`（btn:917 / toggle:1028 / segments:650 / keycaps:1130 / spinner:1167 / focus_ring:443 / control_edge:415 / flyout_shadow:1774） |
| 设置行规格 | `Argus/src-gpui/src/frontend/pages/settings.rs:217` |
| 图标登记表 | `Argus/src-gpui/src/frontend/assets.rs:11` |
| 滚动条 | `Argus/src-gpui/src/frontend/scrollbar.rs` |
| 引导顺序 | `Argus/src-gpui/src/frontend.rs:742` |
| 工程规约 | `Argus/src-gpui/AGENTS.md` |
| 依赖声明 | `Argus/Cargo.toml`（`[patch.crates-io]`）、`Argus/src-gpui/Cargo.toml`（`gpui-kit = "=0.9.1"`） |
