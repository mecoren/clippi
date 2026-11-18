# Clippi UI 重设计 · 设计文档（含设计系统与依赖迁移方案）

> 版本：v1.0（2026-11-17）
> 配套文档：[requirements.md](./requirements.md)（需求、验收标准、里程碑）
> 参考实现：`C:\Develop\project\00_AI\Argus`。本文所有"Argus 对照"均给出可查阅的文件与行号。

---

## 1. 总体架构

### 1.1 四层结构（对齐 Argus）

```
┌─────────────────────────────────────────────────────────────┐
│ ① 令牌表   assets/theme.json                                │
│    颜色 / 字体栈 / 布局尺寸 / 动效时长与曲线，浅暗两套        │
├─────────────────────────────────────────────────────────────┤
│ ② 语义层   src/ui/design/                                   │
│    tokens.rs   解析 + dimension()/token()                   │
│    palette.rs  Palette（缓存）+ apply() 灌注 gpui-kit Theme │
│    typography.rs font()/line_height()/tabular()/fallbacks   │
│    motion.rs   4 曲线 + 2 时长 + transition 工厂            │
├─────────────────────────────────────────────────────────────┤
│ ③ 控件层   src/ui/widgets.rs + src/ui/assets.rs             │
│    btn / toggle / segments / input / tooltip / menu /       │
│    dialog / card / keycaps / empty_state / scrollbar        │
│    product_icons! 图标登记表                                 │
├─────────────────────────────────────────────────────────────┤
│ ④ 页面层   src/ui/*.rs（现有文件逐屏改写）                   │
│    只写 p.ink / p.hover / radius(SM) / font(BODY)，          │
│    不出现颜色字面量、圆角数字、字号数字                       │
└─────────────────────────────────────────────────────────────┘
```

Argus 对照：① `resources/theme.json`；② `frontend/theme.rs`；③ `frontend/widgets.rs` + `frontend/assets.rs`；④ `frontend/pages/*`。

### 1.2 目录规划

```
assets/
  theme.json              # 新增：设计令牌（唯一颜色来源）
src/ui/
  design/
    mod.rs                # 对外 re-export：Palette, font, motion, radius…
    tokens.rs             # theme.json 解析（OnceLock 缓存原始 JSON）
    palette.rs            # Palette 结构、缓存、apply()、install_surface()
    typography.rs         # 字号/行高/等宽数字/CJK 回退，接现有 font.rs 缩放
    motion.rs             # 曲线与时长（全仓唯一缓动定义）
    size.rs               # 圆角/间距/控件高度枚举
  widgets.rs              # 控件工厂（新）
  assets.rs               # product_icons! 登记表（重写现有 PickerAssets）
  font.rs                 # 保留：fs/sc/sx 缩放链路
  其余 *.rs               # 保持文件划分，逐屏改写内部实现
```

**分层铁律**（写入 `src/ui/design/mod.rs` 模块注释，效仿 Argus `AGENTS.md`）：

1. 颜色字面量只允许出现在 `assets/theme.json`。
2. 缓动/混色函数只允许定义在 `design/motion.rs`。
3. 页面层不得直接构造 `gpui_kit::component::Input/Tooltip/Button`，必须经 `widgets::` 工厂。
4. 图标只经 `widgets::icon(name, color)`，新增图标先在 `product_icons!` 登记。
5. 带内置文案的组件库控件（输入框右键菜单等）必须显式传 `I18nKey` 文案。

## 2. 令牌规范：`assets/theme.json`

结构对齐 Argus `resources/theme.json`，按 Clippi 的产品形态（窄面板、卡片列表、品牌绿）取值：

```json
{
  "fontSize": 13,
  "lineHeight": 1.4,
  "layout": {
    "unit": 4,
    "titlebarHeight": 30,
    "searchBarHeight": 28,
    "filterBarHeight": 22,
    "bottomBarHeight": 26,
    "settingsRowHeight": 68,
    "cardRadius": 10,
    "groupRadius": 8,
    "controlRadius": 6,
    "smallRadius": 4,
    "cardTypeWidth": 36,
    "favBarWidth": 3,
    "iconSm": 12,
    "iconMd": 16,
    "iconLg": 20,
    "minPanelWidth": 340
  },
  "fonts": {
    "sans": ["Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI"],
    "mono": ["Cascadia Mono", "Consolas"],
    "tooltip": ["Segoe UI", "Microsoft YaHei UI"],
    "fallback": ["Microsoft YaHei UI", "Yu Gothic UI", "Meiryo", "Segoe UI Symbol"],
    "traditionalChinese": ["Microsoft JhengHei UI", "Microsoft JhengHei", "Microsoft YaHei UI"]
  },
  "motion": {
    "t-fast": "167ms",
    "t-slow": "250ms",
    "ease-out": "cubic-bezier(0.1, 0.9, 0.2, 1)",
    "ease-in": "cubic-bezier(0.7, 0, 1, 0.5)",
    "ease-move": "cubic-bezier(0.33, 0, 0.1, 1)",
    "ease-enter": "cubic-bezier(0.33, 1, 0.68, 1)"
  },
  "light": {
    "window-bg": "#f3f4f6",
    "ink": "#1d1f22",
    "ink-2": "#5c6167",
    "ink-3": "#7d828a",
    "layer": "rgb(255 255 255 / 0.6)",
    "layer-strong": "rgb(255 255 255 / 0.85)",
    "hover": "rgb(0 0 0 / 0.04)",
    "hover-strong": "rgb(0 0 0 / 0.06)",
    "press": "rgb(0 0 0 / 0.07)",
    "stroke": "rgb(0 0 0 / 0.08)",
    "stroke-strong": "rgb(0 0 0 / 0.14)",
    "control": "rgb(255 255 255 / 0.75)",
    "control-hover": "rgb(250 250 250 / 0.6)",
    "control-checked": "#ffffff",
    "flyout": "rgb(251 251 253 / 0.95)",
    "scrim": "rgb(0 0 0 / 0.28)",
    "accent": "#287a55",
    "on-accent": "#ffffff",
    "fav": "#b07a2e",
    "danger": "#c42b1c",
    "on-danger": "#ffffff",
    "pin": "#1668dc",
    "tag-bg": "rgb(0 0 0 / 0.05)",
    "tag-ink": "#5c6167",
    "tip": "rgb(252 252 253 / 0.96)",
    "scroll-thumb": "rgb(0 0 0 / 0.22)",
    "scroll-thumb-hover": "rgb(0 0 0 / 0.38)"
  },
  "dark": {
    "window-bg": "#1f2021",
    "ink": "#eef0f0",
    "ink-2": "#a6aaad",
    "ink-3": "#75797d",
    "layer": "rgb(255 255 255 / 0.045)",
    "layer-strong": "rgb(255 255 255 / 0.07)",
    "hover": "rgb(255 255 255 / 0.05)",
    "hover-strong": "rgb(255 255 255 / 0.08)",
    "press": "rgb(255 255 255 / 0.03)",
    "stroke": "rgb(255 255 255 / 0.08)",
    "stroke-strong": "rgb(255 255 255 / 0.13)",
    "control": "rgb(255 255 255 / 0.06)",
    "control-hover": "rgb(255 255 255 / 0.09)",
    "control-checked": "rgb(255 255 255 / 0.13)",
    "flyout": "rgb(38 39 41 / 0.95)",
    "scrim": "rgb(0 0 0 / 0.45)",
    "accent": "#7ecba3",
    "on-accent": "#10241a",
    "fav": "#d9a55d",
    "danger": "#ff6a5c",
    "on-danger": "#2a0400",
    "pin": "#5b9bff",
    "tag-bg": "rgb(255 255 255 / 0.07)",
    "tag-ink": "#a6aaad",
    "tip": "rgb(44 45 47 / 0.96)",
    "scroll-thumb": "rgb(255 255 255 / 0.2)",
    "scroll-thumb-hover": "rgb(255 255 255 / 0.34)"
  }
}
```

### 2.1 取值依据

| 令牌 | 依据 |
|---|---|
| `layer / layer-strong / stroke / control` 全部半透明 | Argus 的分层方式：亮度差而不是实色堆叠（`theme.json:58-65`）；为 Mica（P3）预留 |
| 亮色 `accent #287a55` / 暗色 `#7ecba3` | 沿用 Argus "同色相、亮色加深"模式（其 `#0067c0`/`#4cc2ff`）。`#6ab890` 白底 2.2:1 不达标；M9 复核 `#2e8f66` 反白仍只有 4.01:1，再压深到 `#287a55`（反白 5.24:1、作正文 4.76:1） |
| 亮色 `ink-3 #7d828a` / 暗色 `#75797d` | M9 实测原值在卡片上 2.78:1 / 3.08:1，辅助文字 3:1 不达标，各提一档到 3.7:1 / 3.4:1 |
| `on-accent` 暗色用深绿 | 暗色 accent 是浅绿，反白文字应为深色（Argus `on-accent #00213b` 同理） |
| `fav` 亮色加深为 `#b07a2e` | 现两模式共用 `#d8a155`，白底对比不足 |
| 半径 4/6/8/10 | 控件 4（Argus 按钮）、卡片组 6（Argus 设置卡）、浮层 8（Argus 菜单/对话框）、剪贴板卡片 10（保留现有身份，最大层级） |
| `fontSize 13 / lineHeight 1.4` | Argus 同值；GPUI 默认 1.618 行高会让换行文本偏高 |

## 3. 语义层 `design/palette.rs`

### 3.1 Palette 结构

```rust
#[derive(Clone, Copy)]
pub struct Palette {
    // 文字三级
    pub ink: Hsla,        // 原 text_1
    pub ink_2: Hsla,      // 原 text_2（次要说明、元信息）
    pub ink_3: Hsla,      // 原 text_3（占位符、禁用文字、极弱标签）
    // 表面
    pub background: Hsla, // 窗口底（Mica 开启时 opacity 0）
    pub layer: Hsla,      // 卡片
    pub layer_strong: Hsla,
    pub flyout: Hsla,     // 菜单/下拉（不透明，阴影画在底下）
    // 描边
    pub stroke: Hsla,
    pub stroke_strong: Hsla,
    // 交互叠加（画在元素之上或混入底色）
    pub hover: Hsla,
    pub hover_strong: Hsla,
    pub press: Hsla,
    // 控件
    pub control_face: Hsla,
    pub control_hover: Hsla,
    pub control_checked: Hsla,
    pub control_edge: Hsla, // 底边加重线：remainder(stroke, stroke_strong)
    // 强调与语义
    pub accent: Hsla,
    pub on_accent: Hsla,
    pub fav: Hsla,
    pub danger: Hsla,
    pub on_danger: Hsla,
    pub pin: Hsla,        // 传输站固定（原 transfer_pin_color）
    // 附件
    pub tag_bg: Hsla, pub tag_ink: Hsla,
    pub tip: Hsla, pub scrim: Hsla,
    pub scroll_thumb: Hsla, pub scroll_thumb_hover: Hsla,
    pub dark: bool,
}
```

要点（全部照搬 Argus 手法）：

- **缓存**：`thread_local! Cell<Option<(PaletteKey, Palette)>>`，键 = `(暗色?, accent, ink, background)`。列表每帧取色零成本（Argus `theme.rs:98`）。
- **`remainder()`**：由 `stroke` 与 `stroke_strong` 推出控件底边加重线的实色（Argus `theme.rs:76`）。
- **`on_card(name)`**：半透明控件面叠在卡片上的不透明等效色，避免透明窗口上多层叠加发暗（Argus `theme.rs:68`）。
- **系统强调色**：`theme_color == "system"` 时保留现有 `platform::accent::read_system_accent()`，替换 `accent` 并按 `dark` 混出 `accent_soft`（现 `theme.rs:112` 逻辑迁入）。
- **混合**：一切"强调色 15% 底"类派生改为 `mix()`（见 §6），不再用 `Rgba{a:…}` 手工拼。

### 3.2 `apply()`：灌注 gpui-kit 主题

对齐 Argus `theme.rs:198`（`Theme::change` → `semantic_tokens()` → 逐角色赋值）：

```rust
pub fn apply(mode: ThemeMode, window: &mut Window, cx: &mut App) {
    Theme::change(mode, Some(window), cx);
    let t = Theme::global_mut(cx).semantic_tokens();
    t.colors.background = token("window-bg", dark);
    t.colors.foreground = token("ink", dark);
    t.colors.surface = opaque(token("flyout", dark));   // 菜单阴影画在不透明面下
    t.colors.primary = accent;                          // 系统强调色在此生效
    t.colors.primary_foreground = token("on-accent", dark);
    t.colors.secondary = token("control", dark);
    t.colors.muted_foreground = token("ink-2", dark);
    t.colors.border = token("stroke", dark);
    t.colors.ring = accent;
    t.typography.md.size = typography::font(13.);
    t.typography.sans = typography::installed_family("sans", cx);
    // Clippi 专属
    t.radius_lg = px(8.);   // 对话框/菜单
    // …（list_hover、table_*、switch、tab_* 等按 Argus apply() 同样处理）
    Theme::global_mut(cx).apply_semantic_tokens(&t);
    Theme::sync_base(cx);
}
```

### 3.3 `install_surface()`：产品表面

替换现在的 `gpui_component::Root`：

```rust
pub fn install_surface(cx: &mut App) {
    gpui_kit::base::Root::register_plugin(cx, |_, _| ProductSurface);
}
struct ProductSurface;
impl Render for ProductSurface { /* div() */ }
impl gpui_kit::base::RootPlugin for ProductSurface {
    fn prepare(&mut self, window: &mut Window, _: &mut Context<Self>) {
        window.set_rem_size(typography::rem_base());
    }
    fn style(&self, surface: &mut Stateful<Div>, _: &mut Window, cx: &mut App) {
        surface.text_style().font_fallbacks = Some(typography::fallbacks(cx));
        surface.text_style().font_features = Some(typography::tabular());
        surface.text_style().line_height = Some(typography::line_height());
    }
}
```

（Argus `theme.rs:290-320` 原样模式；`rem_base()` 来自现有 `font.rs:207`，缩放能力保留。）

## 4. 排版规范 `design/typography.rs`

| 档位 | 基准值 | 用途 |
|---|---|---|
| CAPTION | 10 | 徽标、keycaps、时间戳单位 |
| BODY | 12 | 卡片正文、菜单项、底栏 |
| BASE | 13 | 设置行标题、输入框、按钮 |
| TITLE | 16 | 设置页标题、对话框标题 |
| DISPLAY | 26 | 大数字/空状态主标题（可选） |

- 全部经 `font(base) = px(base * scale())` 缩放，`scale()` 即现 `font.rs` 的档位（0.92~1.30）。
- 行高全局 `relative(1.4)`，由 `ProductSurface` 继承；个别密排行（卡片元信息）允许 `relative(1.25)`。
- `tabular()`：`FontFeatures([("tnum", 1)])`，应用于时间戳、大小、计数、Alt 序号（Argus `theme.rs:467`）。
- `fallbacks()`：`fonts.fallback` 链 + 繁体/日文特殊链（Argus `theme.rs:345`）；解决英文系统中文渲染成日文字体的问题。
- `installed_family()`：取字体栈中第一个系统实际安装的族名，避免缺族落入比例默认（Argus `theme.rs:384`）；macOS 继续用现有 `system_family()` 的 DirectWrite/CoreText 防崩逻辑。
- 字重：`LABEL_WEIGHT = MEDIUM` 全局常量；BOLD 只用于数值和 DISPLAY。

## 5. 尺寸规范 `design/size.rs`

```rust
pub enum Radius { SM, MD, LG, CARD, PILL }   // 4 / 6 / 8 / 10 / 999
pub enum Space { XS, SM, MD, LG, XL, XXL }   // 2 / 4 / 6 / 8 / 12 / 16(+24)
pub enum Ctrl { CHIP, INPUT, BTN, BTN_SM }   // 22 / 28(32*) / 32 / 24
```

- 4pt 网格：`px(3)`、`px(5)`、`px(7)` 全部归档到最近档（视觉差 ≤1px，AC-4 门禁保证不再回潮）。
- `dimension(key)` 直读 `theme.json.layout`（Argus `theme.rs:419`），卡片内部需要随字号缩放的尺寸继续走 `sc()`，但**仅限 `clipboard_card.rs` / `quick_paste.rs` 的行内内容**，容器间距一律固定像素。

## 6. 动效规范 `design/motion.rs`

```rust
pub fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32; // 二分采样（移植 Argus theme.rs:525）
pub fn ease_out(t)   -> f32;  // bezier(0.1, 0.9, 0.2, 1)     入场/淡入
pub fn ease_in(t)    -> f32;  // bezier(0.7, 0.0, 1.0, 0.5)   退场
pub fn ease_move(t)  -> f32;  // bezier(0.33, 0.0, 0.1, 1)    位移（滑块、抽屉）
pub fn ease_enter(t) -> f32;  // bezier(0.33, 1.0, 0.68, 1)   页面滑入
pub fn ease_in_out(t)-> f32;  // 现有 toggle/reorder 用的三次式，唯一保留的自写曲线
pub fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla;             // 预乘 sRGB 插值（Argus theme.rs:546）

pub const FAST: Duration = 167ms;
pub const SLOW: Duration = 250ms;
pub fn transition(cx) -> gpui_kit::base::motion::Transition;  // FAST + ease_move，含 reduce_motion→0
```

组件级动效清单（时长/曲线/触发）：

| 动效 | 时长 | 曲线 |
|---|---|---|
| 分段控件滑块、导航下划线滑移 | 250 | ease_move |
| 页面/Tab 切换滑入 | 250 | ease_enter |
| 菜单/下拉淡入淡出 | 167 | ease_out / ease_in |
| Toggle 滑块、hover 变色 | 167 | ease_move |
| 卡片工具条淡入 | 167 | ease_out |
| 搜索框"新建"按钮展开 | 250 | ease_move（保留现有 retarget 语义） |
| 对话框入场（scrim + 卡片） | 250 | ease_out |
| Toast 进/出 | 167 / ease_in | — |

所有入口统一经 `gpui_kit::base::motion::transition(id, target, Transition, window, cx)`，替代 13 个文件里的 `gpui_transitions::use_keyed_transition`。

## 7. 图标规范

- 登记：`product_icons!` 宏（Argus `frontend/assets.rs:11`），一表三用：名称查找、嵌入集合、测试名单。
- 绘制：`icon(name, color)` → Lucide `currentColor` 描边；尺寸 12/16/20。
- 首批登记草案（IconName 按 gpui-kit-assets 实际目录核对，未列出的兜底 `Circle`）：

| 用途 | 名称 | Lucide |
|---|---|---|
| 搜索 | SearchIcon | Search |
| 新建 | PlusIcon | Plus |
| Emoji 面板 | EmojiIcon | Smile |
| 热键筛选 | HotkeysIcon | Keyboard |
| 传输站 | TransferIcon | Send / ArrowLeftRight |
| 收藏 | FavIcon | Star |
| 置顶 | PinIcon | Pin |
| 设置 | SettingsIcon | Settings |
| 标签筛选 | TagIcon | Tag |
| 类型：文本/富文本/图片/文件/链接/路径/颜色/联系人 | Type*Icon | Type / FileText / Image / Paperclip / Link / Folder / Palette / User |
| 黑名单 | BlockIcon | Ban |
| 复制/编辑/删除/合并 | Copy/Pencil/Trash/Merge | Copy / Pencil / Trash2 / Combine |
| 展开/折叠 | Chevron* | ChevronUp/Down/Left/Right |
| 空状态 | EmptyStateIcon | ClipboardList |

- 现有 `assets/picker-icons/*.svg` 中与 Lucide 重复的删除；emoji 分类图标保留 SVG 但统一 12px 档。
- iconfont.ttf、`main.rs:327-381` 的 macOS 注册与 Windows 加载逻辑整体删除。

## 8. 状态与层级规范

### 8.1 状态矩阵

| 状态 | 表现 | 实现 |
|---|---|---|
| hover | 叠加 `hover`（4~5%） | `.hover()` 或 group_hover |
| hover-strong | 工具条等高亮区 `hover_strong` | — |
| press | 叠加 `press` + 文字降为 `ink_2` | `.active()` |
| selected（卡片） | `accent` 15% 底 + 左 3px `accent` 竖条 | `mix(layer, accent, .15)` |
| selected（分段/chip） | 抬高滑块 `control_checked` | segments |
| focus-visible | 2px `accent` 外扩 3px，圆角+3 | `focus_ring()`（移植 Argus widgets.rs:443） |
| disabled | 整体 0.5 透明度，不响应 | `.disabled(true).opacity(0.5)` |
| busy | 控件位换成 `spinner` | FR-COMP-13 |

### 8.2 层级与阴影（与现状最大的观念转变）

| 层 | 做法 |
|---|---|
| 窗口 | `window-bg` 实色（Mica 开启时透明）；DWM 圆角/描边路径不变 |
| 卡片 | `layer` + 1px `stroke`，**无阴影**；顶部可加 1px `stroke` 高光（暗色） |
| 菜单/下拉 | 不透明 `flyout` + `stroke_strong` + 8px 圆角，**无阴影** |
| 对话框/浮层面板 | `flyout` + `scrim` 遮罩 + **唯一允许的双层投影** `flyout_shadow()`（Argus widgets.rs:1774：暗 0.42/12/32 + 0.25/2/8；亮 0.14/8/28 + 0.06/2/6） |
| 工具提示 | `tip` + `stroke_strong`，无阴影（防 NVIDIA over Mica 光晕，Argus AGENTS.md 第 8 条教训） |

现状对照：卡片 `shadow_sm/md`（clipboard_card.rs:1647）、菜单 `shadow_lg`（context_menu.rs:656）、hover 工具条自绘 `BoxShadow`（hover_toolbar.rs:522）全部取消，仅对话框保留投影。

## 9. 组件规格 `ui/widgets.rs`

全部为"构造器 + 工厂"形态（对齐 Argus `widgets.rs`），页面只组装：

| 组件 | 规格（缺省值可覆盖） |
|---|---|
| `btn(id, text, glyph, Tone)` | 32 高 / 12px 内边距 / 6px 圆角 / gap 6；Plain=control 面 + stroke + 底边加重；Accent/Danger=实底 + on-*；Ghost=透明。hover/press 见 §8.1 |
| `btn_small` / `btn_glyph` | 24 高 / 8px 内边距；图标钮 32×32（标题栏用 28×28 变体） |
| `toggle(id, value, …)` | 40×20；off=透明底 + `ink_2` 描边 + `ink_2` 12px 滑块；on=accent 实底 + on_accent 滑块；hover 滑块 14px、active 17×14；`Role::Switch` |
| `segments(id, choices, …)` | 轨道 `tag_bg` + 1px `stroke`、6px 圆角、24 高；选中滑块 `layer_strong` + `stroke_strong`，250ms ease_move 滑移；锚点测量 + `remember()` 通知机制（Argus widgets.rs:270 `remember`） |
| `input(state, Field::Boxed/Bare)` | Boxed：28 高（Clippi 搜索框）/ 32（设置）；`control` 面 + `stroke` + 底边加重；聚焦底边变 2px accent。Bare：透明，聚焦画 focus_ring。占位符统一 `ink_3` 13px |
| `tooltip(text)` / `tooltip_key(text, key)` | 12px、tooltip 字体栈、`tip` 底 + `stroke_strong`、无阴影；`shadow_none()` 显式声明 |
| `card(children)` | `layer` + `stroke` + 10px 圆角 + overflow_hidden |
| `group(title, rows)` | 设置分组：标题（13px MEDIUM `ink`，上 6 下 6）+ 卡片（6px 圆角）+ 行间 `stroke` hairline（左右 14 缩进） |
| `settings_row(icon, title, desc, controls)` | 68 高 / 16px 内边距 / gap 16；行首 20px 图标列；标题 13 + 描述 12 `ink_2`（Argus pages/settings.rs:217） |
| `menu(items)` / `menu_item` | 宽 ≥164 / 8px 圆角 / 4px 内边距 / 行 30 高 4px 圆角；快捷键列右对齐；分隔线 `stroke` |
| `dialog(title, body, actions)` | scrim + 8px 圆角 + flyout_shadow；入场 250 ease_out；标题 TITLE 档 |
| `keycaps(keys)` | 每键 8/2 内边距、1px `stroke_strong` + 2px 底边、4px 圆角、`control` 面、12px（Argus widgets.rs:1130） |
| `empty_state(icon, title, desc, action)` | 居中列：20px 图标 `ink_3` + 标题 13 MEDIUM + 描述 12 `ink_2` + 主按钮 |
| `scrollbar(handle, dark)` | thumb 3px→悬停 5px、`scroll_thumb(_hover)` 令牌；滚轮滑行（可选移植 Argus scrollbar.rs:601 `wheel_glide`） |
| `spinner(size, color)` | 0.22 透明轨道 + accent 弧，800ms 旋转（Argus widgets.rs:1167） |

## 10. 逐屏设计

### 10.1 主面板（自上而下）

1. **标题栏 30px**：左 = 20px Logo + 应用名（12px MEDIUM `ink_2`）+ 拖拽区；右 = 28×28 图标按钮（Emoji/热键/传输/收藏/置顶/设置），hover `hover`、active `press`、激活态 accent 图标色；现有 Alt 快捷键 popover 保留并改 keycaps 风格。
2. **搜索框 28px**：`Bare` 输入 + 左侧 16px 搜索图标 + 右侧"新建"按钮（保留现有 hover 展开 pill 动效，色值换令牌：rest=`control`/`stroke`，open=`accent` 实底 + `on_accent` 文字）。
3. **筛选条 22px**：类型筛选改 `segments`（图标+文字自适应降级为图标 Only 的逻辑保留）；来源应用横滚条沿用滚动容器但 chip 换分段滑块选中态；尾部标签按钮 24×22。
4. **卡片列表**：卡片间距 6；卡片 = `layer` + `stroke` + 10px 圆角，左 36px 类型区（缩略图 20px 圆角 4 / 色板 / 文件图标），右侧内容 12px；元信息行（时间、来源应用、标签 pill）11px `ink_3` + tabular；收藏条 3px `fav`；hover 叠加、选中 accent 15% + 竖条（§8.1）。
5. **hover 工具条**：浮在卡片顶部边缘的 pill 组，`flyout` 底 + `stroke_strong`、**去阴影**；淡入 167ms。
6. **底栏 26px**：顶边 `stroke` hairline；左 = 前台应用图标 15px + 名称 12 MEDIUM + 窗口标题 11 `ink_3`（ellipsis）；右 = 22×22 两个黑名单按钮。

### 10.2 设置页

- 顶栏：返回按钮（28×28）+ "设置" TITLE 档；Tab 行改**下划线滑移指示器**（3px `accent`，250ms ease_move 跟随选中项，Argus 导航指示器同思路 `layout.rs:196`）。
- 内容：`group()` 分组卡 + `settings_row()`；行首加 20px Lucide 图标（现无图标，视觉密度立刻不同）；右列控件只允许 toggle/segments/input/btn/keycaps。
- 滚动条换 `scrollbar()`；右键菜单、确认框、字体选择器、应用列表面板全部换 §9 组件。

### 10.3 快捷粘贴窗

- 紧凑行 28~34 高：`hover` 叠加 + 选中 accent 15% + 左 3px 竖条；类型条/标签条与主面板同 `segments` 令牌；提示浮层换 `tooltip`/`menu`。

### 10.4 标签侧栏（独立窗口）

- 与主面板同套令牌；标签 pill = `tag_bg` + `tag_ink`，激活 = accent 15% + accent 文字；编辑/删除按钮 22×22 三态。

### 10.5 空状态

- 无条目：`ClipboardList` 图标 + "暂无条目" + "复制的内容将显示在这里" + 主按钮"新建条目"（触发搜索框同款事件）。
- 搜索无结果 / 标签为空 / 后端为空 / Emoji 无结果：同结构，图标与文案不同；主操作分别为"清空筛选""创建标签""添加后端""换个关键词"。

## 11. 依赖迁移设计

### 11.1 事实基础（已核实）

| 项 | 结论 |
|---|---|
| `gpui-pre 0.3.12` | crates.io 包，**lib 名 = `gpui`**，是 Zed gpui `0.2.2`（rev d2a4fb6）的发布快照；现有 `use gpui::*` 无需改动 |
| `gpui-kit 0.9.1` | 依赖 `gpui-component 0.9.1`、`gpui-base 0.9.1`、`gpui-fast 0.1.15`、`gpui-kit-assets 0.9.1`、`gpui-pre 0.3.x` |
| Argus vendor | `vendor/gpui-pre(-windows)` 仅作 `[patch.crates-io]` 兜底通道，默认用 registry 版本 |
| 现使用面 | `gpui_component::` 50 处 / 25 文件；`gpui_transitions` 13 文件；`window.window_handle()` 原生窗口交互走 raw-window-handle 0.6，与 gpui-pre 一致 |

### 11.2 Cargo.toml 变更

```toml
[dependencies]
# 删除：gpui, gpui-component, gpui_transitions
gpui-pre = { version = "=0.3.12", default-features = false, features = ["font-kit"] }
gpui-kit = "=0.9.1"

[target.'cfg(target_os = "windows")'.dependencies]
gpui-pre = { version = "=0.3.12", default-features = false, features = ["windows-manifest"] }
# windows-sys / windows 0.58 现状保留；与 gpui-pre 传递的 windows 版本共存，
# 仅在 HWND 传递路径（platform/*.rs）验证 ABI 后再考虑统一升级

[target.'cfg(target_os = "macos")'.dependencies]
# objc2 系列保留；gpui-pre 的 font-kit 特性在 macOS 必需（对齐现 gpui 配置）
```

> 注意：`gpui-pre` 的 `default` 含 wayland/x11，必须关闭（C6：不支持 Linux）。`windows-manifest` 仅 Windows target 段开启，与现有写法一致。

### 11.3 `main.rs` 引导改造（对照现 `main.rs:319-326/760` 与 Argus `frontend.rs:742`）

```rust
// 之前
Application::new().with_assets(ui::assets::PickerAssets).run(|cx| {
    gpui_component::init(cx);
    /* iconfont 注册 … 删除 */
    cx.open_window(options, |window, cx| {
        // …
        cx.new(|cx| gpui_component::Root::new(view, window, cx))
    })
});

// 之后
gpui_kit::application()
    .with_assets(ui::assets::Assets)          // product_icons! 生成的 AssetSource
    .run(|cx| {
        gpui_kit::init(cx);
        ui::design::install_surface(cx);      // ProductSurface：rem/行高/回退/tnum
        cx.open_window(options, |window, cx| {
            // 主题必须先于第一个布局（Argus frontend.rs:788-801 的顺序注释）
            ui::design::typography::apply_scale(settings.font_scale());
            ui::design::palette::apply(mode_from(settings), window, cx);
            let view = cx.new(|cx| RootView::new(window, state, wm, cx));
            window.defer(cx, move |window, cx| platform_attach(window, cx)); // HWND 等
            view
        })
    });
```

三个窗口（主面板、快捷窗、侧栏）同法处理：`gpui_component::Root::new` 全部删除，窗口根视图直接返回（`ProductSurface` 已由插件机制接管 Root 职责）。

### 11.4 迁移核对清单（M0 逐项过）

1. `gpui_component::input::InputState::new(window, cx)` API 在 0.9 的签名（Argus 用法见 `frontend/widgets.rs:499`，含 `context_menu` 定制 → 顺带落实 C3 本地化）。
2. `v_virtual_list` / `VirtualListScrollHandle` 在 `gpui_kit::component` 的对应物；缺失则主列表退 `uniform_list`（高度计算已有单测）。
3. `gpui_component::text` 3 处调用的 0.9 签名。
4. `Scrollbar` 构件在 0.9 的参数（或直接采用 §9 自绘方案）。
5. `Theme::global_mut` 语义令牌字段全集（对照 Argus `theme.rs:198-289` 逐项映射）。
6. Windows：托盘 `tray-icon` 0.19 与事件循环集成不变；HWND 路径全量手测（置顶、DWM 圆角、IME、黑名单对话框前台激活）。
7. macOS：`objc2` 侧栏窗口、托盘、`NSAppearance` 跟随全量手测。
8. `cargo tree -d` 检查 windows/windows-sys 重复版本，记录差异。
9. 删除 `gpui_transitions` 后 `use_keyed_transition` 的 13 个文件逐个改 `motion::transition`（键语义一致：epoch/generation 防串台，保留现有 `hint_generation` 之类防悬停残影的处理）。

### 11.5 分支与开关策略

- M0 单独分支 `redesign/deps`，合并门禁 = 编译 + 全部单测 + 三窗口手测清单。
- 视觉阶段按里程碑分支 `redesign/m1…m6`，每阶段固化截图基准（浅/暗 × 100/125/150%）入 `docs/ui-redesign/baseline/`。
- Mica（P3）用设置项 `window_backdrop = "auto" | "off"` 门控，默认仅 Win11 22H2+ 且非远程会话开启。

## 12. 质量门禁

| 门禁 | 实现 |
|---|---|
| 颜色/圆角/间距/字号 lint | `scripts/ui_design_lint.py`（rg 驱动，规则 = requirements.md AC-1~AC-9），CI 与 pre-push 钩子 |
| 截图对照 | 每阶段基线目录 + 比对脚本（像素 diff ≤ 0.1% 视为通过；DPI × 主题矩阵） |
| 性能 | 主列表 1000 条滚动帧率对比脚本；`cargo build --release` 体积阈值 |
| 单测 | 现有 ui 单测全保留；新增：palette 缓存命中、token 解析、segments 锚点、motion 曲线端点值 |

## 13. Argus 对照速查

| 本文档章节 | Argus 位置 |
|---|---|
| §2 令牌表 | `resources/theme.json` |
| §3.1 Palette/缓存/remainder/on_card | `frontend/theme.rs:9-197` |
| §3.2 apply() | `frontend/theme.rs:198-289` |
| §3.3 ProductSurface | `frontend/theme.rs:290-320` |
| §4 排版/tnum/fallbacks | `frontend/theme.rs:327-490` |
| §6 动效曲线/mix | `frontend/theme.rs:512-597` |
| §7 图标登记 | `frontend/assets.rs` |
| §8.1 focus_ring | `frontend/widgets.rs:443` |
| §8.2 flyout_shadow | `frontend/widgets.rs:1774` |
| §9 组件规格 | `frontend/widgets.rs:650/917/1028/1130/1167`、`frontend/pages/settings.rs:217` |
| §10.2 导航指示器滑移 | `frontend/layout.rs:196` |
| §11.3 引导顺序 | `frontend.rs:742-808` |
| §12 重绘范围规约 | `src-gpui/AGENTS.md` 第 11 条 |
