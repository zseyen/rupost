# TUI 设计 v3.1：架构还债与迭代规划（含对抗性审查修订）

- **状态**：Phase 0/1 已执行并经第四轮三代理代码审查修复（2026-10-25，jj workspace `rupost-tui`：端口化、安全三防线、URL 统一、textarea/InputMode、state/ 首片拆分；state 剩余域与 handlers 拆分留下一片）；v3.1 对抗性审查、v3.2 实践对照审查、v3.3 执行细化与第三轮查漏记录见第八、九、十节，第四轮记录见第十一节
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
/// v3.3 修订（G1/G2）：exists()+write() 存在 TOCTOU 竞态，canonicalize 无法作用于
/// 尚未创建的目标文件——改为原子 create_exclusive 与"规范化父目录"两个原语。
pub trait FileRepository: Send + Sync {
    /// 原子创建并写入；文件已存在时返回 io::ErrorKind::AlreadyExists（不覆盖），
    /// 权限/磁盘错误原样浮出（修 A8）
    fn create_exclusive(&self, path: &Path, content: &str) -> std::io::Result<()>;
    /// 规范化已存在路径（workspace_root、已存在的父目录）
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
| `v1.2/users` | HostName | `https://v1.2/users`（实现期修正：与 CLI `is_independent` 同规则——首段含点即主机；原表写 NotUrl 有误） |
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

---

## 九、执行细化与示例代码（v3.3）

> 以下代码均为**示意实现**（贴合现有 API 核实结果），落地时以红测试为准。

### 9.1 Phase 1 提交 1：ports.rs（完整骨架）

```rust
// src/tui/ports.rs
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub trait FileRepository: Send + Sync {
    fn create_exclusive(&self, path: &Path, content: &str) -> std::io::Result<()>;
    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf>;
}

/// Adapter：真实文件系统
pub struct FsRepository;

impl FileRepository for FsRepository {
    fn create_exclusive(&self, path: &Path, content: &str) -> std::io::Result<()> {
        let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
        f.write_all(content.as_bytes())
    }
    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }
}

/// 测试替身：内存文件系统
#[derive(Default)]
pub struct InMemoryRepository(Mutex<HashMap<PathBuf, String>>);

impl FileRepository for InMemoryRepository {
    fn create_exclusive(&self, path: &Path, content: &str) -> std::io::Result<()> {
        let mut map = self.0.lock().unwrap();
        if map.contains_key(path) {
            return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists));
        }
        map.insert(path.to_path_buf(), content.to_string());
        Ok(())
    }
    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        Ok(path.to_path_buf()) // 身份映射：不做 symlink 解析（已知局限，trait 文档注明）
    }
}

pub type SharedRepo = Arc<dyn FileRepository>;
```

AppState 装配（39 个冒烟测试零改动）：

```rust
pub fn new() -> Self { Self::with_ports(Arc::new(FsRepository)) }
pub fn with_ports(repo: SharedRepo) -> Self { /* ... repo 字段初始化 ... */ }
```

### 9.2 提交 2：纯函数 + 安全三防线

文件名推导与截断（纯函数，表格驱动测试的直接对象）：

```rust
// src/tui/state/files.rs
pub struct DerivedTest { pub filename: String, pub content: String }

/// 输入 → (文件名, 模板内容)。URL 判定经 http::url::normalize_input_url。
pub fn derive_test_file(input: &str) -> DerivedTest {
    match normalize_input_url(input, "https") {
        NormalizedInput::Absolute(u) | NormalizedInput::HostName(u)
        | NormalizedInput::LocalhostShorthand(u) => DerivedTest {
            filename: format!("{}.http", sanitize(&u)),
            content: format!("GET {u}\n"),
        },
        NormalizedInput::NotUrl(name) => DerivedTest {
            filename: ensure_ext(name),
            content: "GET http://httpbingo.org/get\n".into(), // 债务：硬编码模板已登记
        },
    }
}

/// sanitize：URL → 合法文件名；超 80 字符截断并追加 8 位短哈希（修 A9）
fn sanitize(url: &str) -> String { /* map 非字母数字 → '_'；超限截断 + hash 后缀 */ }
```

jail 判定复用 resolver 提取的共用函数；目标文件尚不存在，故**规范化父目录再 join**（G2）：

```rust
// src/utils/path_guard.rs（从 resolver.rs:93 提取，两处共用）
pub fn is_within_roots(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| path.starts_with(r))
}

// files.rs 内：创建前的围栏检查（修 A7）
fn resolve_target_in_jail(repo: &dyn FileRepository, dir: &Path, filename: &str,
                          root: &Path) -> Result<PathBuf, String> {
    let canon_root = repo.canonicalize(root).map_err(|e| e.to_string())?;
    let canon_dir  = repo.canonicalize(dir).map_err(|e| e.to_string())?;
    let target = canon_dir.join(filename);
    if !is_within_roots(&target, &[canon_root]) {
        return Err("path must stay inside the workspace".into());
    }
    Ok(target)
}
```

改写后的 `create_test_from_input` 骨架（错误浮出修 A8）：

```rust
pub fn create_test_from_input(&mut self) {
    let Some(input) = self.take_quick_input() else { return };
    let derived = derive_test_file(&input);
    let dir = self.selected_target_dir();          // 选中目录/父目录/workspace_root
    match resolve_target_in_jail(self.repo.as_ref(), &dir, &derived.filename, &self.workspace_root)
        .and_then(|p| self.repo.create_exclusive(&p, &derived.content).map_err(|e| (p, e)))
    {
        Ok(()) => { self.last_error = None; /* 重建树 + 光标定位（现有逻辑保留） */ }
        Err((p, e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            self.last_error = None;                // 已存在不覆盖，仍定位（行为锁定）
            /* 光标定位到 p */
        }
        Err((_, e)) => self.last_error = Some(format!("create failed: {e}")),
    }
}
```

### 9.3 提交 3：URL 归一化（http/url.rs）

```rust
// src/http/url.rs 新增
pub enum NormalizedInput { Absolute(String), LocalhostShorthand(String),
                           HostName(String), NotUrl(String) }

pub fn normalize_input_url(input: &str, default_scheme: &str) -> NormalizedInput {
    if has_scheme_prefix(input) {
        return NormalizedInput::Absolute(input.to_string());
    }
    if input.starts_with(':') {
        return NormalizedInput::LocalhostShorthand( // 复用 resolve_final_url 的缩写语义
            resolve_final_url(input, &VariableContext::new(), "http", Some("cli"), None)
                .unwrap_or_else(|_| input.to_string()));
    }
    let looks_like_host = input.split('/').next().unwrap_or("").contains('.')
        && !input.contains(' ') && !input.ends_with(".http") && !input.ends_with(".md");
    if looks_like_host {
        NormalizedInput::HostName(format!("{default_scheme}://{input}"))
    } else {
        NormalizedInput::NotUrl(input.to_string())
    }
}
```

决策表（第四节）逐行落为 `http/url.rs::tests` 的表格驱动测试；`v1.2/users` 的判定已按实现修正为 HostName（与 CLI 同规则，见第四轮审查 R4-3）。

### 9.4 提交 6：quick input 换 ratatui-textarea（修 A6/G3/G4）

用 `InputMode` 枚举消灭两个互斥 bool（G3），textarea 自带多字节正确的编辑与光标：

```rust
// state.rs
pub enum InputMode { None, QuickCreate(TextArea<'static>), Filter(TextArea<'static>) }

// handlers/quick_input.rs —— 替换手写 Backspace/Char 处理
pub fn handle_input(state: &mut AppState, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => *state.input_mode() = InputMode::None,
        KeyCode::Enter if matches!(state.input_mode(), InputMode::QuickCreate(_)) => {
            let value = state.quick_input_text();
            *state.input_mode() = InputMode::None;
            state.set_quick_input_value(value);   // 兼容旧字段，后续 create_test_from_input 用
            state.create_test_from_input();
        }
        _ => {
            if let InputMode::QuickCreate(ta) | InputMode::Filter(ta) = state.input_mode_mut() {
                ta.input(crossterm::event::Event::Key(key));
            }
        }
    }
}

// ui/mod.rs —— 假光标 "█"（mod.rs:155）删除，改真实光标
if let InputMode::QuickCreate(ta) = state.input_mode() {
    let popup = centered_rect(60, 3, main_area);
    frame.render_widget(ta, popup);
    let (row, col) = ta.cursor();
    frame.set_cursor_position((popup.x + col as u16 + 1, popup.y + 1));
}
```

### 9.5 Phase 3 P1：过滤搜索（复用既有投影机制，G7）

`visible_file_nodes` 本就是 `rebuild_visible_tree_nodes()` 重建的投影——过滤即"带谓词重建"，**不引入第二份索引**，从根上规避 selected_index 双重寻址：

```rust
// state/files.rs
pub fn set_filter(&mut self, q: &str) {
    self.filter_query = q.to_lowercase();
    self.rebuild_visible_tree_nodes();  // 内部改为：rel_path.to_lowercase().contains(&self.filter_query)
    self.files_state.selected_index = 0; // 过滤变化重置选中（行为写入特征测试）
}
```

### 9.6 Phase 3 P2/P3：主题与设置

```rust
// src/tui/theme.rs —— 先盘点 ui/* 中全部 Color:: 硬编码（G8），收敛为两套调色板
pub struct Theme { pub border: Color, pub accent: Color, pub ok: Color, pub err: Color }
pub const DARK: Theme = Theme { border: Color::Rgb(63,63,63),  /* ... */ .. };
pub const LIGHT: Theme = Theme { border: Color::Gray,          /* ... */ .. };

// src/tui/settings.rs —— ~/.rupost/tui.toml，serde default 兼容缺失文件
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TuiSettings { pub theme: String }   // "dark" | "light"
impl TuiSettings {
    pub fn load() -> Self { /* 读失败(含写保护 home) → Default + last_error 提示（G9） */ }
    pub fn save(&self) -> std::io::Result<()> { /* ... */ }
}
```

---

## 十、第三轮查漏（v3.3 新发现）

| # | 缺口 | 等级 | 处置 |
|---|------|------|------|
| G1 | **TOCTOU 竞态**：`exists()` 检查与 `write()` 之间文件可能被并发创建，原设计的 `write_file`+`try_exists` 两原语组合不原子 | 设计缺陷 | 端口 API 改为 `create_exclusive`（`OpenOptions::create_new`，内核级原子），AlreadyExists 映射为"不覆盖 + 仍定位"（已回写决策 2） |
| G2 | **canonicalize 无法作用于不存在的目标文件**：新建文件的围栏检查若直接 canonicalize 目标路径必然 Err | 设计缺陷 | 检查改为 canonicalize 已存在的父目录 + join 文件名（9.2 示例） |
| G3 | 快速输入与未来过滤输入是**互斥模态**，两个独立 bool 会出现同时为真的非法状态 | 设计缺陷 | `InputMode` 枚举建模（9.4），旧 `show_quick_input` 字段过渡期保留兼容渲染 |
| G4 | 现快速输入用假光标 `█`（`ui/mod.rs:155`），换 textarea 后需 `frame.set_cursor_position` 真实光标 | 实现细节 | 9.4 示例；app.rs:271 的 `.show_cursor()` 已确认无需改 |
| G5 | `?` 帮助面板与 keybindings.md 均未列 `n`/`/`/`,` 新键 | 文档缺口 | Phase 2 文档对齐 + ui 帮助文本同步更新（已并入 Phase 2 验收） |
| G6 | 文件写入发生在 UI 线程（同步 IO），慢磁盘会短暂卡顿渲染 | 已接受的取舍 | 单个小文件、频次极低，YAGNI 不上 spawn_blocking；此处显式记录 |
| G7 | 过滤后 `selected_index` 若指向两套列表会产生寻址错乱 | 实现陷阱 | 复用"重建投影"机制不引入第二索引；过滤变化重置选中并写特征测试（9.5） |
| G8 | ui/* 四个文件散布 `Color::` 硬编码，主题化需先盘点 | 工作量风险 | P2 第一件事：盘点清单进 PR 描述，收敛为 Theme 常量 |
| G9 | `~/.rupost/tui.toml` 在写保护 home 下保存失败会静默 | 健壮性 | 设置读写失败同样走 `last_error` 浮出（9.6） |
| G10 | 输入 `tests/auth.http` 这类**含子目录的相对文件名**：jail 允许（仍在工作区内），但父目录缺失时写入失败 | 行为决策 | 不自动 mkdir（用户输入即意图）；失败经 A8 浮出，决策表补一行特征测试 |

三轮审查累计 F1–F17、G1–G10，主干决策（渐进重构、端口方向、模块边界、TDD 阶段）均未被推翻。

---

## 十一、第四轮：三代理代码审查记录（v3.3 执行后）

三个并行子代理分别从安全正确性、架构一致性、测试质量审查 `uuqzqlpz..mmunqzzx` 的 6 个提交。结论：主干（三防线、端口原子写、URL 统一、InputMode）兑现且有测试锁定；审查发现 2 个 P0 级流程违约、2 个 P1、8 个 P2。已处置如下（R4-n 对应审查编号）：

**已修复（审查后同批落地的提交）**：
| # | 发现 | 修复 |
|---|------|------|
| R4-1 | 穿越回归测试次断言字符串未插值恒真（测试缺陷，P1） | `format!` 显式拼出逃逸落点断言 |
| R4-2 | `with_ports` 零调用死 API + 权限测试 root 下假失败（P0/P2） | 新增 `FailingRepository` 测试替身，写失败测试改走 `with_ports` 注入，并锁定"成功清错"不变量 |
| R4-3 | 决策表 `v1.2/users` 行与实现相反（文档矛盾，P1） | 本文档两处已改为 HostName 并注明理由 |
| R4-4 | write_all 中途失败留残缺文件，重试被 AlreadyExists 掩盖（P2） | `create_exclusive` 失败时删除残留文件 |
| R4-5 | 文件名通道无长度上限（P2，与 URL 通道不对称） | `bound_filename`：stem 超 80 字节截断+短哈希，保留扩展名（多字节边界安全） |
| R4-6 | 模态门优先级、n 键分发、last_error 渲染零覆盖（P2 测试缺口） | 补 3 组测试（模态中 q/?/E 不触发全局行为；handle_key('n')→QuickCreate；smoke 矩阵加 last_error 维度） |
| R4-7 | 空输入 no-op、选中目录推导落盘无锁定（P2/P3 缺口） | 补 2 组特征测试 |
| R4-8 | render_quick_input_popup 重复 `Clear` 渲染（本迭代引入） | 去重 |

**登记不修（含审查新增债务）**：
- symlink 中间段逃逸（`tests → /outside` 预置符号链接时嵌套名可越界）：单用户工具+需预置，护栏非边界定位不变；ports.rs 的 FsRepository symlink 集成测试承诺撤销，以本条登记替代。
- DefaultHasher 跨版本不稳定（哈希后缀漂移→新文件）、Windows 保留设备名（NUL/CON）、`lines().concat()` 粘贴多行、AlreadyExists 目标为目录/symlink 时静默、`locate_created_file` 对不可见落点无提示、围栏与 create 间 TOCTOU——均为 P3，触发条件出现再修。
- `write_log_with_limit` 直写 std::fs（预存）：待 retention.rs 拆出时一并端口化，Phase 1 验收条款"state/ 写路径零命中"按字面不成立的事实以此条为准（仅快速新建写路径已干净）。
- `use super::*`（tree/layout 两处，全仓仅有的非测试通配导入）：过渡态，随 handlers/ 拆分同片收敛为显式导入。
- keybindings.md 与帮助面板的 n/`/`/`,` 键（G5）：留 Phase 2 文档对齐。

**审查排除的疑点**（核实为干净）：path_guard 提取无语义漂移；ports 未被 UI 误用；files.rs 领域函数纯；旧手写判定零残留；mod.rs 再导出语义完整；19 个新测试中行为锁覆盖成功创建/已存在不覆盖/错误浮出/截断四主路径。

**验证**：修复后 35 单测 + 39 冒烟 + 全量 448 全绿，clippy -D warnings 零输出。
