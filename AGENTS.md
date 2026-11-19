# AGENTS.md

本文件约束 AI 代理在本仓库执行任务时的行为，重点是**与上游同步、处理合并冲突**。

## 1. 仓库事实

| 名称 | 地址 | 用途 |
|---|---|---|
| `origin` | `git@github.com:mecoren/clippi.git` | 本项目 fork，可推送 |
| `upstream` | `git@github.com:Ruszero01/clippi.git` | 上游原始仓库，**只读**（push URL 已禁用） |

分支职责：

- `wait`：**`origin` 仓库的默认分支，也就是本项目的主分支**。所有本地修改最终落在这里；`clone` 后默认检出的就是它；也是所有特性分支的拉出基线和合回目标；向本项目提 PR 时基线选 `wait`。
- `main`：**仅作为上游分叉点，与 `upstream/main` 保持完全一致**。三条禁令：不接受任何本地提交、不承载本地修改、不作为合并目标。只允许 `git merge --ff-only upstream/main` 向前移动，唯一用途是「与上游对齐的参照点」和「向上游回馈时从这里拉分支」。
- `feat/*`：短期特性分支，从 `wait` 拉出，合回前 rebase 到最新 `wait`。

分支关系是一次性的分叉：`upstream/main` → `origin/main`（镜像）→ 当初从 `main` 分出 `wait`。此后 `main` 不再向任何分支合并，`wait` 也不再回头依赖 `main`，两条线只通过「`main` 跟随上游、`wait` 合并 `main`」单向连接。

自检：`git ls-remote --symref origin HEAD` 应返回 `ref: refs/heads/wait`。若返回 `main`，说明 GitHub 上的默认分支设置没生效，先改设置再按本文操作。

若发现 `main` 上有本地提交，说明违反了上述约定：先确认那些提交在 `wait` 上都有（没有就 cherry-pick 过去），再把 `main` 重置回上游 `git checkout main && git reset --hard upstream/main`。`reset --hard` 前必须确认没有只存在于 `main` 的提交。

## 2. 核心优先级

发生冲突时，按此顺序取舍，不得颠倒：

1. **P0**：本项目的现有行为、数据格式、用户可见交互不被破坏。
2. **P1**：上游的功能改进、缺陷修复、安全修复不被丢弃。
3. **P2**：合并结果尽量小，不顺手重构无关代码。

「以本项目为准」指的是**逻辑与接口**以本项目为准，不是把上游改动整块丢掉。

## 3. 同步上游的标准流程

```bash
git fetch upstream --prune

# 第一步：main 镜像上游，不跑测试、不做任何本地改动
git checkout main
git merge --ff-only upstream/main
git push origin main

# 第二步：同步到本项目主分支 wait，这里才跑校验
git checkout wait
git merge main
git push origin wait
```

- 第一步只处理 `main`：完成后 `git rev-parse main upstream/main` 必须输出同一个 sha。`main` 的更新纯粹为了让 fork 与上游对齐，不影响构建，也不需要跑测试。
- 第二步才是本项目的工作：`wait` 合并 `main`。`git merge upstream/main` 结果等价，二选一即可，但别两条都执行。
- 无冲突：验证后直接完成合并提交，不额外改写上游代码。
- 有冲突：按第 4 节处理。
- 禁止对 `wait` / `main` 使用 `git rebase`（这两个分支已推送）；禁止 `git push --force`。
- 禁止对 `upstream` 执行任何 push。
- 不要用 GitHub 网页上的「Sync fork」按钮：它按**同名分支**同步，而 `wait` 在上游不存在，点了只会失败或误操作。同步一律走上面的命令行。
- 仓库已启用 `rerere.enabled`，历史上解决过的相同冲突会被自动复用，注意核对自动结果是否仍成立。

## 4. 冲突处理决策表

合并方向是 `upstream/main` → `wait`，因此：

- `--ours` = **本项目**（`wait`）
- `--theirs` = **上游**（`upstream/main`）

| 冲突情形 | 处理方式 |
|---|---|
| 上游新增文件，本项目未碰过 | 直接采用上游版本 |
| 上游删除/重构的文件，本项目仍在用 | 保留本项目版本，必要时迁到本地独立模块 |
| 双方都改了同一文件 | 逐块合并：本项目逻辑优先；上游的 bugfix、安全修复、崩溃修复必须手工叠加进去 |
| 双方新增了同名文件/模块 | 保留本项目实现；上游实现若确有价值，改为独立模块接入，不平铺直搬 |
| `Cargo.toml` / `Cargo.lock` | 以上游为基线，重新叠加本项目新增依赖，然后跑 `cargo check` 重新生成锁文件 |
| 上游新增功能，本项目没有 | 不做 `git checkout --theirs`，按第 5 节重写该功能 |

补充规则：

- **禁止用 `-X ours` / `-X theirs` 作为全局合并策略**。仅允许在单个文件、且逐块确认过内容后使用。
- 每个冲突文件解决完，用 `git diff --staged <file>` 复核一次，确认没有把上游整段回退、也没有留下 `<<<<<<<` / `>>>>>>>` 标记。
- 解决过程中若发现上游把本项目某个改动做成了官方实现，记录到提交正文，后续可删除本地的重复实现。

## 5. 上游功能的移植规范

上游有、本项目无、且直接合并会冲突的功能，**必须按本项目规则重写，不得整段复制**。

1. 先读懂意图：`git log --oneline upstream/main ^wait` 找到相关提交，用 `git show <sha>` 弄清它的行为契约（输入、输出、边界、副作用），而不是照抄实现。
2. 落到本项目分层，不新增层级：
   - `src/core/`：与 UI 无关的纯逻辑、数据结构、路径与配置。
   - `src/services/`：系统能力与后台服务（剪贴板轮询、更新、同步等）。
   - `src/platform/`：平台差异实现，按 `*_windows.rs` / `*_macos.rs` 拆文件。
   - `src/ui/`：gpui 组件与窗口。
3. 平台差异一律下沉到 `src/platform/`，不要在业务代码里散落 `#[cfg(target_os = "…")]`（测试里用于限定平台的 `#[cfg(target_os = "…")]` 除外）。
4. 沿用本项目的命名、错误处理、日志与模块注册方式（新模块要在 `src/core/mod.rs` 等处显式 `pub mod`）。
5. 不引入上游的辅助宏、工具函数、新依赖；确有必要新增依赖时，在提交正文说明理由。
6. 配置字段、持久化 key、i18n 文案、默认值都沿用本项目方案，禁止直接搬上游字面量。
7. 上游功能与本项目已有实现重复时：合并成一份，保留本项目的对外接口，内部吸收上游更优的实现。
8. 提交正文必须写明来源，例如：`port upstream@1a2b3c4: quick window 焦点恢复`。

## 6. 项目约定（合并与移植时必须遵守）

### 6.1 构建渠道隔离（硬约束）

- 渠道只由 `core::channel::is_release()` 判定，值来自 `build.rs` 嵌入的 `CLIPPI_CHANNEL`（`GITHUB_ACTIONS=true` 或 `CLIPPI_RELEASE=1` 才是 release）。
- **禁止**改用 `cfg!(debug_assertions)` 或 `#[cfg(debug_assertions)]` 判断渠道：本地 `--release` 构建仍是 dev 构建。
- release 渠道：数据目录 `cn.wait.clippi`、单实例端口 `19876`；dev 渠道：`cn.wait.clippi.dev`、端口 `19877`。不得为调试方便改端口或目录名。
- `src/core/paths.rs` 的 `LEGACY_APP_DIR_NAMES` 用于旧目录数据迁移，改目录名时只能追加，不能替换。

### 6.2 国际化

- 所有用户可见字符串必须经 `define_i18n!` 登记在 `src/core/i18n_keys.rs`，格式 `KeyName: ("中文", "English")`，通过 `I18nKey::KeyName.text()` 使用。
- 禁止在 `src/ui/` 里硬编码中文或英文字面量；上游功能移植时要先抽出 key，再把文案填入。

### 6.3 UI

- 颜色、圆角、间距、字号优先取 `src/ui/theme.rs` 的令牌，新代码不新增硬编码颜色字面量。历史遗留的大量散落字面量见 `docs/ui-design-review.md`，处理时不要顺势扩大。

### 6.4 依赖与构建配置

- 项目没有 Cargo features，不要为分支逻辑新增 feature flag；平台差异写在 `[target.'cfg(target_os = "…")'.dependencies]`。
- 新增依赖需在提交正文说明理由；不得改动 `[profile.release]`（`opt-level = "s"`、`lto`、`strip`）与 `[profile.dist]`。
- 日常验证用 `cargo check`；`--release` 带 LTO，仅发布与最终验收时使用。

### 6.5 测试

- 测试与被测代码同文件：`#[cfg(test)] mod tests` + `use super::…`；平台相关测试用 `#[cfg(target_os = "windows")]` 等标注。
- 新增纯逻辑（解析、路径、过滤、合并策略）必须补测试；不要引入测试框架依赖。

### 6.6 平台

- Windows 是主验证平台。改动落在 `*_macos.rs`、`src/platform/ocr_helper.m` 等 macOS 专属代码时，必须在提交正文注明未经 macOS 实测。

### 6.7 文档与敏感数据

- `README.md` 与 `README_EN.md` 必须同步修改，且改动不要落在 bot 自动刷新的贡献墙段落。
- 剪贴板内容属于用户敏感数据：禁止写入日志、提交信息、测试夹具或文档示例，测试用自造假数据。
- 版本号、`RELEASES.md`、`release.yml` / `publish-oss.yml` / `contributors.yml`、`dist-workspace.toml`、`installer.nsi` 由发布流程维护，非任务要求不要改动。`ci.yml` 是本地校验门禁（见第 8 节），不归发布流程管，改校验规则时改它。

### 6.8 开关组件与动画（硬约束）

- 所有"开/关"控件一律使用 `src/ui/components/wait_switch.rs` 的 `WaitToggleSwitch`（`new` / `value` / `disabled` / `on_change`）。禁止自绘开关，禁止用无动画的 checkbox 或普通按钮代替。
- 开关切换只能由**一个进度值**驱动：用 `gpui_kit::base::motion::transition` 对 `0`/`1` 插值，滑块位移、轨道底色、滑块色与两处描边全部由该进度经 `design::motion::mix` 派生。不允许"滑块在动、颜色跳变"。
- 时长与曲线固定用 `design::motion::transition_slow`（`t-slow` 250 ms + `ease-move`），它自带"减少动态效果"的瞬时退化；页面层不得自写缓动、`with_animation` 或另设时长。
- 后续新增的开关类控件（复选式开关、切换按钮、分段式开关等）必须沿用同一套过渡参数与 `<id>:toggle-thumb` 的进度槽约定；需要额外视觉变化时，把它写成该进度的函数，而不是另起一段动画。
- 开关的 `id` 必须在同一视图内唯一，否则多个开关会共用同一个动画进度槽。

## 7. 提交信息规范

- 合并上游：`merge: sync upstream/main <起..止 sha>`，正文列出冲突文件与每处取舍结论。
- 本地功能：`wait: <描述>`。
- 移植上游功能：`wait: port <上游 sha> <功能>`，正文说明与上游实现的差异。
- 前缀历史：`a40060c` 及更早的本地提交使用旧前缀 `local:`，不回改（`wait` 已推送，禁止 rebase / force push），阅读历史时视为同类；新提交一律用 `wait:`，禁止再用 `local:`。
- 一个提交只做一件事；合并提交里不要夹带新的功能开发。

## 8. 提交前校验

全部通过才算完成；`cargo fmt`、`cargo check`、`cargo test` 必须零失败，`cargo clippy` 以不新增告警为准（历史遗留告警不在合并任务范围内，顺手修也可以，但不要为此扩大改动面）：

```bash
cargo fmt
cargo clippy --all-targets
cargo check
cargo test
```

另外：

- 主验证平台为 Windows，涉及平台差异的改动需说明另一平台是否受影响。
- 推送 `wait`（或向 `wait` 开 PR）会触发 `.github/workflows/ci.yml`：`windows-latest` 上依次跑 `cargo fmt --all -- --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`。它是上面那份校验的远端版本，红了就是没完成；工具链在文件里锁版本（新 stable 会带来新 clippy lint），调整时与本地同步。
- 合并后执行一次 `git diff --stat wait@{1} wait`，确认改动规模与预期一致，没有意外的大面积回退。
- 不要提交 `target/`、临时脚本、调试输出。

### 冒烟验证

GUI 无法自动测试，合并后必须人工过一遍，并在此清单上逐项确认：

- 启动无 panic；`Alt+V` 能唤起主窗口，窗口动画与圆角正常。
- 复制一段文本、一张图片各一次，历史列表新增条目且预览正确。
- 回车/双击粘贴到外部应用成功；Windows 下粘贴后焦点回到原窗口。
- 托盘菜单可打开，设置页改动能保存，重启后配置与历史仍在。
- 重复启动不产生第二个进程（验证单实例端口生效）。
- 开发构建的数据目录是 `cn.wait.clippi.dev`，没有读写到正式版的 `cn.wait.clippi`。

## 9. 必须停下来询问的情况

遇到以下情况不要自行决定，先向用户确认：

- 需要删除或改变本项目已有的功能、配置项、数据存储格式。
- 需要改变对外协议、端口、安装/发布流程。
- 上游与本地实现语义冲突且无法同时满足，必须放弃一侧。
- 冲突范围超出预期（例如超过 20 个文件）或涉及核心数据结构。
- 需要改变 `wait` / `main` 的分工、默认分支，或把 `main` 当作开发分支使用。

## 10. 关于本文件

本文件是本项目独有的约定，上游没有对应文件，正常合并不会产生冲突。合并时不要删除它；若上游日后引入了同名文件，按第 4 节「双方新增同名文件」处理，保留本项目版本。
