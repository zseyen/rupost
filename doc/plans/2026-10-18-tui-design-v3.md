# TUI 设计 v3.1：架构还债与迭代规划（含对抗性审查修订）

- **状态**：待评审（Stage 0/1 产出；v3.1 完成对抗性审查修订，v3.2 完成项目最新实践对照审查——两轮记录均见第八节）
- **日期**：2026-10-18
- **关联文档**：`doc/plans/tui_iteration_v2.md`（已交付，未回写进度）、`doc/tui/implementation_plan.md`、`doc/tui/keybindings.md`、`doc/tui/plugin_interface.md`、`doc/plans/progress_summary.md`、`doc/dev/development_workflow.md`
- **原则**：Clean Architecture / DRY / TDD（红-绿-重构）/ YAGNI

---

## 一、当前进度盘点（基于代码核实，非文档转述）

### 1.1 代码现状

| 文件 | 行数 | 职责 |
|------|------|------|
| `src/tui/state.rs` | 1033 | AppState（ViewModel）：布局计算、预折行缓存、文件树、历史、日志 GC、快速输入 |
| `src/tui/handlers.rs` | 810 | 全部按键/鼠标分发 + 单测 |
| `src/tui/app.rs` | 486 | 事件循环（Tokio MPSC select）+ 单测 |
| `src/tui/ui/*` | 1245 | 纯渲染（mod/response/sidebar/editor），TestBackend 可测 |
| `src/tui/event.rs` | 43 | `TuiEvent` + `Action` |
| 合计 | 3627 | |

### 1.2 已交付特性（全部有提交与测试佐证）

- 自适应四档布局、Tab/Shift+Tab 焦点轮转、极小屏防御（R1）
- 非阻塞执行、SSE/WS 流式渲染、历史滑动窗口加载（R2/R3）
- 脏修改弹窗拦截、外部编辑器 `E` 键闭环（R4 + v2）
- **tui_iteration_v2 三特性已实现**（文档未回写）：Detail Tabs（h/l 切换，`handlers.rs:438-474`）、侧边栏一键运行（`trigger_request_execution`，`handlers.rs:291`）、`n` 键快速新建用例 + URL 智能推导文件名（`state.rs:503-600`）
- 文件树目录折叠、运行 spinner、并发锁、外部文件 hot-reload、日志 5MB 限额 + 7 天 GC + LRU 修剪

### 1.3 测试基线（2026-10-18 实跑验证）

- `cargo test --lib tui`：**13 通过**；`cargo test --test tui_smoke_test`：**39 通过**；全仓 241+ 测试绿。
- 提交轨迹健康：`13a632f0`（设计 + 冒烟桩）→ 4 个特性原子提交（8f6c36f5 → 53efadb9），符合 TDD 节奏。

### 1.4 与文档的漂移

- `tui_iteration_v2.md` 记载的缺口**已修复**，但 progress_summary 未登记。
- `doc/tui/keybindings.md` 记载大量**未实现**按键（主题切换、搜索过滤、设置面板、文件树 a/d/y/x 操作、Ctrl+N/T/O、JSON 折叠、Vim 编辑模式）——文档承诺远超实现。

---

## 二、用户故事（Stage 0 合规）

**US-T1 快速新建用例**
- Given 用户在 TUI 按 `n` 输入 `api.github.com/users`
- When 按 Enter
- Then 在当前选中目录生成 `api_github_com_users.http`，内容为 `GET https://api.github.com/users`，侧边栏树刷新且光标定位到新文件
- And 写入失败（权限/磁盘满/名称超长）时在状态区显示可读错误，而非静默无反馈

**US-T2 输入安全边界**
- Given 用户输入 `../../outside.http` 或任何试图逃逸工作区的路径
- When 按 Enter
- Then 拒绝创建并提示"路径必须位于工作区内"，工作区外无任何文件被写入

**US-T3 URL 判定一致性**
- Given 用户在 CLI 输入 `:3000/x` 或 `localhost:3000/x`，或在 TUI 快速输入同样内容
- When 解析
- Then 两处得到完全相同的 URL 归一化结果（同一谓词、同一函数）

**US-T4 搜索过滤**
- Given 侧边栏有 100+ 文件
- When 按 `/` 输入子串
- Then 文件树即时过滤为匹配项，Esc 恢复全量视图

---

## 三、问题清单（架构 / DRY / 安全 / 流程）

| # | 维度 | 问题 | 证据 |
|---|------|------|------|
| A1 | 架构 | **ViewModel 写路径直接做文件 I/O**：`create_test_from_input` 在 Core 层 `std::fs::write` + `canonicalize`，无法离线单测 | `state.rs:574-585` |
| A2 | DRY | **URL 归一化重复且更弱**：手写 `https://` 补全 + `contains('.')` 启发；不认 `ws://`、不支持 `:port` 缩写、`v1.2/users` 误判为 URL | `state.rs:509-524` vs `http/url.rs:7-15` |
| A3 | 架构 | **God ViewModel**：state.rs 1033 行混杂 6 类职责 | `state.rs` |
| A4 | 架构 | handlers.rs 810 行单文件分发三面板 + 弹窗 + 快速输入 | `handlers.rs` |
| A5 | 流程 | 文档-实现漂移（keybindings.md 承诺未实现按键） | 见 1.4 |
| A6 | TDD | 快速输入框手写字符处理，多字节字符边界无测试（`ratatui-textarea` 依赖已在 Cargo.toml，可直接复用） | `handlers.rs:6-26` |
| A7 | 安全 | **路径穿越写**：普通文件名分支无 jail 检查，`../../evil.http` 可写到工作区外（项目在 DAG 已有沙箱先例，TUI 缺失） | `state.rs:559-577` |
| A8 | 安全/UX | **写失败静默吞错**：`let _ = std::fs::write`，EACCES/ENOSPC/只读挂载无任何反馈；`exists()` 无法区分"不存在"与"无权探测" | `state.rs:575-577` |
| A9 | 健壮性 | **文件名无长度上限**：host+path sanitize 后可超 255 字节，写入必失败（且被 A8 吞掉） | `state.rs:527-545` |
| A10 | 架构 | **读路径 I/O 散布**：app.rs 与 handlers.rs 多处 `read_to_string`/`metadata`/`canonicalize`（本设计受控保留，登记为债务，见决策 2） | `app.rs:74-242`、`handlers.rs:109-669` |

---

## 四、核心决策

### 决策 1：渐进式"重写式重构"，不做大爆炸重写

重写允许，但**不推倒重来**：
- 39 个冒烟测试沉淀了大量隐性修复（父路径黑名单泄漏 `8855a872`、1000 行截断防渲染锁死 `2e1dbc76`），重写等于丢弃回归资产；
- 事件回路（app.rs）与渲染层（ui/*）分层清晰，无需重写；
- 债务集中在 state.rs / handlers.rs，按模块拆分 + 端口化即可。

### 决策 2：端口仅覆盖**写路径**，读路径 I/O 登记为受控债务（修 A1，限界 A10）

新增 `src/tui/ports.rs`：

```rust
/// TUI 写路径端口（依赖倒置：Core 只见抽象）
/// 返回 io::Result 而非 bool，让权限/磁盘错误可浮出到状态区（修 A8）
pub trait FileRepository: Send + Sync {
    fn write_file(&self, path: &Path, content: &str) -> std::io::Result<()>;
    fn try_exists(&self, path: &Path) -> std::io::Result<bool>;
    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf>;
}
pub struct FsRepository;                    // Adapter：真实文件系统
pub struct InMemoryRepository { /* Mutex<HashMap<PathBuf, String>> */ } // 测试替身
```

- **范围限界**：只改造 `create_test_from_input` 这一条写路径。app.rs/handlers.rs 的读路径 I/O（加载文件、mtime 探测、hot-reload）**本迭代不动**——它们已被 tempfile 冒烟测试离线覆盖，全量端口化是范围爆炸，登记入第九节债务清单。
- **构造兼容**：`AppState::new()` 保持无参签名（默认注入 `FsRepository`），新增 `AppState::with_ports(repo: Arc<dyn FileRepository>)` 供单测注入——39 个冒烟测试零改动。
- `InMemoryRepository.canonicalize` 为身份映射（不做 symlink 解析），此局限在 trait 文档注释中显式声明。
- 写失败不再吞错：错误写入 AppState 新增的 `last_error: Option<String>` 状态字段，ui 状态区渲染（对接 US-T1）。

### 决策 3：URL 推导复用规范实现（修 A2）

- `has_scheme_prefix` 提升为 `pub(crate)`；
- `http/url.rs` 新增纯函数与枚举：

```rust
pub enum NormalizedInput { Absolute(String), LocalhostShorthand(String), HostName(String), NotUrl(String) }
pub fn normalize_input_url(input: &str, default_scheme: &str) -> NormalizedInput
```

  （枚举用 `NotUrl` 而非 `Relative`——TUI 语境下非 URL 输入是"文件名"，不是相对 URL。）
- **判定决策表**（Phase 0 特征测试逐行锁定）：

| 输入 | 判定 | 归一化结果 |
|------|------|-----------|
| `https://a.com/x` / `ws://a.com` | Absolute | 原样 |
| `:3000/x` | LocalhostShorthand | `http://localhost:3000/x` |
| `localhost:3000/x` | HostName | `http://localhost:3000/x` |
| `api.github.com/users` | HostName | `https://api.github.com/users` |
| `v1.2/users` | NotUrl | 文件名 `v1.2/users.http`（**修正现误判**） |
| `tests/auth.http` | NotUrl（`.http` 后缀排除） | 文件名原样 |
| `my notes`（含空格） | NotUrl | 文件名 `my notes.http` |
| `my.file` | HostName | `https://my.file`（现状如此，锁定；争议留 backlog） |

- CLI、TUI quick-input 两处共用同一函数，删除 state.rs 手写判断。

### 决策 4：state.rs / handlers.rs 按职责拆分（修 A3/A4/A6）

```
src/tui/
├── mod.rs / app.rs / event.rs        # 不变
├── ports.rs                          # ★ 新增（决策 2）
├── state/
│   ├── mod.rs        # AppState 聚合根（pub use 委托，对外 API 不变）
│   ├── layout.rs     # 布局断点 + response_width 计算（纯函数）
│   ├── wrap.rs       # 预折行缓存与滑窗（纯函数 + 缓存失效）
│   ├── files.rs      # 文件树模型 + 用例创建（经 ports 落盘 + 安全检查）
│   ├── history.rs    # 历史/日志模型
│   └── retention.rs  # 限额 / GC / LRU 修剪（纯函数）
├── handlers/
│   ├── mod.rs        # 总分发：弹窗 > quick_input > 全局 > 面板
│   ├── global.rs     # q / ? / Tab / BackTab / E / Esc / 弹窗
│   ├── sidebar.rs    # 文件树 + 历史 tab
│   ├── response.rs   # Detail tabs + 滚动
│   └── quick_input.rs# 复用 ratatui-textarea（已验证在依赖中；AppState: Clone 与 textarea Clone 兼容）
└── ui/               # 不变
```

约束：**对外行为零变化**——`AppState` 公共方法签名保持，ui 层与 app.rs 不感知拆分。

### 决策 5：写路径安全加固（修 A7/A8/A9，对接 US-T2）

在 `derive_test_file` 与落盘之间加两道防线：
1. **工作区围栏（jail）**：目标路径 `canonicalize` 后必须落在 `workspace_root` 根之下，否则拒绝并在状态区提示。**复用而非重写** DAG 沙箱实现：`DependencyResolver::validate_sandbox`（`src/runner/engine/resolver.rs:93`，多根 allowlist + canonical `starts_with` 判定）——将其核心判定提取为 `src/utils` 下的 `pub(crate) fn is_within_roots(path, roots) -> bool`，resolver 与 TUI ports 共用同一实现（DRY）。注意沿用 resolver.rs:39 的诚实口径：这是正确性护栏而非安全边界；TUI 场景下它保护的是用户文件系统，拒绝时必须给出可见提示。
2. **文件名截断**：sanitize 后超过 80 字符时截断并追加 8 位短哈希后缀，保证 `<255` 字节且不破坏扩展名。
3. 写入结果 `io::Result` 一路浮出到 `last_error`，成功时清除。

三者均为纯函数可测（jail 判定、截断算法），先写红测试再实现。

---

## 五、TDD 实施计划

> 每阶段：先写失败测试（红）→ 最小实现（绿）→ 重构 → `cargo fmt && clippy -D warnings && cargo test` → jj 原子提交。

### Phase 0 — 特征测试安全网（0.5d）

锁定现有正确行为 + 以红测试声明新行为（安全三防线天然 TDD）：

| 用例组 | 断言要点 |
|--------|---------|
| 文件名推导 | `google.com` → `google_com.http`；带路径 URL；`tests/auth.http` 原样；空输入 no-op |
| 落盘位置 | 选中目录 → 目录自身；选中文件 → 父目录；无选中 → workspace_root |
| **路径穿越（红）** | `../../evil.http`、绝对路径输入 → 拒绝 + 工作区外零写入（US-T2） |
| **失败浮出（红）** | InMemory 返回 EACCES → `last_error` 非空（US-T1） |
| **超长截断（红）** | 300 字符 path URL → 生成名 ≤ 88 字符且含哈希后缀 |
| 不覆盖 | 已存在同名文件不覆盖、光标仍定位 |

**验收**：新用例全绿（红→绿完成）；`verify_features.sh` + `run_all.sh` 100% 通过。

### Phase 1 — 架构还债（1.5d，6 个原子提交）

1. `feat(tui): add FileRepository port with Fs/InMemory impls`（ports.rs + 单测）
2. `fix(tui): confine quick-input file creation to workspace and surface write errors`（安全三防线；含提取 `utils::is_within_roots` 共用判定，修 A7/A8/A9）
3. `refactor(tui): unify URL normalization via http::url::normalize_input_url`（修 A2，删手写判断）
4. `refactor(tui): split state.rs into state/ domain modules`（修 A3）
5. `refactor(tui): split handlers.rs into handlers/ modules`（修 A4）
6. `refactor(tui): adopt ratatui-textarea for quick input`（修 A6）

**验收**：
- 全量测试绿（模块拆分后测试路径变化属预期，需同步 `#[path]` 无关的 `use` 修正）；
- `grep -rn "std::fs" src/tui/state/` **写路径零命中**（读路径保留并已登记债务）；
- `AppState::new()` 签名未变，39 个冒烟测试文件零改动；
- clippy `-D warnings` 通过。

### Phase 2 — 文档对齐（0.5d，Stage 6 合规）

- `keybindings.md` 按实现裁剪，未实现项移至"规划中"并标注 Phase；
- `progress_summary.md` 回写：tui_iteration_v2 三特性 + 本设计各阶段交付；
- `tui_iteration_v2.md` 头部标注"已交付"；
- **`checkpoint.md` 归档本次重构状态；README 若涉 TUI 能力表述则同步**（v3 遗漏，此处补齐）；
- **`doc/architecture/directory_structure.md` 的 tui 小节（mod/app/event/handlers/state 平铺描述）在 state//handlers/ 拆分后即失效，必须同步改写**（v3.1 遗漏，v3.2 补齐）。

**验收**：文档无未实现承诺；四个文档状态一致。

### Phase 3 — 功能迭代（2-3d，按价值择优）

| 优先 | 特性 | 理由 |
|------|------|------|
| P1 | 文件树/历史 `/` 过滤搜索（US-T4） | 大目录导航是当前最大痛点；纯 state 过滤，最易 TDD |
| P2 | 主题最小版（`,` 切换 dark/light 调色板） | 兑现 keybindings.md 承诺；ui 纯函数，TestBackend 可测 |
| P3 | 设置持久化（`~/.rupost/tui.toml`：主题、布局记忆） | 小体量，复用 config 通道 |
| 缓期 | 语法高亮 / 智能补全 / 多工作区 | YAGNI，入 backlog |

### 非目标（本迭代明确不做）

- TUI 插件接口实现（保留设计，与"插件系统排最后"战略一致）；
- Ctrl+N 快速请求面板（与 `n` 键职责重叠，先观察使用）；
- 内置编辑器 Vim 模式、JSON 折叠；
- 读路径 I/O 全量端口化（见第九节债务清单）。

---

## 六、风险与回滚

| 风险 | 缓解 |
|------|------|
| 拆分引发隐性回归（折行缓存失效、GC 时机） | Phase 0 特征测试先行；每提交全量测试 + 双回归脚本 |
| AppState 拆分波及 ui 层访问路径 | `state/mod.rs` 聚合根 `pub use` 委托，路径不变 |
| URL 语义统一改变 TUI 边界行为 | 决策表逐行锁定；`:port` 支持、`v1.2` 修正是**修正误判**，提交信息注明 |
| 范围蔓延 | 每特性独立可交付；Phase 1 六提交各自可独立回滚 |
| `my.file` → URL 的争议行为 | 特征测试锁定现状，不在本迭代翻案，backlog 留证 |

---

## 七、债务登记（本迭代明确不修，留待后续）

| 债务 | 来源 | 触发条件 |
|------|------|---------|
| 读路径 I/O 端口化（app.rs/handlers.rs 六处） | 决策 2 限界 | TUI 需要非文件系统工作区（如 remote）时 |
| 默认模板硬编码 `httpbingo.org`（离线即失败） | 现状 | 提供本地 mock 默认模板时 |
| `my.file` 判定为 URL 的语义争议 | 决策 3 表 | 用户反馈 |
| keybindings.toml 自定义按键（文档承诺） | Phase 3 缓期 | 设置持久化落地后 |
| TUI 非测试代码约 14 处 `unwrap()/expect()` | 现状扫描 | 按质量加固战役口径（对照 `nusvywluxqvt` 锁中毒改映射）逐步清零，不阻塞本迭代 |
| `is_loading`/`loading_tick` 等 spinner 状态与业务状态混存 | 拆分期顺手观察 | 若 Phase 3 引入更多异步任务（如搜索防抖）再抽离 |

---

## 八、对抗性审查记录（v3 → v3.1）

审查方法：对照实际代码核实设计前提 + 从安全/范围/流程/一致性四个维度攻击设计稿。结论：**主干成立（拆分策略、端口方向、TDD 阶段划分经攻击后保留），但存在以下被推翻或补强项**：

| # | 发现 | 等级 | 处置 |
|---|------|------|------|
| F1 | `AppState::new()` 无参签名被 39 个冒烟测试直接调用，原设计"构造注入"会击穿全部测试 | 推翻前提 | 改为 `new()` 默认 + `with_ports()` 注入（决策 2） |
| F2 | "ViewModel 无 I/O"名不副实：读路径 I/O 散布 app.rs/handlers.rs 六处，全量端口化是范围爆炸 | 修正范围 | 端口仅覆盖写路径，读路径登记债务 A10（决策 2） |
| F3 | 快速新建存在**路径穿越写漏洞**（`../../evil.http` 逃逸工作区），原设计未处理 | P0 安全 | 新增决策 5 围栏 + US-T2 红测试 |
| F4 | `let _ = fs::write` 静默吞错 + `exists()` 无法区分无权探测 | P1 | trait 改 `io::Result` + `last_error` 状态浮出（决策 2/5） |
| F5 | sanitize 后文件名可超 255 字节导致必败写 | P1 | 80 字符截断 + 哈希后缀（决策 5） |
| F6 | 原 Phase 1"提交 3"同时拆两个大文件并换 textarea，与自述"每步一个模块"矛盾 | 流程缺陷 | 重排为 6 个原子提交 |
| F7 | 枚举 `Relative` 命名语义错误（TUI 语境是文件名） | 设计缺陷 | 改 `NotUrl` + 判定决策表 |
| F8 | 缺 Stage 0 要求的 Given-When-Then 用户故事 | 流程合规 | 补第二节 US-T1~T4 |
| F9 | Phase 2 漏掉 checkpoint.md 与 README 的 Stage 6 同步义务 | 流程合规 | 补齐（Phase 2） |
| F10 | textarea 复用可行性未验证即写入设计 | 事实风险 | 已核实 `ratatui-textarea = "0.9"` 在 Cargo.toml、AppState 为 Clone 兼容 |

未发现需要推翻决策 1（渐进重构）、决策 4（模块边界）的攻击路径。

### 审查第二轮：对照项目最新实践（v3.1 → v3.2）

方法：以质量加固执行计划（同日 `2026-10-18-quality-hardening-execution-plan.md`）与近 20 个 jj 提交为基准逐条对照。

| # | 对照项 | 结论 | 处置 |
|---|--------|------|------|
| F11 | 决策 5 原写"复用 DAG 沙箱判定语义"——实际实现已固化：`DependencyResolver::sandbox_roots_from_inputs` + `validate_sandbox`（多根 allowlist，正是加固计划 Phase 2 废弃 LCA 方案后的产物） | 对齐但不够 DRY | 明确提取 `utils::is_within_roots` 共用函数（决策 5、Phase 1 提交 2） |
| F12 | 沙箱注释口径：resolver.rs:39 自述"非安全边界" | 设计需诚实沿用口径 | 已并入决策 5 表述 |
| F13 | Phase 2 文档同步清单漏掉 `directory_structure.md`——归档提交刚更新过 tui 小节，拆分后必失效 | 文档缺口 | 已补进 Phase 2 |
| F14 | TUI 非测试代码 14 处 `unwrap()/expect()`，与加固战役"消灭 panic"方向相悖 | 债务 | 登记债务表，不阻塞迭代 |
| F15 | 提交信息惯例核验：近期 log 均为 `type(scope): 祈使句`（含 `docs(plans):`、`refactor(ws):`），本计划 6 提交与文档提交信息符合 | 通过 | 无需改 |
| F16 | A7（路径穿越写）与战役近期 `kovyvuoozpqt fix(cookie): reject path-traversing --env names` 同类同向，拒绝+可见错误的模式已有先例 | 设计方向正确 | 决策 5 保持 |
| F17 | "先还债后功能"顺序与加固计划 Phase 4"前置 SseRunner 去重避免复制第三份重复块"同原则 | 通过 | Phase 1 → Phase 3 顺序保持 |

第二轮审查同样未推翻任何主干决策；四项修订均已回写正文。
