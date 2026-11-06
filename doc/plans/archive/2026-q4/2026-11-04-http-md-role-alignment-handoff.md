# `.http` / `.md` 职能对齐交接执行手册

> **状态**：Proposed（2026-11-04，待评审）
> **读者**：接手的实现模型（含 flash 级别模型）与人类评审者
> **目标**：在不破坏现有全量测试与示例的前提下，先修复 `.http`/`.md` 职能契约中的高风险缺陷，
> 再按决策队列推进 **`.md` 双向引导生成**（OpenAPI/curl/history/`.http` → `.md`，`.md` → `.http`）
> 与 Markdown 全生命周期能力。
> **规范性依据**：[`http_md_role_contract.md`](../architecture/http_md_role_contract.md)（"应该是什么"）
> **事实源分工**：本文件只承载分析与任务；进度唯一事实源是
> [`progress_summary.md`](progress_summary.md)；本文件不记录完成状态。
> **关联文档**：
> [`file-format-spec.md`](../apidoc/file-format-spec.md)、
> [`2026-06-06-markdown-lifecycle-specification.md`](2026-06-06-markdown-lifecycle-specification.md)、
> [`2026-06-11-markdown-lifecycle-development-spec.md`](2026-06-11-markdown-lifecycle-development-spec.md)、
> [`file_lifecycle_and_embedded_spec_blueprint.md`](../architecture/file_lifecycle_and_embedded_spec_blueprint.md)、
> [`user_story_and_compliance_walkthrough.md`](../architecture/user_story_and_compliance_walkthrough.md)

---

## 0. 接手须知（先读这一节）

### 0.1 默认执行范围

- **必须完成（Phase 1）**：T1、T2、T3、T4、T5。全部是低风险、可独立验证的机械修复/加锁/文档任务。
- **需人类确认后才做（Phase 2）**：**T9（双向引导生成，最高优先，对应 G13）**、T6（YAML frontmatter）、
  T7（未知指令告警）、T8（Mock DSL 操作符）。
- **独立史诗（Phase 3）**：嵌入契约、CI 分发、Schema 校验、弃用熔断、SSE/WS 契约审计。
  **未经人类明确批准，不要开始 Phase 3。** 先把 Phase 1 交付干净。

### 0.2 工作协议（每个任务都适用）

1. **先读后写**：修改任何文件前先完整阅读该文件与相关测试；不要凭本文件的片段直接改。
2. **TDD**：先写会失败的测试（红灯），确认失败原因符合预期，再改实现（绿灯），最后重构。
3. **外科手术**：只改任务列出的文件/函数；不顺手重构、不格式化无关代码、不删既有死代码。
4. **原子提交**：每个 T 任务一个独立提交。优先 `jj`（`jj describe` / `jj new`），
   环境不可用时用 `git commit`；提交信息用 `fix:` / `test:` / `docs:` 前缀。
5. **门禁**：提交前至少运行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、
   本任务相关测试；全部完成后运行 `make gate`。
6. **不猜**：遇到本文件未覆盖的语义冲突，停止并在 `progress_summary` 或 PR 描述里记录问题，
   不要自行发明新语法或改变既有契约。
7. **行号会漂移**：本文件的 `文件:行` 只用于定位，实际以函数名/代码特征为准。

### 0.3 常用命令

```bash
# 只跑本批次相关测试
cargo test --lib mock::compiler
cargo test --lib parser::markdown_file
cargo test --test mock_integration_test
cargo test --test markdown_integration_test

# 快速门禁（fmt + clippy）
make gate-fast

# 完整门禁（fmt + clippy + 全量测试）——每批次收尾必须跑
make gate

# 查看本批次涉及的调用点
grep -rn "MockCompiler::compile" src/ tests/
```

> macOS 环境若 `make gate` 因 SDK 报错，改用 `make build`（内含 CommandLineTools SDK 规避逻辑），
> 详见 `Makefile`。

---

## 1. 项目功能速览（给接手的模型建立地图）

RuPost 是 Rust 编写的终端 HTTP 客户端与 API 回归引擎，核心链路：

```text
.http / .md
   │  parser（纯文本 → AST，不依赖网络/IO 业务）
   ▼
ParsedFile { metadata, dependencies, requests: Vec<ParsedRequest> }
   │  VariableResolver（变量渲染）
   ▼
TestExecutor
   ├─ HttpRunner / SseRunner / WsRunner（协议执行）
   ├─ Assertion / Capture（断言与变量捕获）
   └─ History / Reporter（历史与报告）
```

关键模块（详见 [`directory_structure.md`](../architecture/directory_structure.md)）：

| 模块 | 职责 | 本批次相关文件 |
| :--- | :--- | :--- |
| `src/parser/` | `.http` / `.md` 解析、AST、扫描器 | `http_file.rs`、`markdown_file.rs`、`metadata.rs`、`types.rs` |
| `src/runner/` | 单请求/批处理执行、依赖拓扑 | `executor.rs`、`engine/resolver.rs`、`app.rs` |
| `src/mock/` | Mock 路由编译与匹配 | `compiler.rs`、`matcher.rs`、`server.rs`、`variant.rs` |
| `src/variable/` | 变量级联、捕获 | `resolver.rs`、`config.rs` |
| `src/exporter/` | 请求 → curl/httpie/wget | `preparer.rs`、`mod.rs` |
| `crates/rupost-ai/` | `rupost ai assert` 创作辅助 | `translator.rs` |

关键 AST（[`src/parser/types.rs`](../../src/parser/types.rs)）：

- `FileMetadata`：frontmatter 全局元数据（title/version/base_path/rules/security/extra）。
- `ParsedRequest`：method/url/headers/body/metadata/base_path/line_number。
- `RequestMetadata`：`is_test`、`mock_variants`、断言、捕获、SSE/WS 等请求级元数据。
- `ParsedFile`：requests + metadata + dependencies + source_path。

---

## 2. 文件职能定义（摘要）

完整规范见 [`http_md_role_contract.md`](../architecture/http_md_role_contract.md)。本手册采用的核心裁决：

| 维度 | `.http` | `.md` |
| :--- | :--- | :--- |
| 定位 | 执行态工件 | 设计态 SSOT |
| 承载 | 请求 + 断言/捕获/执行编排 | 设计元数据 + Mock 契约 + `@test` 用例 + AI 上下文 |
| Mock 变体 | 不承载 | `@mock-when` / `@mock-default` |
| `@depends-on` | 执行拓扑 | 执行拓扑（不是契约继承） |
| 契约继承 | 不承载（交付态可带 `# @spec-*` 快照） | `@extends-spec`（设计态） |
| 交付 | 自包含、通用客户端兼容 | 可裁剪；编译为内联快照后可不分发 |

**三个最容易违反的点**：

1. Mock 契约块在测试模式下绝不能发真实请求（I2）。
2. 不支持的 `@mock-when` 条件绝不能变成"无条件命中"（I3）。
3. `base_path` 只有一种语义；Mock 路由使用契约中写明的完整路径（I5）。

---

## 3. 当前实现评估（证据与缺口）

### 3.1 评估基准

| 层级 | 来源 | 性质 |
| :--- | :--- | :--- |
| L1 | README / `progress_summary.md` | 已交付能力 |
| L2 | `2026-06-06` 生命周期规范 + `2026-06-11` 开发规格 | Markdown 生命周期 MVP |
| L3 | `file_lifecycle_and_embedded_spec_blueprint.md` + `user_story_and_compliance_walkthrough.md` | 全生命周期/CI 分发/契约审计 |

本文件编写时执行过的基线（供接手者对照，不是全量基线）：

```text
cargo test --lib markdown_file::                                   → 11 passed
cargo test --test mock_integration_test --test markdown_integration_test → 11 passed
```

接手后第一步（T0）应自行运行 `make gate` 并记录全量基线，后续所有结论以新基线为准。

### 3.2 缺口登记表

| ID | 级别 | 问题 | 证据（文件:函数/行） | 影响 |
| :--- | :--- | :--- | :--- | :--- |
| G1 | **高** | 不支持的 `@mock-when` 条件静默变成无条件命中（fail-open） | `src/mock/compiler.rs` `compile` / `parse_condition_expression`；`src/mock/matcher.rs` 变体循环 | 错误分支吞掉所有请求、遮蔽 `@mock-default`；测试假绿/假错 |
| G2 | **高** | 纯契约 `.md` 在 `rupost test` 下会被当真实请求执行 | `src/runner/executor.rs` `execute_all` 只按 `has_explicit_tests` 跳过；`src/exporter/preparer.rs` 则始终跳过 mock 变体块 | 设计文档可能被发到真实服务；行为与 exporter 不一致 |
| G3 | **高** | 设计元数据只解析不消费；YAML `security` 会丢 | `FileMetadata` 仅 parser 使用；`markdown_file.rs::parse_frontmatter` TOML 失败后行扫描只映射 4 个 key | "AI Memory/SSOT"名不副实；文档承诺与实现不符 |
| G4 | **高** | 嵌入契约、`@extends-spec`、Schema 校验、CI 编译分发完全未实现 | 全仓 grep 不到 `@spec-`、`EmbeddedSpec`、`@extends-spec`、semver 校验；`export` 只是命令导出 | L3 用户故事 3/4/7 不成立；`examples/lifecycle_showcase/` 无法运行 |
| G5 | 中 | `base_path` 在 Runner 与 Mock 间语义分裂 | Runner：`executor.rs` → `http/url.rs` 拼接；Mock：`compiler.rs` 直接用 `req.url` | 相对契约路径 + `base_path` 时 Mock 路由与测试目标不一致 |
| G6 | 中 | `@depends-on` 混淆执行依赖与设计依赖 | `@extends-spec` 未实现；规范方案 B 只能用 `@depends-on` 关联设计文档，而依赖文件会被执行 | 设计与测试解耦模式不可用（T2 可缓解执行风险） |
| G7 | 中 | Mock 条件表达力不足 | `parse_condition_expression` 只认 `==` / `None`；`CompareOp::Contains` 在 DSL 中不可达；无 delay；动态变量无 `$random_id` | 用户故事 2 与 `lifecycle_showcase` 的 `> 100` 不可用 |
| G8 | 低 | 未知 `@` 指令静默忽略 | `src/parser/metadata.rs` 默认 `_ => Ok(None)` | 用户以为 `@extends-spec`/`@deprecated` 生效，实际 no-op |
| G9 | 低 | `@test` 用子串判定 | `markdown_file.rs` `block.content.contains("@test")` | 正文/URL 含 `@test` 会误判契约块为测试块 |
| G10 | 低 | Mock 块行号固定为 1 | `markdown_file.rs::parse_mock_block(block, 1)` | 错误报告定位不准 |
| G11 | 低 | 文档命令漂移与过期状态 | 多份 `doc/plans` 写 `rupost mock start`，实际无 `start` 子命令；`file-format-spec.md` 仍写 "Phase 2/3 计划中" | 用户复制命令失败；文档可信度下降 |
| G12 | 高（验收） | `examples/lifecycle_showcase/` 是期望态夹具但当前不可执行 | `order_spec.md` 使用 `# @mock-when`、`>`、`@deprecated`、`@spec-response-schema`、`host`；`order_test.http` 使用 `@extends-spec` 与内联契约 | 它是 Phase 3 的验收目标，不是现状 |
| G13 | **高（产品）** | `.md` 只能手写；缺少 OpenAPI/curl/history/`.http` → `.md` 的引导生成与反向提升 | 主仓 `generate`/`import curl` 只产出 `.http`；无 `import openapi`、`spec promote`/`spec compile`；计划中的 `ai gen --spec` 未实现 | SSOT 建档成本高、存量资产无法迁移、手写易与实现漂移；用户 2026-11-04 明确反馈"不合理，应该双向" |
| G14 | 中 | 派生 `.http` 无 provenance/漂移检测，双向回写无所有权规则 | `src/generator/mod.rs::run_generate` 直接 `fs::write`；`src/importer/curl.rs` 只做防覆盖；无 `@generated-from`/`--check` | 手工修改派生文件后重新生成会静默覆盖；无法判断"谁是源" |

### 3.3 分层结论

| 需求层 | 结论 |
| :--- | :--- |
| L1 基础能力 | ✅ 满足 |
| L2 Markdown 生命周期 MVP | ⚠️ 基本满足，但 G1/G2 必须先修；G3/G5 需裁决与补齐 |
| L3 全生命周期 SSOT | ❌ 未实现，需按 Phase 3 史诗推进 |
| 本文件定义的 `.http`/`.md` 职能 | ⚠️ 部分满足：`.md` 的 Mock+测试成立；设计 SSOT/契约校验/AI Memory 未成立；且缺少 `.md` 引导生成通道（G13，2026-11-04 用户反馈后纳入契约修订） |

---

## 4. 需求分层与本次目标

### 4.1 本次（Phase 1）目标

把 `.md`/`.http` 的**执行语义**修到与契约一致，不新增产品能力：

1. Mock 条件解析 fail-closed（G1）。
2. Mock 契约块在测试模式永不执行（G2）。
3. `base_path` 语义锁定并加回归测试（G5）。
4. 修正会让用户复制失败的活动文档（G11）。
5. 登记进度（T5）。

### 4.2 非目标（Phase 1 MUST NOT 做）

- 不实现 `@extends-spec`、`@spec-*`、内联契约、Schema 校验、弃用熔断（Phase 3）。
- 不新增 YAML 依赖、不重写 frontmatter（Phase 2 决策项）。
- 不新增 Mock 操作符（`>`/`contains`/`delay`）（Phase 2 决策项）。
- 不改 `TestResult`/JSON 报告字段（当前 JSON schema 有精确键集合测试锁定）。
- 不重构 `parse_condition_expression` 的返回类型以外的既有结构。
- 不批量改写历史性文档（只改用户会照抄的活动文档）。
- **不实现自动双向同步**（文件监听、静默回写、三方合并）；双向只做显式
  `bootstrap` / `promote` / `compile`（Phase 2 T9）。

---

## 5. Phase 1：任务卡

> 执行顺序：T1 → T2 → T3 → T4 → T5。T1/T2 可分别独立提交；T3/T4 是文档/测试锁；T5 收尾。

| 任务 | 代码改动量（估） | 风险 | 依赖 |
| :--- | :--- | :--- | :--- |
| T1 | 2 个源文件 + 2 个测试文件，约 80 行 | 中（改公共 API 签名） | 无 |
| T2 | 4 个源文件 + 1 个测试文件，约 40 行 | 低（执行期跳过） | 无 |
| T3 | 仅测试 + 文档，约 20 行 | 低 | T1（`compile` 已返回 `Result`） |
| T4 | 仅文档 | 低 | 无 |
| T5 | 仅文档 | 低 | T1-T4 |

### T0 基线（无代码改动）

1. 阅读：`AGENTS.md`、本文件、`doc/architecture/http_md_role_contract.md`、
   `src/mock/compiler.rs`、`src/runner/executor.rs`、`src/exporter/preparer.rs`、
   `src/http/url.rs`、`src/parser/markdown_file.rs`。
2. 运行 `make gate`，记录退出码与失败项（若基线本身失败，先报告，不要开始改代码）。
3. 运行 `grep -rn "MockCompiler::compile" src/ tests/`，记录所有调用点。
4. 把基线结果写入交接报告（见 §11 模板）。

**验收**：能说清 7 个调用点、当前全量测试是否全绿、基线失败清单。

---

### T1（P0）Mock 条件解析 fail-closed + `@mock-default` 排序

#### 目标

1. 不支持的 `@mock-when` 表达式在 **编译期** 返回错误，绝不再退化为无条件命中。
2. `@mock-default`（`condition == None`）始终排在条件变体之后匹配。

#### 非目标

- 不新增操作符（`>`、`contains` 等，属 T8/Phase 2）。
- 不改 `@mock-default` 的语义（仍是无条件兜底）。
- 不改快照 Mock 的排序逻辑（`server.rs` 中已有 snapshot 分支）。

#### 涉及文件

| 文件 | 改动 |
| :--- | :--- |
| `src/mock/compiler.rs` | `compile` 返回 `Result`；解析失败返回 `RupostError::ParseError`；变体稳定排序 |
| `src/mock/server.rs` | `MockCompiler::compile(parsed)?` |
| `src/mock/compiler.rs` `mod tests` | 更新现有测试 `.unwrap()`；新增 2 个单测 |
| `tests/mock_integration_test.rs` | 所有调用点 `.expect(...)`；新增 1 个集成测试（可选但推荐） |

#### 现状代码要点

> API 说明：`MockCompiler::compile` 是库公开 API，返回类型改为 `Result` 属破坏性变更。
> 项目当前为 0.x 内部工具，仓库内调用点共 7 处（`src/mock/server.rs` 1、`src/mock/compiler.rs` 测试 1、
> `tests/mock_integration_test.rs` 5），全部在本批次更新；不要为了"兼容"保留一个会 fail-open 的旧入口。

```rust
// src/mock/compiler.rs（当前）
pub fn compile(parsed_file: &ParsedFile) -> Vec<MockRouteConfig> {
    ...
    let condition = var.condition_expr
        .as_ref()
        .and_then(|expr| Self::parse_condition_expression(expr)); // None 会变成无条件
    ...
}
```

`parse_condition_expression` 只在 `expr.split("==")` 恰好两段且前缀为
`query.` / `header.` / `body.` 时返回 `Some`；否则返回 `None`。
`None` 在 `TrieRouteMatcher::match_request` 中表示"无条件匹配"，因此任何拼错或未支持的条件
都会变成兜底分支。

#### 实施步骤

1. **改签名**：

   ```rust
   use crate::{Result, RupostError};

   pub fn compile(parsed_file: &ParsedFile) -> Result<Vec<MockRouteConfig>> { ... }
   ```

2. **对每个变体显式区分"无条件"与"条件解析失败"**：

   ```rust
   let condition = match var.condition_expr.as_deref() {
       None => None, // @mock-default，合法
       Some(expr) => Some(Self::parse_condition_expression(expr).ok_or_else(|| {
           RupostError::ParseError(format!(
               "无法解析 @mock-when 条件 `{expr}`（路由: {method} {path}）。\
                当前仅支持 `query.|header.|body.` 前缀 + `==`，以及 `== None`。"
           ))
       })?),
   };
   ```

   建议在错误信息中追加 `parsed_file.source_path`（若有），方便定位文件；
   不要为此改 `ParsedFile` 结构。

3. **稳定排序**（在 `variants` 构建完成后、`routes.push` 之前）：

   ```rust
   // @mock-default 必须最后匹配；稳定排序保留条件变体之间的书写顺序
   variants.sort_by_key(|v| v.condition.is_none());
   ```

4. **更新调用点**：

   ```bash
   grep -rn "MockCompiler::compile" src/ tests/
   ```

   - `src/mock/server.rs`：`let routes = MockCompiler::compile(parsed)?;`
   - `src/mock/compiler.rs` 测试：`.unwrap()`
   - `tests/mock_integration_test.rs` 5 处：`.expect("mock routes should compile")`

5. **新增单测**（放 `src/mock/compiler.rs` 的 `mod tests`）：

   - `test_compile_rejects_unsupported_mock_condition`
     - 构造 `ParsedRequest`：`POST /orders`，`mock_variants = [condition_expr: Some("query.age > 18")]`
     - 断言 `compile` 返回 `Err`，错误字符串包含 `query.age > 18` 与 `/orders`。
   - `test_compile_puts_mock_default_last`
     - 构造变体顺序：先 `condition_expr: None`，后 `condition_expr: Some("header.X == 1")`
     - 断言编译后 `variants[0].condition.is_some()`、`variants[1].condition.is_none()`。
   - 现有 `test_mock_compiler_compile` 只加 `.unwrap()`，断言不变。

6. **新增集成测试**（推荐，放 `tests/mock_integration_test.rs`）：
   `test_run_server_rejects_invalid_mock_condition`

   - 用 `tempfile::tempdir()` 写一个 `.md`，内容含一个 `@mock-when query.age > 18` 的契约块；
   - 断言 `run_server_from_file(path, 0).await.is_err()`，错误信息包含 `query.age > 18`；
   - 失败发生在绑定端口前，不会挂起测试。
   - `tempfile` 已在 `[dev-dependencies]`，无需新增依赖。

#### 验证命令

```bash
cargo test --lib mock::compiler
cargo test --test mock_integration_test
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

#### 验收标准

- [ ] 任意非法/未支持条件 → 编译错误，错误信息含原始表达式与路由。
- [ ] `@mock-default` 永远排在条件变体后。
- [ ] 现有被解析的 `@mock-when` 示例（均为 `==` / `== None`）仍可编译。
- [ ] `tests/mock_integration_test.rs` 全绿。
- [ ] 无新增依赖、无无关格式化。

#### 常见坑

- `parse_condition_expression` 内部对 `split("==")` 的 `parts.len() != 2` 也返回 `None`；
  现在会被外层正确转成错误，不要再改回 `and_then`。
- 不要在 `compile` 里用 `expect`/`unwrap`：错误要向上传播到 CLI。
- 注意 `RupostError::ParseError` 显示为 `解析错误: ...`，测试断言用 `contains`，不要断言完整字符串。
- `sort_by_key` 是稳定排序；不要用 `sort_unstable`，否则条件变体顺序可能变化。
- `examples/lifecycle_showcase/order_spec.md` 里有 `# @mock-when ... > 100`：
  该行带 `#` 前缀，当前 `parse_mock_block` 不会把它识别为变体，因此不会触发 T1 的报错路径。
  不要把它加入 T1 的回归集合；注释前缀 + `>` 操作符属于 Phase 3（见 7.7）。

#### 建议提交信息

```text
fix(mock): reject unsupported @mock-when conditions and order @mock-default last
```

---

### T2（P0）Mock 契约块不得被 `rupost test` 执行

#### 目标

含 `mock_variants` 且没有 `@test` 的代码块，在测试模式下必须跳过（不发包），
与 `src/exporter/preparer.rs` 的既有规则一致。

#### 非目标

- 不改变 `.md` 普通代码块（无变体、无 `@test`）的"文档即测试"行为。
- 不新增 `--include-contracts` 开关（如需，另立项）。
- 不改变 exporter 的导出格式；只把"是否为契约块"的判定统一到同一个标记。

#### 涉及文件

| 文件 | 改动 |
| :--- | :--- |
| `src/parser/types.rs` | `RequestMetadata` 增加 `is_mock_contract: bool`（默认 false） |
| `src/parser/markdown_file.rs` | 识别到 Mock 契约块时置 `is_mock_contract = true`（即使变体解析为空） |
| `src/runner/executor.rs` | `execute_all` 的 `should_skip` 条件 |
| `src/exporter/preparer.rs` | 跳过条件改用同一标记（行为对现有文件不变） |
| `tests/mock_integration_test.rs` | 新增纯契约文件不执行的测试 |

#### 现状代码要点

```rust
// src/runner/executor.rs
let has_explicit_tests = parsed_file.requests.iter().any(|r| r.metadata.is_test);
...
let should_skip = parsed_request.should_skip()
    || (has_explicit_tests && !parsed_request.metadata.is_test);
```

问题：若文件**没有**任何 `@test`，`has_explicit_tests == false`，契约块不会被跳过。

另一个边界：`markdown_file.rs` 用 `block.content.contains("@mock-when")` 判定契约块，
但 `parse_mock_block` 可能因注释前缀（如 `# @mock-when`）解析不出任何变体。
此时 `mock_variants.is_empty()` 为真，单靠变体列表无法识别它是契约块。

#### 实施步骤

1. **新增 AST 标记**（`src/parser/types.rs`）：

   ```rust
   pub struct RequestMetadata {
       // ...
       /// 该请求是否来自 Mock 契约块（由 Markdown 解析器标记；与变体是否解析成功无关）
       pub is_mock_contract: bool,
   }
   ```

   `Default` 已派生，无需手写；`tests/tui_smoke_test.rs` 的 3 处 `RequestMetadata { ..Default::default() }`
   无需改动。

2. **解析器标记**（`src/parser/markdown_file.rs`）：

   - 在 `parse_content` 的 `has_mock_variants` 分支里，`parse_mock_block` 返回后设置
     `req.metadata.is_mock_contract = true;`（在 `parsed_file.add_request(req)` 之前）。
   - 不要只依赖 `mock_variants` 非空；契约块即使变体解析失败也必须被标记。

3. **执行器跳过**（`src/runner/executor.rs`）：

   ```rust
   let is_contract_block = parsed_request.metadata.is_mock_contract
       || !parsed_request.metadata.mock_variants.is_empty();
   let should_skip = parsed_request.should_skip()
       || (is_contract_block && !parsed_request.metadata.is_test)
       || (has_explicit_tests && !parsed_request.metadata.is_test);
   ```

   保留 `!mock_variants.is_empty()` 作为兜底：手工构造 AST 的测试/调用方可能没有设置新字段。

4. **导出器对齐**（`src/exporter/preparer.rs`）：

   把现有的

   ```rust
   if !req.metadata.mock_variants.is_empty() && !req.metadata.is_test {
   ```

   改为

   ```rust
   let is_contract_block =
       req.metadata.is_mock_contract || !req.metadata.mock_variants.is_empty();
   if is_contract_block && !req.metadata.is_test {
   ```

   跳过原因仍用 `"mock route definition"`，避免影响现有导出测试断言。

5. **新增集成测试** `test_pure_contract_markdown_is_not_executed`：
   - 用 `MarkdownFileParser::parse_content` 解析一段内联 Markdown，包含两个契约块：
     1. 规范契约块：`@mock-when query.x == 1` + `HTTP/1.1 200 OK`；
     2. 注释前缀契约块：`# @mock-when query.x == 1`（解析不出变体，但应被识别为契约块）。
   - 两个块都无 `@test`；URL 用 `http://127.0.0.1:1/never`（若真的发包必然连接失败）。
   - `TestExecutor::with_ephemeral_cookies().execute_all(parsed, &mut context).await`。
   - 断言：`results.len() == 2`、两个结果均 `skipped == true`、`success == true`、
     `error.is_none()`。
   - 既有 `test_abstract_base_url_scenario` 继续验证"有 `@test` 时契约块被跳过"。
   - 若断言 `is_mock_contract` 字段，可直接在解析结果上断言，不依赖执行结果。

6. **不新增字段到 `TestResult`**；跳过仍走已有的 `TestResult::skipped(...)`。

#### 验证命令

```bash
cargo test --test mock_integration_test
cargo test --test markdown_integration_test
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

#### 验收标准

- [ ] 只有契约块、没有 `@test` 的 `.md`，`rupost test` 结果为全部 skipped，不发包。
- [ ] 注释前缀/解析失败但含 `@mock-when` 的契约块也被标记并跳过。
- [ ] 有 `@test` 的混合文件行为不变（现有测试保持绿）。
- [ ] 无 `@test` 且无变体的普通文档仍按"文档即测试"执行。
- [ ] `--only` 选中契约块时仍 skipped（在测试中显式断言或注释说明）。
- [ ] exporter 对规范契约块与畸形契约块都跳过，既有导出测试不变。

#### 常见坑

- 不要把"跳过契约块"实现成解析期过滤：Mock 模式还需要这些块，必须在执行期跳过。
- 不要用 `metadata.mock_variants.is_empty()` 反向判断"普通块"：`@test` 块也可能带变体，
  必须加 `!is_test`。
- 不要只依赖 `mock_variants` 非空：注释前缀或畸形变体会导致变体列表为空，
  必须用 parser 标记的 `is_mock_contract`。
- 跳过结果 `success` 为 `true`（既有语义），不要改成 `false`。
- 新增 `is_mock_contract` 字段后，`RequestMetadata` 的 `Default` 派生仍可用；
  不要手写 `impl Default`。

#### 建议提交信息

```text
fix(runner): never execute mock contract blocks as tests
```

---

### T3（P1）`base_path` 语义锁定（文档 + 回归测试，不改行为）

#### 目标

把已裁决的语义固化，避免后续实现者"顺手"让 Mock 也拼 `base_path` 造成双重前缀：

- 契约路由路径 = 代码块中写明的路径，Mock 编译**不**叠加 `base_path`。
- `base_path` 只参与测试请求的相对 URL 解析（Runner）。
- 设计文档写契约路由时必须写从 `/` 开始的完整路径。

#### 非目标

- 不修改 `MockCompiler` 的生产行为。
- 不修改 `resolve_final_url`。

#### 涉及文件

| 文件 | 改动 |
| :--- | :--- |
| `src/mock/compiler.rs` `mod tests` | 新增 1 个回归锁测试 |
| `doc/architecture/http_md_role_contract.md` | 已在 §6.1 记录（评审时确认） |
| `doc/guides/mock_server.md` | 补一段 `.md` 契约路径写法说明 |

#### 实施步骤

1. 新增单测：

   ```rust
   #[test]
   fn test_compile_ignores_file_base_path_for_contract_routes() {
       let mut parsed = ParsedFile::new();
       let mut req = ParsedRequest::new(1);
       req.method = Some("GET".to_string());
       req.url = "/orders".to_string();
       req.base_path = Some("/api/v1".to_string());
       parsed.add_request(req);

       let routes = MockCompiler::compile(&parsed).unwrap();
       assert_eq!(routes.len(), 1);
       assert_eq!(routes[0].path, "/orders"); // 不叠加 base_path
   }
   ```

2. 在 `doc/guides/mock_server.md` 的 Markdown 编译章节补 3–5 行说明：
   - frontmatter `base_path` 不参与 Mock 路由拼接；
   - 契约块请写完整路径（如 `POST /api/v1/orders`）；
   - 若写相对路径，Mock 会按原样注册，通常不是期望行为。
3. 不修改 `base_path` 的 Runner 行为；如需解释，链接 `src/http/url.rs` 的 `resolve_final_url`。

#### 验证命令

```bash
cargo test --lib mock::compiler
```

#### 验收标准

- [ ] 回归测试锁定"Mock 路由 = 契约原文"。
- [ ] 文档解释与测试一致。
- [ ] 无生产代码行为变化。

#### 常见坑

- 如果评审结论认为 Mock 应该叠加 `base_path`，不要直接改代码：先更新契约文档与
  所有 `examples/design_scenarios/*`（它们当前写的是完整路径），否则会双重前缀并破坏示例。
  该变更属于契约变更，需人类确认。

#### 建议提交信息

```text
test(mock): lock base_path semantics for contract routes
```

---

### T4（P1）活动文档漂移修正

#### 目标

修正用户会照抄的命令与明显过期的状态描述；不重写历史记录。

#### 非目标

- 不批量改写 `doc/competition/`、`doc/debug/`、`doc/dev/progress_summary.md` 等历史/分析文档。
- 不重写 `progress_summary.md` 的历史行（只在 T5 追加新行）。
- 不修改代码注释以外的实现。

#### 实施步骤

1. 先盘点：

   ```bash
   grep -rn "mock start" README.md doc/ AGENTS.md .agents/ 2>/dev/null
   ```

2. 只修改"活动文档"：
   - `doc/plans/2026-06-06-markdown-lifecycle-specification.md`
   - `doc/plans/2026-06-06-mock-server-design.md`
   - `doc/architecture/user_story_and_compliance_walkthrough.md`
   - `doc/plans/production_debug_development_plan.md`（若仍在用）
   - 其他用户指南（`doc/guides/*`）若出现，统一改。

   把 `rupost mock start <file>` 改为 `rupost mock <file>`；
   若句子强调"启动"语义，改成 "运行 `rupost mock <file>` 启动"。

3. 更新 `doc/apidoc/file-format-spec.md` 的过期状态：
   - "断言（Phase 2 Stage 5 实现）" → 已实现；
   - "变量替换（Phase 3 实现）" → 已实现；
   - "暂不支持变量捕获和传递（Phase 3）" → 已实现 `@capture`；
   - 保留确实未实现的能力（如 multipart/form-data；先 `grep -rn "multipart" src/` 确认为空）。
   - 在 `.md` 章节补一句：普通块 = Mock 契约，`@test` 块 = 测试用例，见契约文档。

4. 不修改 `doc/plans/progress_summary.md` 历史行；T5 负责追加。

#### 验证命令

```bash
grep -rn "mock start" README.md doc/ AGENTS.md .agents/ 2>/dev/null
# 预期：只剩历史/分析文档；活动文档为 0
```

#### 验收标准

- [ ] 用户指南、生命周期规范、用户故事中的命令可直接复制执行（`rupost mock <file>`）。
- [ ] `file-format-spec.md` 不再声称已实现能力"计划中"。
- [ ] 历史文档未被无意义改写。

#### 建议提交信息

```text
docs: align mock command and file-format status with implementation
```

---

### T5（收尾）进度与变更记录

#### 目标

按 `AGENTS.md` 更新唯一进度事实源，并记录用户可见修复。

#### 实施步骤

1. 在 `doc/plans/progress_summary.md` 的表格追加一行（放在最新批次附近，保持现有格式）：
   - 批次名、目标、状态 `已完成`、交付物（文件列表 + 测试名）。
2. 在 `CHANGELOG.md` 的 `[Unreleased]` 下补充：
   - `Fixed`: 不支持的 `@mock-when` 不再静默变成兜底；纯契约 `.md` 不再被 `test` 执行。
   - `Docs`: `rupost mock start` → `rupost mock`；`base_path` 语义说明。
3. 跑 `make gate`，记录结果。

#### 验收标准

- [ ] `progress_summary` 有新批次行，引用真实测试名。
- [ ] `CHANGELOG` 条目与代码行为一致。
- [ ] `make gate` 全绿（若基线有已知失败，需在报告中列出并说明非本批次引入）。

#### 建议提交信息

```text
docs: record http/md role alignment batch in progress summary
```

---

## 6. Phase 2：决策后任务（先确认，再实施）

> 以下任务会改变用户可见能力或引入依赖/新语法，**必须**先得到人类确认。
> 确认前只需在交接报告中给出建议，不要动代码。

### T6（决策 D1）YAML frontmatter 子集解析

**问题**：文档主推 YAML frontmatter，但当前 `---` 分支先试 TOML，失败后行扫描只映射
`title/version/base_path/rules`，`security` 等字段静默丢失
（`src/parser/markdown_file.rs::parse_frontmatter`）。

**三个选项**：

| 选项 | 做法 | 优点 | 缺点 |
| :--- | :--- | :--- | :--- |
| A（推荐） | 手写 YAML 子集解析器，不新增依赖 | 与现有"轻量回退器"哲学一致；可控 | 只支持文档实际用到的子集 |
| B | 引入维护中的 YAML crate（如 `saphyr` / `serde_yaml_ng`） | 完整 YAML 支持 | 新依赖需评审；项目曾主动移除 `serde_yaml` |
| C | 文档/示例统一迁移到 TOML frontmatter | 零解析器代码 | 文档大改；与生态习惯不一致 |

**若选 A，建议的最小语法**（仅覆盖当前文档）：

- 顶层 `key: value` 标量；引号剥离规则与现实现一致。
- 顶层 `key: |` 块标量（保留现有 `rules` 测试的 `"\n"` 结尾行为）。
- `security:` + 缩进 `- key: value` 列表 → `serde_json::Value::Array`（对象元素）。
- 未识别的顶层标量 → `FileMetadata.extra`（`Value::String`）；未识别嵌套 → 忽略并 `tracing::warn!`。
- 不支持锚点/别名/多文档/折叠标量；遇到时回退为现有行为，不 panic。

**必须新增的测试**（`src/parser/markdown_file.rs` `mod tests`）：

- `test_frontmatter_yaml_security_list`
- `test_frontmatter_yaml_rules_block_scalar`
- `test_frontmatter_yaml_unknown_scalar_to_extra`
- `test_frontmatter_malformed_does_not_panic`
- 现有 `test_parse_frontmatter`、`test_frontmatter_toml_parsing` 保持通过。

**验收**：YAML 示例中的 `security` 不再丢失；TOML 路径行为不变；无新增 panic 路径。

---

### T7（决策 D2）设计元数据消费 / AI Memory

**问题**：`FileMetadata.title/version/rules/security` 没有任何运行时/AI 消费方；
`rupost ai assert` 只接收自然语言文本，不接收文件上下文
（`crates/rupost-ai/src/translator.rs` 的 system prompt 是固定的）。

**需要人类先回答**：

1. `rules/security` 是否进入 `rupost ai assert` / 未来 `ai bootstrap` 的上下文？
2. 是否进入 `rupost test` 的报告/警告（例如违反 `rules` 的静态检查）？
3. 若都不做，是否在 README / 生命周期规范中降级表述为"仅解析、暂不消费"？

**建议**：在本批次只做 3（文档降级），把 1/2 并入 AI 史诗；不要为了"看起来完整"而
把规则文本塞进 system prompt（会带来注入与成本问题）。

---

### T8（决策 D3）Mock DSL 操作符与延迟

**问题**：`user_story` 与 `examples/lifecycle_showcase/order_spec.md` 使用
`item_count > 100`；当前只支持 `==` / `None`，且未支持语法会 fail-open（T1 修复后变为报错）。

**若确认要做，最小范围**：

1. 扩展条件解析（`MockCompiler::parse_condition_expression`）：按最长匹配识别 `==`、`!=`、`>=`、`<=`、`>`、`<`、`contains`、`exists`。
2. `CompareOp` 增加对应枚举；Header/Query 用字符串比较；Body 先取 JSON 值：
   - 两边都能解析为数字时按 `f64` 比较；
   - 否则 `>`/`<` 按字符串字典序（文档必须写明）；
   - `contains`/`exists` 语义复用现有 `variant.rs` 分支。
3. 表驱动测试：每个 source × 每个 operator × 数字/字符串/缺失值。
4. `@mock-delay 200ms`：给 `MockVariant` 增加可选延迟，Axum handler `tokio::time::sleep`；
   测试用短延迟（如 10ms）断言不 panic，不做精确计时断言。

**非目标**：任意表达式引擎、逻辑与/或组合、模板化条件。

---

### T9（决策 D8/D9/D10）双向引导：`.md` 生成与反向提升

**背景（2026-11-04 用户反馈）**："`.md` 设计态 SSOT 只能手写"不合理：手写复杂、难维护，
且存量资产（OpenAPI/curl/历史/`.http`）无法迁移。裁决见
[`http_md_role_contract.md`](../architecture/http_md_role_contract.md) §3.5。

**目标**：

1. `.md` 可从既有资产生成（bootstrap），手写只用于补充正文/规则。
2. `.md → .http` 可确定性编译；`.http → .md` 可显式反向提升（有内联快照时结构化还原）。
3. 全程有 provenance 与漂移检测；**不做自动双写/文件监听**。

**非目标**：

- 不做文件监听、自动回写、三方合并。
- 不保证 prose/标题/评审结论可恢复（F3 不可恢复，用 TODO 占位）。
- 不在本任务实现 AI 语义增强（确定性生成先行；AI 可后续挂接）。
- 不追求 `curl → .md` 生成完整契约（没有设计信息，只能生成 `@test` 草稿）。

**建议命令面**（最终由 D8 确认）：

| 命令 | 输入 | 输出 | 说明 |
| :--- | :--- | :--- | :--- |
| `rupost import openapi <spec.json> -o api.md` | OpenAPI 3 JSON | `.md` | 冷启动主通道；纯确定性（AI 增强另由 `ai gen` 挂接） |
| `rupost import curl "<curl>" --to-md -o case.md` | curl | `.md` | 单请求 `@test` 草稿 |
| `rupost generate out.md --to-md --last N [--with-mock]` | 历史 | `.md` | N 个 `@test` 块；`--with-mock` 用录制响应生成 `@mock-default` |
| `rupost spec promote case.http -o case.md` | `.http` | `.md` | 有 `# @spec-*` 快照 → F1 还原；无 → 请求级草稿 + TODO |
| `rupost spec compile api.md -o api.http [--check]` | `.md` | `.http` + 快照 | 确定性；`--check` 只比对不写 |

**实施切片（建议按顺序，每片独立提交/验收）**：

1. **T9.0 双角色块前置（parser/compiler，必须先做）**
   - `src/parser/markdown_file.rs::parse_mock_block` 增加"请求体"状态：
     `RequestLineAndHeaders` →（空行）`RequestBody` → `VariantHeaders`/`VariantBody`。
   - 请求体终止条件：遇到 `@mock-*`，或已知请求元数据行（`@test`/`@name`/`@assert`/
     `@capture`/`@skip`/`@timeout` 等）。元数据行在请求体之后仍必须被正确解析。
   - `MockCompiler::compile`：编译**所有含 `mock_variants` 的块**（不论 `is_test`）；
     只有 `@test`、没有变体的块跳过。
   - `MockCompiler` 路由 path 归一化：绝对 URL → 取 path；`{{base_url}}/...` → 去掉变量前缀；
     相对路径原样。禁止把变量/scheme 注册进 Trie。
   - 测试：带 JSON body 的 mock 块解析；双角色块（`@test` + `@mock-default`）测试执行 + Mock
     路由双通；既有 `mock_integration_test` 全绿；`@assert` 在 body 之后仍生效。
2. **T9.1 OpenAPI → `.md`（最高优先）**
   - 新模块 `src/openapi/{model,parser,emit}.rs`（parser 不依赖 mock/runner）。
   - 解析 OpenAPI 3.0/3.1 **JSON**：`info`、`servers`、`paths`、`operationId`、parameters、
     requestBody、responses、局部 `$ref`（`#/components/...`；循环引用检测）。
     MVP 不支持 YAML/外部 `$ref`/Swagger 2.0，显式报错（与 `ai gen` 计划口径一致）。
   - `servers[0].url` 归一化：提取 origin（写入 frontmatter `host`）与 path 前缀；
     契约块 URL 使用**从 `/` 开始的完整相对路径**（含 servers path 前缀），
     **不写 `{{base_url}}`，不重复写 `base_path`**，避免 Mock 路由与测试 URL 分叉。
   - 每个 operation 生成**一个双角色块**（T9.0 已支持）：

     ````markdown
     ## createOrder — Create order

     ```http
     @test
     @name create_order
     POST /v1/orders
     Content-Type: application/json

     {"item_count": 5}

     @assert status == 201
     @assert body.id exists

     @mock-default
     HTTP/1.1 201 Created
     Content-Type: application/json

     {"id": "order_1", "status": "pending"}
     ```
     ````

   - frontmatter 只写确定字段（`title`/`version`/`host`；`base_path` 默认不写，避免与块内
     完整路径双重前缀）；每个块标题由 `operationId` 或 `METHOD path` 派生。
   - 生成完成后打印下一步命令（DX）：

     ```text
     [✓] 已生成 api.md（12 个双角色块）
         rupost mock api.md --port 9000
         rupost t api.md --var base_url=http://127.0.0.1:9000
     ```

   - 无法确定的内容用 `<!-- TODO(rupost): ... -->`，不编造；多响应按 Q3 裁决处理。
   - 防覆盖：输出已存在则拒绝（与 `import` 一致）；不新增依赖（`serde_json` 已有）。
   - 测试：`tests/fixtures/petstore.json`（自造最小 spec，不引外网）→ 生成的 `.md` 可被
     `rupost mock` 起服；`rupost t --var base_url=...` 对着本地 mock 零外网全绿；
     负例夹具（缺字段/错类型）必须失败；坏 spec 报错含 JSON 路径；
     局部 `$ref`/循环引用/缺 `servers` 的降级。
3. **T9.2 `--to-md`（curl / history）**
   - `import curl --to-md`：复用现有 `CurlImportRequest`，发射 `.md`：标题（method+path）+
     `@test` 块 + `# @assert status == 200`；若 `--with-mock` 则在同一块追加
     `@mock-default`（响应体用占位/TODO，不能伪造）。
   - `generate --to-md`：每条历史一个双角色块（`@test` + 可选 `@mock-default`）；
     `--with-mock` 时把录制响应（状态/Content-Type/JSON body）作为同块 `@mock-default`。
   - 输出防覆盖；敏感 Header/URL query 仍模板化；Body 敏感键警告（与现有生成器一致）。
   - 测试：curl→md 可被 `rupost test` 解析；history→md 的双角色块可被 `rupost mock` 编译；
     无历史不建文件；输出已存在拒绝。
4. **T9.3 `spec promote`（`.http → .md`）**
   - 有 `# @spec-*` 快照：从快照还原契约信息，把 `.http` 请求提升为双角色块
     （有 mock 信息则加 `@mock-default`，否则只加 `@test`）。
   - 无快照：每个请求一个 `@test` 块 + status 断言；在文件顶部写
     `<!-- TODO(rupost): 缺少契约快照，schema/mock 未还原 -->`。
   - 不把请求体/`@assert` 反推成响应 schema。
   - 测试：round-trip（`.md → .http → .md`）结构等价（F1 字段逐个比较）；
     无快照路径的 TODO 标注；敏感值不新增明文。
5. **T9.4 `spec compile` + provenance**
   - 编译只输出 `@test` 请求块（双角色块按测试请求导出）+ 顶部 `# @spec-*` 快照；
     `.md` 没有 `@test` 块时只输出快照 + 注释（仍可被 `mock` 使用，避免生成"无请求的空文件"）。
   - 编译产物顶部写 `# @generated-from <md 相对路径>` + `# @generated-hash <sha256>` + `# @spec-*`。
   - 确定性：无时间戳/随机 UUID；同输入输出字节一致。
   - `--check`：与磁盘现有文件比较，漂移时打印 diff 摘要并 exit 1。
   - `--force` 才允许覆盖；被手工修改（无 provenance/哈希不符）时默认拒绝并提示
     `promote` 或 `--detach`。
   - 测试：确定性（连续两次输出相等）；漂移检测；`--detach` 后不再视为派生文件；
     快照可被 `promote` 还原。
6. **T9.5 增量维护（可后置，MVP 不阻塞）**
   - `spec update <md> --from <draft.md>`：按 `@name` 锚点替换代码块，保留正文/顺序。
   - 无锚点的新块追加到文末；删除块默认保留（需 `--prune`）。
   - 该切片不阻塞 T9.0–T9.4；MVP 可先只支持"新建文件 + 拒绝覆盖"。

**验收（T9 整体）**：

- [ ] 从零开始：`import openapi petstore.json -o api.md` → `mock` + `t` 零外网闭环全绿。
- [ ] `import curl --to-md` / `generate --to-md` 产出的 `.md` 可被 `test`/`mock` 直接消费。
- [ ] 有快照的 `.http` 经 `promote` 还原出结构等价的 `.md`（F1）。
- [ ] `spec compile --check` 能阻断手工漂移；不产生时间戳/随机值。
- [ ] 生成/提升全程凭据模板化，无新增明文落盘。
- [ ] 不新增依赖（OpenAPI 用 `serde_json`；哈希用标准库/`sha2` 需评审——若项目无 `sha2`，
  可先用 `std::hash` + 版本号，或复用现有依赖；D10 决策）。
- [ ] 所有生成器测试离线、可并行、无 TempDir 泄漏。

**决策项**：

- **D8 命令面**：`import openapi` + `--to-md` + `spec promote/compile`（推荐）vs 全部塞进
  `ai gen` / `export`。推荐前者：职责清晰，确定性不依赖 AI。
- **D9 保真度**：默认只带结构契约（F1）；`--with-design` 才携带 rules/security。
- **D10 哈希与更新策略**：`sha2` 新依赖 vs 现有依赖；MVP 是否只支持新建文件。

---

## 7. Phase 3：全生命周期史诗（需人类批准）

> **不要在本批次直接开始。** 以下为切片级设计，用于批准后逐片实施。
> 每片独立提交、独立验收；任一片可中止而不影响已交付部分。

### 7.1 契约 AST 与解析器

**目标**：让 `.md` 与 `.http` 都能表达契约，且与执行依赖分离。

**数据模型（建议新增 `src/contract/types.rs`）**：

```rust
pub struct EmbeddedSpec {
    pub version: Option<String>,
    pub base_path: Option<String>,
    pub host: Option<String>,
    pub routes: Vec<EmbeddedSpecRoute>,
}

pub struct EmbeddedSpecRoute {
    pub method: String,
    pub path: String,
    pub schema: Option<serde_json::Value>,
    pub deprecated: Option<String>,
    pub sse: bool,
    pub sse_events: std::collections::HashMap<String, serde_json::Value>,
    pub websocket: bool,
    pub ws_client_send: std::collections::HashMap<String, serde_json::Value>,
    pub ws_server_push: std::collections::HashMap<String, serde_json::Value>,
}
```

**扩展点**：

| 结构 | 新增字段 | 说明 |
| :--- | :--- | :--- |
| `FileMetadata` | `host: Option<String>` | 契约本地 Host；`extra` 中已有同名值时以字段为准 |
| `ParsedFile` | `extends_spec_path: Option<PathBuf>`、`extends_spec_version_req: Option<semver::VersionReq>`、`embedded_spec: Option<EmbeddedSpec>` | 契约继承不进 DAG |
| `RequestMetadata` | `deprecated: Option<String>`、`spec_response_schema: Option<serde_json::Value>` | `.md` 块内契约声明 |
| `Metadata` 枚举 | `Deprecated(String)`、`SpecResponseSchema(String)` | `parse_metadata` 增加分支 |

**解析规则**：

1. `@extends-spec <path> (version >= x.y.z)`：
   - 只允许一行；多行 → `ParseError::InvalidMetadata`。
   - 只写入 `extends_spec_path/version_req`，**不**写入 `dependencies`。
   - 路径相对当前 `.http` 文件解析。
2. `# @spec-* = ...`：
   - 行扫描 `.http` 顶部连续注释；`=` 前为 key，后为 value。
   - `@spec-route = METHOD PATH (opts)`：`opts` 是括号内的 `key: value` 列表；
     按顶层逗号切分（必须感知 `{}`、`[]`、引号），值用 `serde_json::from_str` 解析。
   - 支持 `schema`、`deprecated`、`sse` + `events`、`websocket` + `client_send` + `server_push`。
   - 非法 JSON → 带行号的 `ParseError`，不得 panic。
3. `.md` 块内 `# @spec-response-schema = {...}` / `@deprecated ...`：
   - 复用 `parse_request_block` 的元数据提取路径（已会剥离 `#`/`//`）；
   - 在 `parse_metadata` 增加分支，字段落到 `RequestMetadata`。

**测试**：

- 表驱动：`method/path`、`schema` 嵌套、`deprecated` 带引号、`sse`/`websocket` 选项、坏 JSON、括号/引号边界。
- 单继承：两行 `@extends-spec` → 必失败。
- `@extends-spec` 不得出现在 `ParsedFile.dependencies`。

### 7.2 契约编译器与 Mock 内联降级

**目标**：`.md` 可编译为 `CompiledSpec`；Mock 在没有 `.md` 时可用 `.http` 内联快照起服。

**建议模块**：`src/contract/compiler.rs`（新模块，parser 不感知 mock）。

```rust
pub struct CompiledSpec { pub routes: Vec<CompiledRoute>, pub host: Option<String> }

pub fn compile_spec(parsed: &ParsedFile) -> CompiledSpec;
```

- `routes` 来源：非 `@test` 请求的 `method/url` + `spec_response_schema` + `deprecated` + `mock_variants`。
- `.md` 的 `mock_variants` 仍走现有 `MockCompiler`；`EmbeddedSpecRoute` 走新增
  `MockCompiler::compile_embedded(&ParsedFile)`。
- `run_server_from_file`：
  - `.md` → 现有路径；
  - `.http` 且 `embedded_spec.is_some()` → 编译内联路由；
  - `.http` 无内联契约 → 保持现有"请求回显"兜底行为（并在帮助文本说明）。

### 7.3 运行期契约校验（含 Host 逃逸与弃用）

**目标**：`rupost test order_test.http` 在 `.md` 或内联快照存在时做契约校验。

**接入点（推荐：不改 Batch/Parallel 的调用面）**：

- 在 `TestExecutor::execute_all` 入口解析一次契约：
  - `let contract = contract::loader::load_for_parsed_file(&parsed_file)?;`
  - `load_for_parsed_file` 规则：有 `extends_spec_path` → 优先读物理 `.md`；缺失则用
    `embedded_spec`；都没有 → 返回 `Ok(None)` 并 `tracing::warn!`（蓝图裁决：不因缺契约而失败）。
  - 没有 `extends-spec`/内联契约 → `Ok(None)`，不做校验（避免误伤普通用例）。
  - 这样 `BatchExecutor`/`ParallelExecutor`/TUI/CLI inline 全部无需感知契约。
- `execute_one` **不新增参数**；在 `execute_all` 拿到 `result` 后统一做校验：
  - 校验结果追加为 `AssertionResult`，便于 reporter/JSON 原样展示：
    - `raw`: `contract: POST /api/v1/orders`
    - `expected`: schema 摘要
    - `message`: 缺失字段/类型不符/host 逃逸说明
  - 任一失败 → `result.success = false`；这样 reporter/JSON 无需新增字段。
  - 契约加载失败（IO/解析）建议直接返回 `Err`，让用户看到明确错误，而不是静默跳过。
- 不建议新增 `execute_all_with_contract` 公共 API：调用点太多，且契约来源可由
  `ParsedFile` 自身决定。
- 规则：
  - Host 逃逸：请求 Host ∈ {`spec.host`, `localhost`, `127.0.0.1`, `::1`} → 校验；
    否则跳过契约校验（可选记一条 `warnings`，不判失败）。
  - 路由匹配：复用/抽取 `mock/trie.rs` 的路径匹配思想；不要在 runner 里复制 mock 的 Trie 逻辑，
    建议把"路径模式匹配"抽成 `utils/path_match.rs` 纯函数并双端复用。
  - Schema 超集：递归对象；数组只校验类型；数字接受 `number`/`integer`；
    `null` 值按"字段存在且为 null"处理；多余字段允许。
  - 弃用：命中 `deprecated` 路由或请求带 `@deprecated` → `warnings` 追加
    `[DEPRECATION] ...`；`rupost.toml` 增加 `fail_on_deprecation: bool` 时置 `success = false`。

**测试**（离线）：

- wiremock/本地 TcpListener 提供固定响应，断言：
  - 命中契约 + schema 匹配 → 成功；
  - 缺字段/类型错 → 失败且 message 可见；
  - 第三方 Host → 跳过校验；
  - 弃用路由 → warning；`fail_on_deprecation` → 失败。

### 7.4 契约编译导出（CI 分发）

> 与 Phase 2 T9.4 的 `spec compile` 是同一实现；T9 先交付最小可用版（含 provenance/`--check`），
> 本切片再补 `@extends-spec` 路径改写、增量更新与完整快照字段。

**需要决策 D5：命令面**。蓝图写的 `rupost export ... --all-in-place` 与现有
"请求 → curl/httpie/wget" 冲突，建议新增独立子命令：

```bash
rupost spec export <spec.md> --target <file.http> [--in-place] [--force]
```

**行为**：

1. 解析 `.md` 为 `CompiledSpec`。
2. 读目标 `.http`（若存在），删除所有 `# @spec-` 行。
3. 在顶部插入稳定排序的 `# @spec-version/base-path/host/route` 行。
4. `--in-place` 时覆写目标；默认输出到 stdout 或 `--output`。
5. 不修改 `@extends-spec` 行（保留快照来源信息）；是否改写为 basename 属决策项。

**测试**：round-trip——`spec export` 产物再解析出的 `EmbeddedSpec` 与源 `CompiledSpec` 等价。

### 7.5 SSE / WebSocket 契约审计（可延后）

- 解析 `events` / `client_send` / `server_push`。
- 在 `SseRunner` / `WsRunner` 的帧处理路径上按 `event` / `type` 分流，做弱 Schema 校验。
- 失败追加 `AssertionResult`；未知事件按契约缺失处理（警告或失败，需决策）。
- 测试：本地 SSE/WS server，逐帧断言；未知事件、类型错、缺字段。

### 7.6 Mock 数据自动生成插件（可延后）

- `MockGeneratorPlugin` trait（`generate_mock_body(schema)`）；默认实现按 Schema 类型返回零值。
- 通过 `rupost.toml` 配置启用；无插件时保持现有"响应体=请求体"兜底。
- 测试：空 body + schema → 生成对象；插件注册/查找；错误降级。

### 7.7 验收夹具：`examples/lifecycle_showcase/`

当前夹具不可执行，原因：

| 夹具内容 | 当前实现 | Phase 3 需要 |
| :--- | :--- | :--- |
| `# @mock-when $.body.item_count > 100` | `#` 前缀不被识别；`>` 不支持 | 注释前缀剥离 + 操作符（T8） |
| `### @mock-variant ...` 分界 | 无此 DSL | **建议改夹具为规范语法**（`@mock-when` / `@mock-default`），不加别名 |
| `@spec-response-schema` | 未解析 | 7.1 |
| `@deprecated` | 未解析 | 7.1/7.3 |
| `@extends-spec` | 未解析 | 7.1 |
| TOML `host` | 落在 `extra` | 7.1 |
| `order_test_exported.http` | 静态期望文件 | 7.4 生成并比对 |

**规范化建议**：把 `order_spec.md` 的第二个变体显式写成 `@mock-default`；
把 `### @mock-variant ...` 保留为人类可读注释即可，不作为解析指令。

**离线验收**（不要打真实外网）：

1. 复制夹具到 `tests/fixtures/lifecycle_showcase/`，把微信支付用例改为本地 mock 地址或 `@skip`。
2. `rupost mock order_spec.md --port 9000` 后，用本地 HTTP 客户端断言：
   `item_count=120` → vip 响应；`item_count=5` → 标准响应；legacy → 200。
3. `rupost test order_test.http --var base_url=http://127.0.0.1:9000` → 契约校验通过；
   缺字段/错类型时失败。
4. `rupost spec export order_spec.md --target /tmp/order_test.http --in-place` → 产物解析出的
   路由与源契约一致。
5. 手工冒烟（可选、需外网）：用 `examples/lifecycle_showcase/` 原文件验证第三方逃逸与弃用警告。

---

## 8. 决策队列（必须由人类确认）

| ID | 决策 | 推荐 | 阻塞任务 |
| :--- | :--- | :--- | :--- |
| D1 | YAML frontmatter：手写子集 / 引 crate / 迁 TOML | A 手写子集 | T6 |
| D2 | `rules/security` 是否进入 AI/报告 | 先文档降级，AI 并入史诗 | T7 |
| D3 | 是否扩展 Mock 操作符与延迟 | 是，按 T8 最小范围 | T8、7.7 |
| D4 | `@extends-spec` 语法与路径改写规则 | 只允许单继承；不改写路径 | 7.1/7.4 |
| D5 | 契约导出命令面 | 新增 `rupost spec export`，不扩 `export` | 7.4 |
| D6 | SSE/WS 契约审计失败策略 | 未知事件=警告，schema 错=失败 | 7.5 |
| D7 | `fail_on_deprecation` 配置位置 | `rupost.toml` 顶层 bool | 7.3 |
| D8 | `.md` 生成/提升命令面 | `import openapi` + `--to-md` + `spec promote/compile`；不塞进 `ai gen`/`export` | T9 |
| D9 | `.md` 生成保真度 | 默认 F1 结构契约；`--with-design` 才携带 rules/security | T9 |
| D10 | 哈希与更新策略 | MVP 新建文件 + 拒绝覆盖；增量 update 后置；哈希方案需评审（避免无谓新依赖） | T9 |
| D11 | 生成块形态：单块双角色（`@test` + `@mock-*`）还是双块（契约 + 测试） | 单块双角色 + 相对路径为目标（一个 operation 一个块，避免重复漂移）；先做 T9.0 parser/compiler 前置；双块 + lint 仅作降级 | T9.0/T9.1、角色契约 §3.2 |
| D12 | 上游 OpenAPI 变更后的维护模型 | MVP 一次性引导；后续 `spec update --from <spec>` 做锚点增量，不做自动同步 | T9.5 |
| D13 | OpenAPI 多状态码/多响应 → Mock 变体映射 | 默认响应 + 其余状态码用合成触发条件（如 `header.X-Mock-Status`），或只生成默认 + TODO | T9.1 |
| D14 | 输出布局：单文件/按 tag/按 operation | MVP 单 spec 单文件；`--split-by-tag` 后置 | T9.1 |
| D15 | `--with-mock` 的隐私边界（历史响应体可能含 PII/密钥） | 默认关闭；开启时脱敏 + 截断 + 显式确认；不允许把明文写入 `.md` | T9.2 |
| D16 | provenance 是否包含上游 spec 哈希（检测 OpenAPI 漂移） | 是：`@generated-from <spec>` + `@source-hash`；`--check` 同时比对源与产物 | T9.4 |

---

## 9. 全局验收清单（Phase 1 收尾）

- [ ] T1：非法条件编译失败；`@mock-default` 最后；所有现有示例可 mock。
- [ ] T2：纯契约 `.md` 测试模式全 skipped，零发包。
- [ ] T3：`base_path` 回归锁 + 文档。
- [ ] T4：活动文档命令/状态修正，`grep "mock start"` 只剩历史文档。
- [ ] T5：`progress_summary` + `CHANGELOG` 更新。
- [ ] `make gate-fast` 通过。
- [ ] `make gate` 通过（或基线失败清单与本次无关，已记录）。
- [ ] 每个任务一个原子提交，提交信息符合 `fix:`/`test:`/`docs:` 约定。
- [ ] 没有新增依赖、没有无关格式化、没有改 `TestResult`/JSON 报告字段。

---

## 10. 交接报告模板（完成后写给人类）

```markdown
## 批次结果

- 分支/提交：<jj change id 或 git sha>
- 基线：`make gate` <通过/失败项>
- 改动文件：
  - ...
- 新增/修改测试：
  - ...
- 验证命令与结果：
  - `cargo test ...` → ...
  - `make gate` → ...
- 未完成/阻塞：
  - ...
- 发现的额外问题（未修）：
  - ...
- 是否触碰 Phase 2/3：<否/是 + 理由>
```

---

## 附录 A：关键代码索引

| 主题 | 文件 | 函数/位置 |
| :--- | :--- | :--- |
| Markdown 解析 | `src/parser/markdown_file.rs` | `parse_content`、`parse_frontmatter`、`parse_mock_block`、`extract_code_blocks` |
| `.http` 解析 | `src/parser/http_file.rs` | `parse_content`、`split_by_separator`、`parse_request_block`、`extract_dependencies` |
| 元数据 | `src/parser/metadata.rs` | `parse_metadata`、`apply_metadata` |
| AST | `src/parser/types.rs` | `FileMetadata`、`ParsedRequest`、`RequestMetadata`、`ParsedFile` |
| 执行 | `src/runner/executor.rs` | `execute_all`、`execute_one` |
| 批处理/依赖 | `src/runner/app.rs`、`engine/resolver.rs`、`engine/workflow.rs` | `run_test_suite`、`resolve_and_parse` |
| Mock 编译 | `src/mock/compiler.rs` | `compile`、`parse_condition_expression` |
| Mock 匹配 | `src/mock/matcher.rs`、`trie.rs`、`variant.rs` | `match_request`、`add_route`、`VariantCondition::evaluate` |
| Mock 启动 | `src/mock/server.rs` | `run_server_from_file` |
| URL/`base_path` | `src/http/url.rs` | `resolve_final_url`、`join_paths` |
| 导出跳过规则 | `src/exporter/preparer.rs` | `prepare`（`has_explicit_tests` / mock 变体跳过） |
| 历史 → `.http` 生成 | `src/generator/mod.rs`、`src/generator/http.rs` | `run_generate`、`generate_with_variables`、`format_entry` |
| curl → `.http` 导入 | `.workspace/rupost-ai/src/importer/curl.rs`（主线未合并） | `tokenize`、`parse_curl_to_entry`、`run_curl_import` |
| OpenAPI → `.md`（T9 新增） | `src/openapi/{model,parser,emit}.rs`（规划中） | parser/emit；不依赖 mock/runner |
| 双向提升/编译（T9 新增） | `src/spec/{promote,compile}.rs` 或 `src/generator/`（规划中） | `spec promote` / `spec compile --check` |
| AI 断言 | `crates/rupost-ai/src/translator.rs` | `AssertionTranslator::translate` |

## 附录 B：可复现命令

```bash
# 当前行为复现（T1 修复前）：非法条件变兜底
# 1. 写临时 md，含 @mock-when query.age > 18
# 2. cargo test --test mock_integration_test（或临时单测）
# 预期（修复前）：编译成功且该变体无条件命中
# 预期（修复后）：compile 返回 Err

# T2 复现：纯契约 md 在 test 下被执行
# 用 MarkdownFileParser::parse_content 解析仅含 @mock-when 的块，
# TestExecutor::execute_all 应全部 skipped（修复后）。

# 调用点盘点
grep -rn "MockCompiler::compile" src/ tests/
grep -rn "has_explicit_tests" src/
grep -rn "mock start" README.md doc/ AGENTS.md .agents/ 2>/dev/null
```

## 附录 C：`lifecycle_showcase` 能力矩阵

| 能力 | 展示文件 | 现状 | 所属切片 |
| :--- | :--- | :--- | :--- |
| TOML frontmatter + `host` | `order_spec.md` | 解析到 `extra`，未消费 | 7.1/7.3 |
| `@spec-response-schema` | `order_spec.md` | 忽略 | 7.1/7.3 |
| `# @mock-when ... > 100` | `order_spec.md` | 未识别 + 无操作符 | T8 + 7.2 |
| `### @mock-variant` 分界 | `order_spec.md` | 无此 DSL | 7.7（建议改规范语法） |
| `@deprecated` | `order_spec.md` | 忽略 | 7.1/7.3 |
| `@extends-spec` | `order_test.http` | 忽略 | 7.1 |
| `# @spec-route` 内联快照 | `order_test_exported.http` | 忽略 | 7.1/7.2 |
| 契约校验/第三方逃逸 | `order_test.http` | 无 | 7.3 |
| CLI 契约导出 | 全部 | 无 | 7.4 |

## 附录 D：术语

- **SSOT**：Single Source of Truth，单一事实来源。
- **fail-open / fail-closed**：失败开放（错误配置静默放行/命中）vs 失败关闭（错误配置直接报错）。
- **契约块**：含 `@mock-when`/`@mock-default` 的无 `@test` 代码块。
- **内联快照**：`.http` 顶部以 `# @spec-*` 注释承载的契约摘要。
- **弱 Schema 超集校验**：期望 schema 的字段必须存在且类型匹配，实际响应允许多余字段。

## 附录 E：查漏补缺与待确认清单（2026-11-04）

> 用途：T9 开工前的开放问题与边界审查。**未确认前不要自行选择默认值**；
> 决策后回填 §8，并在本附录标记关闭。
> 分级：**阻塞**=不确认会返工；**重要**=影响体验/维护；**工程**=实施细节但需提前定。

### E.1 阻塞 T9 的 10 个问题

| # | 问题 | 为什么必须确认 | 建议默认 | 影响 |
| :--- | :--- | :--- | :--- | :--- |
| Q1 | 生成块形态：单块双角色（`@test` + `@mock-*`）还是双块（契约块 + `@test` 块）？ | 双块会重复 method/URL/headers/body，手改易漂移；单块需要补 parser 请求体/元数据与 MockCompiler 语义 | **推荐单块双角色 + 相对路径**（一个 operation 一个块）；先做 T9.0 前置；双块 + lint 仅作排期降级 | T9.0/T9.1、T2、角色契约 §3.2 |
| Q2 | 生成后的维护模型：一次性 bootstrap、还是可持续 regenerate？ | 用户核心痛点是"难维护"；一次性生成后 OpenAPI 变更仍需手改 | **MVP 一次性引导**；`spec update --from <spec>` 锚点增量后置，不做自动同步 | T9.4/T9.5 |
| Q3 | OpenAPI 多状态码/多响应如何映射 Mock 变体？ | OpenAPI 没有"请求条件"；一个 operation 可能有 200/400/401 多个响应，无法直接变成 `@mock-when` | 默认响应 → `@mock-default`；其余状态码 → 合成触发条件（如 `header.X-Mock-Status == 400`）或只生成 TODO | T9.1 |
| Q4 | `import openapi` 与已规划的 `rupost ai gen --spec` 关系？ | 两个命令都做 OpenAPI→`.md` 会重复、命名冲突、用户困惑 | `import openapi` 做确定性核心；`ai gen` 只做可选增强层（或直接合并） | D8、路线图 |
| Q5 | 输出布局：单 spec 单文件 / 按 tag / 按 operation？ | 大 spec 单文件巨大、diff 噪音、merge 冲突；拆分又增加导航成本 | MVP 单 spec 单文件；`--split-by-tag` 后置 | T9.1 |
| Q6 | `--with-mock` 的隐私边界：历史响应体可能含 PII/密钥/大 body | 生成 `.md` 会落盘，可能违反安全约定 | 默认关闭；开启时脱敏+截断+显式确认；不得写明文凭据 | T9.2 |
| Q7 | 单一写主/禁止自动双写是否确认？ | 若团队期望"两边都能改、自动同步"，当前契约会被认为不可用 | 保持"`.md` 写主 + 显式 promote/compile"；自动同步明确 Non-goal | 契约 §3.5/6.5 |
| Q8 | provenance 粒度：是否记录上游 spec 路径+哈希？ | 只记录产物哈希无法发现 OpenAPI 源变更 | 记录 `@generated-from` + `@source-hash` + `@generated-hash`；`--check` 双比对 | T9.4、D16 |
| Q9 | 往返/验收标准：结构等价还是字节等价？如何避免"自证式"测试？ | Mock 与测试都由同一 spec 生成，自环测试可能永远通过，不能证明契约校验有效 | 结构等价（F1）；额外用**负例夹具**（缺字段/错类型/错 host）证明校验会失败 | T9 验收 |
| Q10 | 保真度边界：`--with-design` 是否携带 rules/security？ | 可能泄露敏感设计；不携带则反向生成有信息损失 | 默认 F1；`--with-design` 显式 opt-in，并在产物中标注 | D9、T9.3 |

### E.2 OpenAPI 功能覆盖（MVP 白名单需确认）

| 特性 | MVP 建议 | 未支持时行为 |
| :--- | :--- | :--- |
| OpenAPI 3.0 / 3.1 JSON | 支持 | Swagger 2.0、YAML 显式报错（依赖评审） |
| 局部 `$ref`（`#/components/...`） | 支持，含循环检测 | 外部/远程 `$ref` 拒绝（不联网） |
| `allOf` / `oneOf` / `anyOf` / `discriminator` | 至少支持 `allOf` 合并；其余降级为 TODO | 不允许静默生成错误断言 |
| `nullable` / 3.1 type 数组 / `format` / `enum` | 支持基础映射 | 无法映射时保留原始 schema 注释 |
| parameters：path/query/header/cookie、required、default、example | 支持常用风格；`style/explode` 复杂组合降级 | 缺失值用占位符 + TODO |
| requestBody：JSON / form-urlencoded / multipart | JSON、form 支持；multipart 当前 runner 不支持 | multipart 生成 TODO/`@skip`，不伪造 |
| securitySchemes：apiKey/basic/bearer/oauth2 | 生成 `{{token}}`/`{{api_key}}` 占位 | 不生成真实密钥；OAuth 流程不展开 |
| responses：多状态码、content types、examples、headers | 主响应 + 示例；其余按 Q3 | 二进制/非 JSON 降级为 TODO |
| servers：多个、变量、相对 URL | `servers[0]`；变量用占位/默认值 | 多环境映射后置 |
| tags / operationId / summary / deprecated | 标题与 `@name` 来源；deprecated 生成警告 | 缺失 operationId 用 method+path 派生 |
| webhooks / callbacks / links / externalDocs | Out of scope | 显式忽略并记录 |
| 大批量 operation / 超大 schema | 顺序处理；后续考虑 split/增量 | 需设上限与可读错误，避免 OOM |

### E.3 `.md ↔ .http` 语义缺口

- **`.md → .http`**：只编译 `@test` 块还是也输出契约块？SSE/WS、`@skip`、`@stream_to`、
  `@diagnose`、`@capture`、`@depends-on`、`@extends-spec` 如何保留？
- **`@test` + Mock 单块**：生成物统一用相对路径（`/v1/orders`）作为请求行，测试时由
  `base_url` 补主机、Mock 直接注册 path；若用户手写 `{{base_url}}/path` 或绝对 URL，
  T9.0 的 path 归一化负责提取 path（禁止把变量/scheme 注册成路由）。是否需要
  `@mock-path` 作为逃生口，待 Q1 确认。
- **`.http → .md`**：哪些请求变成 `@test`？哪些变契约块？`@assert` 是否全部保留？
  注释/正文/顺序如何生成？多文件如何合并？
- **更新/合并**：上游变化时按 `@name` 锚点替换哪些块？手改进 `.http` 与 `.md` 冲突谁赢？
  删除的 operation 是否保留（避免误删人工用例）？MVP 是否只支持新建文件？
- **命名/定位**：operationId 缺失/重复、路径参数变化、`@name` 截断/冲突、大小写、
  多版本 API（`/v1`、`/v2`）如何分组。
- **排序/确定性**：paths/operation 顺序、JSON map 顺序、换行（LF/CRLF）、BOM、Unicode、
  格式化宽度；同一输入跨平台/跨 serde 版本是否字节一致。
- **内联快照体积**：大 schema 放进 `.http` 顶部注释是否可读？是否需要 sidecar/
  压缩/`--no-snapshot`？通用客户端是否仍能接受超长注释行？

### E.4 工程、安全与流程缺口

- **安全**：不可信 OpenAPI（超大文件、YAML 炸弹、恶意 `$ref`/路径穿越）、输出路径穿越/
  符号链接、生成文件权限、`rules/security` 与示例中的密钥/PII、AI 增强时的 prompt injection。
- **CI/Git**：生成 `.md`/`.http` 是否提交？CI 用 `--check` 还是现场生成？`.gitattributes`
  统一换行；生成物 diff 噪音；spec 目录是否随交付分发；缓存与哈希稳定性。
- **错误处理**：部分 operation 不支持时是整体失败还是"生成 + 警告 + exit 0"？
  `--strict` 语义、JSON 错误报告、JSON Pointer 行号、`--dry-run`/`--diff`。
- **性能**：数千 operation、超大 schema、多个 spec、历史数千条；内存/耗时/输出布局。
- **兼容性**：现有 `generate` 覆盖行为、`import -o` 防覆盖、`MockCompiler` 公共 API、
  生命周期规范中"Mock 忽略测试块"的语义若因 Q1 改变，需要版本化说明。
- **测试策略**：golden 文件、round-trip property test、tokenizer/parser fuzz、跨平台 CI、
  负例契约测试、离线自环测试；不能只测"生成成功"。
- **UX/文档**：命令别名/参数位置（`trailing_var_arg` 陷阱）、stdout 与文件、退出码、
  TUI 是否暴露 import/compile、迁移指南、限制说明、生成物示例。
- **团队协作**：`.md` 的 owner/评审人；生成物是否允许手改；谁负责上游 spec 变更；
  多团队/多服务边界；API 版本与弃用流程。

### E.5 建议的确认顺序

1. 先确认 **Q1（块形态）** 与 **Q3（多响应映射）**：它们决定 T9.1 的产物结构和 AST 改动。
2. 再确认 **Q2（维护模型）** 与 **Q7（写主/不自动同步）**：决定 T9.4/T9.5 的范围。
3. 然后确认 **Q4/Q5/Q6**：命令面、布局、隐私。
4. 最后确认 OpenAPI 白名单（E.2）与验收负例（Q9）。
5. 未确认项不要阻塞 Phase 1 的 T1/T2；这两项与 T9 无依赖，可先做。

### E.6 本附录与 §8 的对应

| 附录问题 | 决策 ID |
| :--- | :--- |
| Q1 块形态 | D11 |
| Q2 维护模型 | D12 |
| Q3 多响应映射 | D13 |
| Q4 命令面 | D8 |
| Q5 输出布局 | D14 |
| Q6 隐私 | D15 |
| Q7 写主 | 契约 §3.5/6.5（已裁决，需确认） |
| Q8 provenance | D16 |
| Q9 验收 | T9 验收标准 |
| Q10 保真度 | D9 |
