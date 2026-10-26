# RuPost P0 测试缺口执行方案 (Execution Plan)

> **创建日期**: 2026-10-25
> **父纲**: [全量测试方案 §6](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-25-test-strategy-plan.md)（本文件是其 P0 四项的可执行落地细化，含逐场景说明）
> **范围**: P0-1 `generate` 域、P0-2 快照重放、P0-3 历史子命令 + `export` 桩、P0-4 CLI 全局参数契约 —— 合计 **13 个新测试 + 2 处文档真话修正**
> **设计期实证**: 本方案编写期间已在真实二进制上验证关键行为（见 §0），场景断言均以实证为锚、非凭空推测

---

## 0. 设计期实证结论（本方案的新事实，父纲相应口径随本方案微调）

| # | 实证方式 | 结论 | 对方案的影响 |
| :-- | :--- | :--- | :--- |
| E1 | `./target/debug/rupost --debug test /tmp/x.http` vs `./target/debug/rupost test /tmp/x.http --debug` | **两种顺序均正确解析**（都走到"路径不存在"业务错误，说明 clap v4 已把 `global = true` 参数继承给子命令） | P0-4 的契约不是"锁定后置失效"，而是**锁定双序皆可**；AGENTS.md §5 CLAP 规则表述过时，列为修正交付 |
| E2 | `./target/debug/rupost --debug GET http://127.0.0.1:1/x` vs `./target/debug/rupost GET http://127.0.0.1:1/x --debug` | **真正的陷阱在内联模式**（无子命令）：后置 `--debug` 被根级 `args`（`trailing_var_arg + allow_hyphen_values`）吞入内联参数，`cli.debug` 为 false，且该 token 污染内联解析输入——实测改变了解析器选择（前置走 httpie parser，后置走 curl parser） | P0-4 增加"内联陷阱"场景行，这是 AGENTS.md 警告的真实成立范围 |
| E3 | 对照 `src/cli.rs:207`（Show help："1-indexed, 1 is the oldest"）与 `src/history/printer.rs:60`（`entries.len() - n`，1=**最新**） | **help 注释与实现矛盾** | P0-3 附带交付：按实现修正 help 注释，测试按实现语义（1=最新）锁定 |
| E4 | 读 `src/history/selector.rs` 底部 `mod tests` | selector 已有 2 个单测（`test_format_display`、`test_select_last`） | 父纲 §5.14 口径微调：generate 域缺口实为 **`generator/http.rs` 0 测试 + Interactive 路径无法进程内自动化**（inquire TTY 交互，归 L5 手工清单），并非整个 selector 无测试 |
| E5 | 读 `src/history/storage.rs:226`（`get_storage()` 为 `OnceLock`） | `RUPOST_HISTORY_DIR` 只在**首次**初始化生效；同进程内库级测试存在初始化顺序敏感（现有 `history_integration_test` 已带此隐患） | P0-3/P0-1 的 CLI 行为测试全部走**独立进程**（见 §1 基建），每进程重新读环境变量，彻底绕开 OnceLock 竞态 |

---

## 1. 测试基建决策（全方案共用）

**二进制级测试统一采用 `env!("CARGO_BIN_EXE_rupost")` + `std::process::Command`，零新增依赖。**

- Cargo 对含 `[[bin]]` 的 package 在集成测试中自动注入 `CARGO_BIN_EXE_<name>` 环境变量——无需引入 `assert_cmd`。
- **每条 CLI 场景一个独立子进程**：`RUPOST_HISTORY_DIR` 指向各自 `TempDir`，`get_storage()` 的 `OnceLock` 在子进程内首次初始化即读到正确目录（E5），测试间零串扰、天然可并行。
- 历史预置不依赖 CLI：测试进程内用 `HistoryStorage::new_with_path(dir.join("history.jsonl"))` 直接 `append`（与 `selector.rs` 既有单测同法），写完后子进程按 dir 读回——预置与断言两端解耦。

**通用 helper**（放各测试文件顶部，不建共享 crate，遵循极简原则）：

```rust
fn run_rupost(history_dir: &std::path::Path, args: &[&str], stdin: Option<&str>)
    -> std::process::Output;          // 设 RUPOST_HISTORY_DIR，捕获 stdout/stderr/exit code
fn seed_entry(storage: &HistoryStorage, id: &str, method: &str, url: &str, status: u16);
```

---

## 2. P0-1 `generate` 域（新建 `tests/generate_integration_test.rs`，4 测试）

> 用户故事：**As a** API 调试者，**I want** 把我刚发过的请求一键固化成 `.http` 测试文件，**So that** 一次性的手工调试沉淀为可回归的资产——这正是"文档即测试"理念的反向闭环（运行历史 → 测试文档）。

### 场景 2.1 单条目格式化与往返可解析（库级）
- **测试**: `test_generate_formats_single_entry_roundtrip_parsable`
- **Given**: 一条 `HistoryEntry`：`POST http://api.example.com/users`；headers 含 `Content-Type: application/json`（应保留）、`User-Agent: rupost/1.0.0`（应剥离）、`Authorization: Bearer tk`（应保留）；body 为压缩 JSON `{"name":"alice","roles":["a","b"]}`（应美化打印）；`response.status = 201`；`request.timeout = Some(500ms)`。
- **When**: `HttpGenerator::generate(&[entry])`，输出再喂 `rupost::parser::parse_content()`。
- **Then**:
  - 输出含 `### Request {id 前 8 字符}`、`# @timeout 500ms`、`POST http://api.example.com/users`、`Content-Type: application/json`、`# @assert status == 201`；
  - 输出**不含** `User-Agent`/`user-agent`（`should_skip_header` 剥离清单）；
  - body 以美化后的多行 JSON 呈现；
  - `parse_content` 恰好解出 1 个请求，method/url/body 等价，且断言元数据含 `status == 201`——**生成即可再执行**的往返证明。

### 场景 2.2 多条目分隔符正确性（库级）
- **测试**: `test_generate_multiple_entries_join_separator`
- **Given**: 3 条不同 method/url 的 entries。
- **When**: `generate`。
- **Then**: 3 个请求块以空行分隔；`parse_content` 解出恰好 3 个请求（证明分隔符既不粘连也不误切——启发式拆分器对生成物的兼容性）。

### 场景 2.3 CLI 端到端用户故事闭环（二进制级，旗舰场景）
- **测试**: `test_generate_cli_from_history_end_to_end`
- **Given**: 测试进程内启动 wiremock（`GET /orders` → 200 `{"ok":true}`）；TempDir 同时充当 `RUPOST_HISTORY_DIR` 与工作目录；目录内预写 `case.http`（一行 `GET {mock_uri}/orders`）。
- **When**（三段接力，模拟真实用户操作序列）:
  1. 子进程 `rupost t case.http` —— 真实执行并记录历史；
  2. 子进程 `rupost g out.http --last 1` —— 从历史生成测试文件；
  3. 子进程 `rupost t out.http` —— **执行生成物**。
- **Then**:
  - 步骤 2 退出码 0，stdout 含 `Generated test file: out.http (1 requests)`；
  - `out.http` 含 `GET {mock_uri}/orders` 与 `# @assert status == 200`；
  - 步骤 3 退出码 0（生成物真实可执行，闭环成立）。

### 场景 2.4 空历史友好退出（二进制级）
- **测试**: `test_generate_cli_empty_history_friendly_exit`
- **Given**: `RUPOST_HISTORY_DIR` 指向空 TempDir（无 `history.jsonl`）。
- **When**: `rupost g out.http --last 3`。
- **Then**: 退出码 0（`main.rs:100-103` 走 warn + 提前返回）；`out.http` 未被创建。注：warn 走 tracing，默认日志级别可能不落终端——验收只锁退出码与"不写文件"，不锁 warn 文本。

### 范围外登记
`generate --interactive`（inquire `MultiSelect` TTY 交互）无法进程内自动化，转入 [测试指引 §2 手工清单](file:///Users/zsyzzx/project/rust/rupost/doc/guides/testing_guide.md)：手工场景"`rupost g pick.http -i` → 空格勾选 2 条 → 回车 → 断言 pick.http 含 2 个请求块"。

---

## 3. P0-2 快照录制与重放（扩充 `tests/snapshot_replay_test.rs`，1 → 4 测试）

> 用户故事：**As a** 回归测试编写者，**I want** 录制一次真实交互后离线重放到任意目标环境，**So that** 不依赖原服务的稳定回归。

### 场景 3.1 多请求往返等价
- **测试**: `test_replay_multi_entry_roundtrip_equivalence`
- **Given**: 快照 2 条（原 host `example.com`）：`GET /a`、`POST /b`（JSON body `{"k":"v"}`）；wiremock 挂对应两条 mock 并启用 verify。
- **When**: `ReplayExecutor::new(Some(mock.uri()), false).replay_file()`。
- **Then**: `total=2, passed=2`；wiremock verify 断言实际收到 `GET /a` 与 `POST /b` 且 body 精确匹配——一次性证明 **Host/Port 改写 + Path/Query/Body 原样透传** 三项契约。

### 场景 3.2 录制 `@timeout` 透传（计时行为学证明）
- **测试**: `test_replay_honors_recorded_timeout`
- **Given**: 快照 1 条 `timeout = Some(500ms)`；wiremock mock `set_delay(2s)`。
- **When**: 计时执行 `replay_file`。
- **Then**: `passed=0`（超时 → 请求失败 → 状态比对失败）；**总耗时 < 1.5s**——若 timeout 未透传则退回 30s 默认、2s delay 后才失败，计时断言即透传的行为学证明。
- **实现期确认点**: 请求错误在 `ReplayReport` 中计入 `failed` 还是独立字段，首跑后对齐断言（计时断言为硬验收，不受此影响）。

### 场景 3.3 损坏快照容错
- **测试**: `test_replay_corrupt_snapshot_readable_error`
- **Given**: 三个坏输入：① 纯文本非 JSON；② 合法 JSON 但非快照结构（`{}`）；③ 结构合法但 `entries` 为空数组。
- **When**: 逐个 `replay_file`。
- **Then**: ①② 返回 `Err` 且错误信息非空（可读、非 panic）；③ 按实现锁定（预期 `total=0` 的 Ok 报告），首跑固化——**现状即契约**，防止未来静默行为漂移。

---

## 4. P0-3 历史子命令 + `export` 桩（新建 `tests/history_cli_test.rs`，5 测试）

> 用户故事：**As a** 重度使用者，**I want** 检索、详情查看、按龄清理与一键清空历史，**So that** 长期使用中历史库不失控、可追溯。

### 场景 4.1 `list` 的 tail 语义与反转
- **测试**: `test_history_list_limit_and_reverse`
- **Given**: 预置 3 条（id `a1…`/`b2…`/`c3…` 依次写入，全 8 字符 UUID，断言用前 8 短 ID）。
- **When**: `rupost history list --limit 2`；再 `rupost history list --limit 2 --reverse`。
- **Then**: 前者 stdout 含 `b2`/`c3` 短 ID、不含 `a1`；后者中 `c3` 所在行位置先于 `b2`（顺序断言用行索引比较；若 comfy_table 换行导致脆弱，降级为集合断言并在测试注释注明）。

### 场景 4.2 `show` 四分支（索引/短ID/越界/未命中 + 空库）
- **测试**: `test_history_show_by_index_id_and_notfound`
- **Given**: 预置 3 条（URL 各异，从 jsonl 读回真实 UUID 供短 ID 断言）。
- **When/Then**（同一测试内顺序覆盖五分支）:
  1. `show 1` → stdout 含**第 3 条**（最新）URL——按实现锁定 **1=最新**（E3）；
  2. `show <UUID 前 8>` → 含对应条目 URL；
  3. `show 99` → `Index 99 out of range. Total entries: 3.`；
  4. `show deadbeef` → `No matching history entry found for 'deadbeef'`；
  5. 空库 TempDir `show 1` → `No history entries found.`
- **同提交附带交付**: 修正 `src/cli.rs` Show 的 help 注释 `1 is the oldest` → `1 is the newest`（E3，doc-truth 与测试同原子提交）。

### 场景 4.3 `prune` 只删过期文件
- **测试**: `test_history_prune_old_logs_only`
- **Given**: `<dir>/logs/` 下两文件：`a.log` mtime 设为 10 天前（dev-deps 已有 `filetime`）、`b.log` mtime 为当前。
- **When**: `rupost history prune --days 7`。
- **Then**: stdout 含 `Removed 1`；`a.log` 消失、`b.log` 仍在。
- **实现期确认点**: `--max-size` 的单位（help 写 MB、gc 内部可能 bytes）确认后可加第二断言（超限按最旧清理）；单位不清时仅锁定 days 维度。

### 场景 4.4 `clear` 三档语义
- **测试**: `test_history_clear_semantics`（三场景顺序执行于同一测试）
- **Given**: `logs/` 1 文件 + `history.jsonl` 1 条。
- **When/Then**:
  1. `clear` 且 stdin 管道 `"n"` → stdout `Aborted.`，两处文件原样（交互拒绝路径）；
  2. `clear --yes` → `logs/` 文件消失，`history.jsonl` **仍在**（不误删主库）；
  3. `clear --yes --all` → 两处均消失（含 `Cleared history.jsonl` 输出）。

### 场景 4.5 `export` 桩行为锁定
- **测试**: `test_history_export_stub_locked`
- **Given**: 任意目录（桩不读历史，无需预置）。
- **When**: `rupost history export --last 5 -o out.json`。
- **Then**: stdout 含 `not implemented`；退出码 0（`main.rs:70-72` 返回 `Ok`）；`out.json` 未创建。
- **场景说明（测试的生命周期）**: 这是**桩契约锚点**——export 实现落地之日本测试必先红，红色是"契约已变"的信号而非缺陷，届时以真实行为测试替换。防止两个方向的静默漂移：有人以为 export 可用（现在是桩）、或桩被悄悄改动而无人知。

---

## 5. P0-4 CLI 全局参数位置契约（`src/cli.rs` `mod tests` 新增表驱动，1 测试 × 8 场景行）

> 用户故事：**As a** 脚本作者，**I want** 全局参数写在前缀或后缀都能按文档生效，**So that** 编写 CI 脚本时不被解析陷阱咬到。

- **测试**: `test_global_args_position_contract_table`（`Cli::try_parse_from`，纯解析级、零 I/O、零网络；cli.rs 是 `main.rs` 的 mod，测试自动进入主二进制 unittests——与现有 6 个 `cli::parser` 单测同通道）
- **场景表**（每行一个 `(argv, 期望解析结果)`）：

| # | 场景 | argv（省略程序名） | 断言 |
| :-- | :--- | :--- | :--- |
| 1 | 后置单旗（E1 实证） | `t f.http --debug` | `Test` 分支，`debug == true` |
| 2 | 前置单旗 | `--debug t f.http` | 同上 |
| 3 | 混合前后置 | `--debug t f.http --debug-on-failure` | 双旗均 `true` |
| 4 | 带值后置 | `t f.http --env dev` | `env == Some("dev")` |
| 5 | 带值前置 | `--env dev t f.http` | 同上 |
| 6 | 内联正确用法 | `--debug GET http://x/y` | `command == None`，`debug == true`，`args == ["GET","http://x/y"]` |
| 7 | **内联陷阱**（E2 实证） | `GET http://x/y --debug` | `command == None`，**`debug == false`**，`args` 含 `"--debug"`（旗被吞入内联输入） |
| 8 | 内联多旗后置 | `GET http://x/y --no-cookies --env dev` | 同 7 模式：旗全落入 `args`，全局字段保持默认 |

- **同提交附带交付**: AGENTS.md §5 CLAP 规则修正为准确表述——**"子命令模式下全局参数前后位置皆可（clap v4 继承）；内联模式（无子命令）全局参数必须置于最前，否则被内联参数吞入"**。修正与测试同原子提交（测试是修正的证据）。
- **场景说明**: 场景 7/8 锁定的不是理想行为而是**现状陷阱**——按父纲"现状即契约"原则锁定，为未来"内联模式吞旗告警"的改进留下红测试锚点。

---

## 6. 提交序列（jj 原子提交，每个提交前跑全量三步）

| # | 提交描述 | 内容 |
| :-- | :--- | :--- |
| 1 | `test(generate): lock HttpGenerator format, round-trip and CLI e2e loop` | §2 四测试（T1–T4） |
| 2 | `test(replay): multi-entry equivalence, timeout passthrough, corrupt snapshot` | §3 三测试（T5–T7） |
| 3 | `test(history): list/show/prune/clear CLI contracts + export stub lock` | §4 五测试（T8–T12）+ `cli.rs` Show help 注释修正 |
| 4 | `test(cli): global-args position contract table + inline-mode trap` | §5 表驱动（T13）+ AGENTS.md §5 修正 |

---

## 7. 验证与验收清单

**每提交前强制三步**（继承研发流程规范）：`cargo fmt --all -- --check` → `cargo clippy --all-targets --all-features -- -D warnings` → `cargo test` 全绿。

**战役完成验收**（对照父纲 §6 P0 四行验收标准）：

| 父纲验收标准 | 本方案对应 | 达成判据 |
| :--- | :--- | :--- |
| "`tests/` 含 generate 往返闭环测试" | 场景 2.3 | 新文件存在且三段接力全绿 |
| "`snapshot_replay_test.rs` ≥ 4 测试全绿" | §3 | 1 → 4 测试 |
| "四实子命令各有 ≥1 集成测试；桩行为有锚点" | §4 | list/show/prune/clear 各 1 + export 桩 1 |
| "`tests/` 内有显式命名测试；行为变更时测试先行失败" | §5 | 表驱动存在；clap 升级破坏任一场景行即红 |

**基线对账**: 全量 468 → 预期 481（tests/ +10 = 155；主二进制单测 +1 = 7）。新增测试均为隔离设计（独立进程 / TempDir / wiremock），不触碰 `examples/`，仍按纪律跑一次 `examples/run_all.sh` 确认 15/15。

---

## 8. 风险与范围外

| 项 | 处置 |
| :--- | :--- |
| `ReplayReport` 对请求错误的计数字段名 | 场景 3.2 实现期确认点（首跑对齐，计时断言不受影响） |
| `--max-size` 单位歧义（MB vs bytes） | 场景 4.3 实现期确认；不清则只锁 days 维度 |
| comfy_table 长行换行致顺序断言脆弱 | 场景 4.1 已备降级路径（集合断言） |
| `generate --interactive`（TTY） | 范围外 → L5 手工清单（§2 尾注） |
| `export` 真实实现 | 范围外（独立特性）；本战役止步桩锁定 |
| `get_storage()` OnceLock 注入化改造 | 范围外（架构债 Phase 6）；本方案以独立进程绕开而非修复 |
| AGENTS.md 修正 | 属用户工作区指令文件，修正内容仅为事实校准（E1/E2 实证），随提交 4 交付并可在 review 时单独审阅 |
