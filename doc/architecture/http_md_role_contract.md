# `.http` 与 `.md` 文件职能契约 (File Role Contract)

- **状态**：Proposed（2026-11-04 分析结论，待评审通过后改为 Accepted）
- **日期**：2026-11-04
- **适用范围**：`src/parser`、`src/runner`、`src/mock`、`src/exporter`、AI 上下文注入、CI/CD 分发
- **事实源分工**：本文件定义"应该是什么"（规范）；当前实现差距、任务与验收见
  [`2026-11-04-http-md-role-alignment-handoff.md`](../plans/2026-11-04-http-md-role-alignment-handoff.md)；
  进度唯一事实源仍是 [`progress_summary.md`](../plans/progress_summary.md)。
- **关联文档**：
  [`file-format-spec.md`](../apidoc/file-format-spec.md)、
  [`2026-06-06-markdown-lifecycle-specification.md`](../plans/2026-06-06-markdown-lifecycle-specification.md)、
  [`file_lifecycle_and_embedded_spec_blueprint.md`](file_lifecycle_and_embedded_spec_blueprint.md)、
  [`user_story_and_compliance_walkthrough.md`](user_story_and_compliance_walkthrough.md)

---

## 0. 一句话定义

- **`.md` 是设计态单一事实来源（SSOT）**：设计元数据、Mock 契约、集成测试、AI 上下文同源。
  **SSOT 不等于手写**：`.md` MUST 可从既有资产（OpenAPI/curl/history/`.http`）引导生成，
  手写只用于补充设计意图、正文与规则。
- **`.http` 是执行态工件**：自包含、可独立运行的请求用例，面向 Runner、CI 与现场交付。
- **`.md` 内的 `http`/`rest` 代码块必须是 `.http` 语法的子集**，这是"文档即测试"成立的前提。
- **双向指"双向可转换"，不是双向自动同步**：`.md` 是设计事实的写主，`.http` 是派生视图；
  回写必须显式触发并带 provenance（见 §3.5）。

---

## 1. 规范性语言

- **MUST / MUST NOT**：违反即为缺陷，必须有测试守护。
- **SHOULD / SHOULD NOT**：违反需在 PR/评审中说明理由。
- **MAY**：可选能力，不得成为其他能力的前置条件。

---

## 2. `.http` 的职能

### 2.1 MUST

1. 可被通用 HTTP 客户端（VS Code REST Client、JetBrains HTTP Client、ijhttp 等）识别与执行；
   除注释/指令外，不得引入专有边界符或破坏请求语法的结构。
2. 只承载执行所需内容：请求行、Headers、Body，以及请求级元数据
   （`@name`、`@skip`、`@timeout`、`@assert`、`@capture`、`@sse*`、`@websocket`、`@decoder`、
   `@stream_to`、`@diagnose` 等）。
3. `@depends-on` 只表达**执行拓扑**（哪个文件先跑），MUST NOT 表达"设计契约继承"。
4. 单文件可独立执行；批处理时的执行顺序由 `@depends-on` 有向无环图决定。

### 2.2 MUST NOT

1. 承载文件级设计元数据（`title`、`version`、`base_path`、`rules`、`security`、`host`）。
   这些属于 `.md` 的 frontmatter。
2. 承载 Mock 多分支契约（`@mock-when` / `@mock-default`）。
   当前实现只在 Markdown 解析器中识别它们，这是有意的职责划分，不是遗漏。
   例外：由 `.md` 编译生成的 `# @spec-mock` 内联快照（§2.3），它是派生物而非手写源。
3. 承载响应 Schema 的"设计声明"。`.http` 只写"实际期望"（`@assert`），不写"契约应该是什么"。

### 2.3 交付态扩展（Phase 3，MAY，需先实现）

当 `.md` 设计资产不能分发到 CI/现场时，`.http` 顶部 MAY 携带纯注释形式的契约快照：

```http
# @spec-version = 1.2.0
# @spec-base-path = /api/v1
# @spec-host = api.orderservice.local
# @spec-route = POST /orders (schema: {"id": "string", "status": "string"})
# @spec-mock = POST /orders (when: "query.item_count > 100", status: 201, body: {...})
```

要求：

1. 必须是普通单行注释，不得改变通用客户端的执行结果。
2. 运行期契约查找优先级：**本地物理 `.md` > `.http` 内联快照 > 无契约（仅普通测试）**。
3. 内联快照由 `.md` 编译生成，MUST NOT 手写维护为唯一来源；它同时是 `.http → .md`
   反向提升（`spec promote`）的结构化依据。
4. 快照 MUST 带 `# @generated-from` 与内容哈希，供 `spec compile --check` 做漂移检测。

---

## 3. `.md` 的职能

### 3.1 四种角色

| 角色 | 载体 | 面向对象 | 说明 |
| :--- | :--- | :--- | :--- |
| R1 设计/SSOT | YAML/TOML frontmatter | 人、评审、AI | 文件级元数据：标题、版本、base_path、host、rules、security |
| R2 Mock 契约 | 含 `@mock-when`/`@mock-default` 的 `http/rest` 块（可与 `@test` 共存） | 前端、联调、Mock 服务 | 变体决定条件响应；同一块可同时是回归用例 |
| R3 集成测试 | 含 `@test` 的 `http/rest` 块 | Runner、CI | 测试模式执行；若同时含变体，则也是 Mock 契约 |
| R4 上下文/Memory | R1 元数据 + 文档正文 | AI、工具链 | rules/security 是只读上下文，不参与执行 |

### 3.2 代码块分类规则（MUST）

**角色是正交的，不是互斥的**：同一个块可以既是测试用例（`@test`）又是 Mock 契约
（含变体），从而做到"一个 operation 一个块"，避免重复定义漂移。

| `@test` | `mock_variants` | 测试模式（`rupost test`） | Mock 模式（`rupost mock`） |
| :---: | :---: | :--- | :--- |
| 有 | 有 | 执行（双角色） | 编译为路由契约 |
| 有 | 无 | 执行（纯测试） | 忽略 |
| 无 | 有 | **跳过（MUST NOT 发真实请求）** | 编译为路由契约 |
| 无 | 无 | 文件内没有任何 `@test` 时执行（文档即测试） | 编译为默认回显路由（响应体=请求体） |

补充规则：

1. `@test` 的识别必须是**独立指令行**（剥离 `#` / `//` 前缀后以 `@test` token 开头），
   MUST NOT 使用 `content.contains("@test")` 这类子串匹配（会误伤 `admin@test.com`、注释正文）。
2. **无 `@test` 的** Mock 契约块在测试模式下永远跳过，即使用户用 `--only` 选中它；
   需要执行时，应显式加 `@test`（或未来提供 `--include-contracts`，需另立项）。
3. Mock 模式编译**所有含变体的块**，不论是否有 `@test`；只有 `@test`、没有变体的块
   MUST NOT 注册成 Mock 路由。
4. 双角色块要求请求 URL 可直接作为 Mock 路由路径（生成物使用相对路径如 `/v1/orders`）；
   契约声明中禁止将客户端变量写在 URL 动态段（如 `/orders/{{order_id}}` 为未定义反向捕获行为），
   契约路由声明统一使用 RESTful `:id` 或 OpenAPI `{id}` 标准语法；
   测试端（.http）在调用时可自由写 `{{order_id}}`；
   若 URL 含 `{{base_url}}`/绝对地址，MockCompiler MUST 提取 path（变量感知/URL 解析），
   MUST NOT 把变量或 scheme 原样注册进 Trie。Trie 匹配算法中，静态段 (Literal) 具有绝对最高优先级，严格优先于动态参数段 (Param) 匹配。
5. 双角色块的请求元数据（`@test`/`@name`/`@assert`/`@capture` 等）MUST 位于第一个
   `@mock-*` 之前；若解析器支持"请求体后元数据"，可放宽并在文档中说明。

### 3.3 元数据角色（R1/R4）

1. frontmatter 中出现在文档里的字段 MUST 被解析为 `FileMetadata`；不再支持的字段 MUST
   在文档中显式标注"未实现"，不得让用户以为已生效。
2. 当前被运行时消费的字段仅 `base_path`。`title/version/rules/security/host` 在消费方落地前，
   文档 MUST 标注为"仅解析、暂未参与执行/AI 注入"。
3. `security` 等复杂字段的解析结果 MUST 不因 frontmatter 风格（YAML/TOML）不同而静默丢失；
   要么完整解析，要么给出可见警告。

### 3.4 生命周期派生（Phase 3，目标态）

1. `.md` MUST 可被编译为 `.http` 顶部的内联契约快照。
2. 现场执行只依赖 `.http`，缺失 `.md` 时 MUST 自动降级读取内联快照。
3. `@extends-spec` 只表达契约继承，MUST NOT 进入执行拓扑（不得与 `@depends-on` 混用）。
4. 契约比对规则：
   - Host 逃逸：请求 Host 不属于"契约本地 Host 集合"时直接放行，不报"未定义路由"；
   - 路由匹配：按 Method + Path（支持 `:param`、`*`、`**`）匹配契约路由；
   - Schema 校验：弱 Schema 超集校验（期望字段存在且类型一致，允许多余字段）；
   - 弃用：命中过期路由时输出警告；配置 `fail_on_deprecation = true` 时判失败。

### 3.5 作者通道与双向转换（2026-11-04 修订）

**SSOT 不等于手写。** `.md` 作为事实来源，MUST 至少提供一条从既有资产引导生成的路径；
手写只用于补充设计意图、正文与规则，MUST NOT 成为建档的唯一方式。

**"双向"的准确含义：双向可转换，不是双向自动同步。**

| 方向 | 命令（建议名，见决策 D8） | 作用 |
| :--- | :--- | :--- |
| 既有资产 → `.md` | `rupost import openapi <spec.json> -o api.md` | 冷启动主通道：路由/Schema/示例/断言/Mock 骨架 |
| curl → `.http` | `rupost import curl "<curl>" -o case.http` | 单请求快速固化（现状） |
| curl → `.md` | `rupost import curl "<curl>" --to-md -o case.md` | 生成 `@test` 块草稿，不伪造契约 |
| history → `.http` | `rupost generate out.http --last N` | 调试历史 → 回归用例（现状） |
| history → `.md` | `rupost generate out.md --to-md --last N` | `@test` 块 + 基于录制响应的 `@mock-default` 草稿 |
| `.http` → `.md` | `rupost spec promote case.http -o case.md` | 有内联快照 → 结构化还原；无快照 → 请求级草稿 |
| `.md` → `.http` | `rupost spec compile api.md -o api.http [--check]` | 确定性编译 + 内联快照；CI 漂移检查 |
| `.md` → Mock | `rupost mock api.md` | 现状 |

**保真度必须诚实标注**：

- **F1 结构契约**（可完整恢复）：method/path、Schema、deprecated、mock 变体、状态/Headers/Body。
- **F2 设计元数据**（部分恢复）：version/base_path/host；rules/security 仅在显式 `--with-design` 时携带。
- **F3 正文与评审意图**（不可恢复）：Markdown 标题、说明文字、评审结论。
  反向生成时用 `<!-- TODO(rupost): ... -->` 占位，MUST NOT 编造。
- 没有内联快照的 `.http` 只能生成 **请求级草稿**，MUST NOT 伪造 Schema/Mock。

**所有权与漂移规则**：

1. `.md`（或显式声明的源资产）是设计事实的**唯一写主**。
2. 由 `.md` 派生的 `.http` MUST 带 `# @generated-from <path>` 与内容哈希；
   `spec compile --check` 在 CI 中验证派生文件未漂移。
3. 对派生 `.http` 的修改 MUST 显式 `promote`（回写 `.md` 草稿）或 `--detach`
   （声明该 `.http` 为独立手写文件），不得静默回写。
4. 编译 MUST 确定性：同一 `.md` + 同一配置 → 同字节输出（时间戳/随机值必须可选或固定）。
5. 生成/反向生成 MUST 沿用凭据模板化与脱敏规则，不得把明文密钥写入 `.md`/`.http`。
6. **不做**文件监听、自动双写、三方合并；冲突用 `--check` + 人工确认（见非目标）。

---

## 4. 派生关系与 SSOT 优先级

```text
[既有资产] OpenAPI / curl / history / .http
      │  bootstrap（可多次、显式）
      ▼
[设计期] .md (R1+R2+R3+R4)  ←──── 人工补充正文/规则（可选）
      │  compile（确定性、CI --check）
      ▼
[交付期] .http + 顶部 # @spec-* 内联快照（+ provenance）
      │  promote（显式、带 diff；无快照时只生成请求级草稿）
      └──────────────────────────────→ 回到 .md
      │  分发时排除 .md
      ▼
[执行期] rupost test：
   1) 找本地 .md 物理契约 → 用它校验
   2) 找不到 → 解析 .http 内联快照
   3) 都没有 → 仅普通测试，不做契约校验
```

约束：

- `.md` 是设计事实的写主，`.http` 是派生物；允许手写独立 `.http`，但一旦声明 `@extends-spec`，
  其契约来源必须是 `.md` 或内联快照，不能"部分继承"。
- 同一文件只允许一个 `@extends-spec`；多行 MUST 报错阻断（避免契约冲突）。
- `@depends-on`（执行依赖）与 `@extends-spec`（契约依赖）在解析层 MUST 分开存放，
  前者进 DAG，后者不进 DAG。
- 反向提升（`.http → .md`）优先使用内联快照；没有快照时只能生成"请求级草稿"，
  并在草稿中标注缺失的设计字段（TODO）。

---

## 5. 核心不变量（测试必须守护）

| ID | 不变量 | 守护位置（建议） |
| :--- | :--- | :--- |
| I1 | `.md` 代码块内部语法与 `.http` 解析器一致 | `parser/markdown_file.rs` 单测 |
| I2 | **无 `@test` 的** Mock 契约块在测试模式下永不发包；双角色块（`@test` + 变体）正常执行 | `runner/executor.rs` 集成测试 |
| I3 | 不支持的 `@mock-when` 条件必须编译期失败，MUST NOT 退化为无条件命中 | `mock/compiler.rs` 单测 + 集成测试 |
| I4 | `@mock-default` 必须在变体列表最后匹配 | `mock/compiler.rs` 单测 |
| I5 | `base_path` 只有一种语义：只作用于测试请求的相对 URL；Mock 路由使用契约中写明的完整路径 | `mock/compiler.rs` 回归锁 + 文档 |
| I6 | `@test` 只按独立指令识别，不能子串误判 | parser 单测 |
| I7 | `@extends-spec` 不进 DAG；`@depends-on` 才进 DAG | parser + workflow 单测 |
| I8 | `.http` 内联快照为纯注释，通用客户端执行不受影响 | E2E/快照测试 |
| I9 | `.md` 必须可从既有资产引导生成；手写不得是唯一建档方式 | 生成器集成测试（OpenAPI/curl/history/.http） |
| I10 | 派生 `.http` 必须带 provenance；编译确定性且可 `--check`；禁止静默双向回写 | `spec compile --check` 测试 + 漂移/脱离测试 |
| I11 | 反向生成不得伪造缺失的设计事实；缺失项必须 TODO 标注 | `spec promote` 快照/非快照两路径测试 |

---

## 6. 已裁决的语义细节

### 6.1 `base_path`

- **契约路由路径**：MUST 写成从 `/` 开始的完整路径（例如 `POST /api/v1/orders`）。
- **测试请求 URL**：可以写绝对 URL；写相对路径时由 Runner 拼接 `base_path`。
- **Mock 编译**：直接使用契约中写明的路径，**不**再叠加 `base_path`。
- 原因：现有 `examples/design_scenarios/*` 与 `examples/iteration_scenarios/*` 的契约块均写完整路径；
  若 Mock 再叠加 `base_path` 会双重前缀。此裁决由
  `test_compile_ignores_file_base_path_for_contract_routes` 锁定（Phase 1 T3）。

### 6.2 `@test`

- 只有剥离注释前缀后以 `@test` 开头的行才算测试标记。
- 一个代码块内任一条真正的 `@test` 指令使该块成为测试块。

### 6.3 `@mock-when`

- 语法：`<source>.<path> <op> <value>`，`source ∈ {query, header, body}`。
- Phase 1 支持 `==` 与 `== None`；其他表达式**编译期报错**（fail-closed）。
- 操作符扩展（`>`、`contains`、`exists` 等）属于 Phase 2/3，需单独立项并配测试。

### 6.4 未知指令

- 未实现的 `@` 指令 MUST NOT 静默改变语义。
- 对已知但未实现的保留指令（`@extends-spec`、`@deprecated`、`@proxy`、`@insecure` 等），
  SHOULD 输出可见警告；在实现前，文档不得宣称其可用。

### 6.5 双向转换语义（裁决）

1. **写主唯一**：同一契约单元同一时刻只能有一个写主（默认 `.md`）。`.http` 派生后只读；
   要改执行细节，改 `.md` 后重新编译，或显式 `promote` 回 `.md`。
2. **确定性**：`.md → .http` 的编译输出不得包含当前时间、随机 UUID、绝对临时路径等
   不可复现内容；如需动态值，使用变量占位。
3. **不自动同步**：不实现文件监听/自动回写/三方合并；`--check` 发现漂移时以非零退出码
   提示，由人或 CI 决策。
4. **提升优先用快照**：`.http → .md` 先读内联快照；无快照则生成草稿并标注
   `<!-- TODO(rupost): schema/mock missing -->`，不得把请求体或 `@assert` 反推成契约。
5. **敏感信息**：生成方向必须复用 `exporter::redact` 与模板化规则；反向生成读取已有
   明文时 SHOULD 警告，不得把新明文写入文件。
6. **增量维护（MVP 可后置）**：更新既有 `.md` 时按 `@name` 锚点替换对应代码块，
   保留正文与顺序；MVP 允许只支持"新建文件 + 拒绝覆盖"，但接口设计必须为增量留位。

---

## 7. 变更流程

1. 任何修改上述职能/不变量的 PR，必须同时修改本文件并新增/更新守护测试。
2. 实现进度不写入本文件，统一登记到 `progress_summary.md`。
3. 若发现本契约与产品目标冲突，先改契约、再改代码；禁止代码先行造成既成事实。
