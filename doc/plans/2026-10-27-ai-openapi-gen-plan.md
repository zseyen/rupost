# RuPost P2 开发计划：OpenAPI 导入 + AI 生成 Mock 与断言 (`rupost ai gen`)

> **创建日期**: 2026-10-27
> **定位**: [开发使用旅程设计](file:///Users/zsyzzx/project/rust/rupost/.workspace/rupost-ai/doc/plans/2026-10-27-ai-developer-journey-design.md) P2 立项——**S1 冷启动断点**的解：存量 OpenAPI/Swagger 资产 → rupost Markdown 活文档（含 AI 增强的 `@mock-when` 与 `@assert`）
> **上游哲学**: [bootstrap 提案](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-26-ai-bootstrap-proposal.md) 同源——**确定性基线 + AI 增强 + 引擎裁决 + 降级不塌 + 生成即退场**
> **范围收敛（用户裁决）**: P2 只做 OpenAPI 导入；Postman/curl 导入登记后续

---

## 1. 目标与非目标

### 目标

1. 命令 `rupost ai gen --spec <openapi.json> -o api.md`：将 **OpenAPI 3.0.x / 3.1 JSON** 规格转换为可执行的 Markdown 活文档。
2. 产物三合一（每个操作一个区块）：
   - **请求块**：method/url/路径参数/查询参数/请求体（来自 spec，确定性）
   - **`@mock-when` 契约仿真**：响应状态 + 示例体（spec 的 `example`/`examples`/`default` 优先，AI 补真实感值）
   - **`@assert` 断言**：机械层（status、required 字段 exists）+ AI 语义层（类型约束、description 推导）
3. 降级语义：AI 不可用（无 Key/网络失败/输出非法）→ **纯机械基线** + 警告，exit 0。
4. 防覆盖：`-o` 目标已存在 → 报错不覆盖（对齐 init/bootstrap 的 `create_exclusive` 语义）。
5. 自足自证：生成物可被 `rupost mock api.md` 一键起服，`rupost t api.md --var base_url=http://127.0.0.1:9000` 对着自己的 mock 全绿（**零外网闭环**，与 bootstrap 成功指标同型）。

### 非目标（显式裁剪）

| 裁剪项 | 理由 |
| :-- | :--- |
| YAML 规格支持 | `serde_yaml` 刚被依赖净化清除（unmaintained）；不为此引回。MVP 仅 JSON；YAML 候选 `serde_yaml_ng`/`yaml-rust2`，需走依赖评审，登记后续 |
| Swagger 2.0 | 结构差异大（definitions/ basePath）；显式报错并指引先用转换工具升 3.x |
| Postman / curl 导入 | 用户裁决：P2 只做 OpenAPI |
| 外部文件 `$ref`（`Pet.json`） | 仅支持文档内局部 `$ref`（`#/components/...`）；外部引用报错说明 |
| 生成物的人工智能二次对话（"帮我改这个接口"） | 生成即退场；修改发生在文档里 |
| 全量 OpenAPI 特性 | webhooks/callbacks/discriminator 多态——跳过并在文档中留 `TODO` 注释标记 |

## 2. 命令形态与产物样例

```bash
rupost ai gen --spec petstore.json -o api.md
# [✓] 已生成 api.md：12 个操作，12 个 @mock-when 规则，38 条 @assert（AI 增强 21 条）
# [i] AI: http://127.0.0.1:11434/v1 (qwen2.5:7b)     # 一次性后端提示

rupost ai gen --spec petstore.json -o api.md --no-ai   # 显式纯机械基线
rupost mock api.md --port 9000                          # 生成的文档直接起 mock
rupost t api.md --var base_url=http://127.0.0.1:9000    # 对自己的 mock 回归，全绿
```

产物样例（单操作区块）：

````markdown
### listPets — List all pets

# @name list_pets
GET {{base_url}}/pets?limit=10
Accept: application/json

# @mock-when query.limit == 10
HTTP/1.1 200 OK
Content-Type: application/json

[
  {"id": 1, "name": "Rex", "tag": "dog"}
]

# @assert status == 200
# @assert headers.content-type contains "application/json"
# @assert body.0.name exists
# @assert body.0.id exists
````

要点：URL 主机统一模板化为 `{{base_url}}`（环境变量，用户 `--var` 注入）；`@mock-when` 默认变体条件为无条件的兜底 200（多状态码操作生成多变体：主变体 + 条件变体）；`@name` 采用 `<operationId>` 或方法+路径派生（供 `@depends-on` 链与未来交互选择）。

## 3. 五阶段管线（复用 bootstrap 的 A–E 形态）

| 阶段 | 确定性 | 内容 | 复用/新增 |
| :-- | :-- | :--- | :--- |
| **A 规格解析** | ✅ | OpenAPI JSON → 内部模型（operations/parameters/schemas/responses/examples）；局部 `$ref` 解析（含循环引用检测）；畸形 spec 结构化报错（JSONPath 定位） | 新增 `src/openapi/parser.rs` + `model.rs` |
| **B 机械还原** | ✅ | 模型 → 文档区块：请求块 + status 断言 + required-exists 断言 + mock 骨架（状态码/Content-Type/example 值） | 新增 `src/openapi/emit.rs`（**零 AI 依赖，独立可用**） |
| **C AI 语义增强** | 🤖 | 逐操作发送紧凑 schema 摘要（§4 契约）→ 索取建议：语义断言、mock 示例值增补、区块命名 | `crates/rupost-ai` 新增 `SpecEnhancer`（复用 `AiClient`） |
| **D 引擎裁决 + 落盘** | ✅ | AI 断言过 `assertion::parser` lint（非法丢弃+警告）；mock 变体过 `MockCompiler` 校验（路由不可编译则降级为纯响应体注释）；`create_exclusive` 原子落盘 | 复用 assertion parser / path_guard 模式 |
| **E 回归资产** | ✅ | 产物即普通活文档：`mock` 起服、`t` 回归、进 CI、Git diff 审查 | 存量能力 |

**降级矩阵**：无 Key/探测失败/HTTP 错误/AI 输出非 JSON/建议全非法 → 全部落到 B 阶段机械基线 + 黄色 `[!]` 警告（区分原因），exit 0。`--no-ai` 显式跳过 C 阶段。

## 4. AI 契约（结构化输出 Schema，单操作）

```json
{
  "operation": "POST /pets",
  "name": "create_pet",
  "assertions": [
    "status == 201",
    "body.name exists",
    "body.tag exists"
  ],
  "mock_examples": {
    "body": {"name": "Rex", "tag": "dog", "age": 3}
  }
}
```

Prompt 约束（继承 translator 的语法固化风格）：断言仅限 8 运算符 × 4 目标（`status`/`headers.<name>`/`body.<json_path>`/`response.time`）；mock 值必须匹配 schema 类型；禁止解释性文字，仅输出 JSON。上下文发送**只含 schema 结构与 description**（规格是设计时数据，无运行时敏感值，但仍做 4KB 截断与键名键值脱敏复用 P0 `redactor`——防御 spec 里贴了真实证书/密钥示例的团队）。

## 5. 架构与分层（沿用既定共识）

```
src/openapi/            # 确定性领域模块：parser + model + emit（不知道 rupost-ai 存在）
crates/rupost-ai/       # SpecEnhancer：schema 摘要 → 结构化建议（不知道 rupost 内部类型，吃文本吐文本）
src/ai/                 # 编排层（P0 已立项）：config/context/redactor + gen 流程编排
src/cli.rs + main.rs    # ai gen 子命令接线
```

分层合规：`openapi` 不依赖 `rupost-ai`（emit 可独立交付纯机械基线）；`rupost-ai` 不依赖 rupost 内部类型（未来可独立 MCP 化）；编排层双向胶水。

## 6. 任务拆分与提交计划（5 个 jj 原子提交）

| # | 提交 | 内容 | 验证 |
| :-- | :--- | :--- | :--- |
| 1 | `feat(openapi): OpenAPI 3 JSON parser and model` | `src/openapi/{parser,model}.rs`：解析/`$ref` 局部解析/循环检测/错误定位 | L1 单测（§测试方案 3.1） |
| 2 | `feat(openapi): emit executable markdown scaffold` | `emit.rs`：请求块 + status/required 断言 + `@mock-when` 骨架 + `{{base_url}}` 模板化 | L1 emit 决策表；产物手工 `mock`+`t` 冒烟 |
| 3 | `feat(ai): spec enhancer with engine lint and exclusive write` | crate `SpecEnhancer` + `ai gen` 接线 + lint 裁决 + 防覆盖 + 降级矩阵 | L1 lint 表 + 降级单测 |
| 4 | `test(ai): openapi gen integration with offline dogfooding` | `tests/ai_gen_test.rs` + `tests/fixtures/petstore.json` + `examples/ai_assert/mock_ai_gen.json` | L2 全绿（含自足闭环） |
| 5 | `docs(ai): ai gen walkthrough and verify step` | `examples/ai_gen/README.md` + verify_features.sh 步骤 8 + README/CHANGELOG/progress_summary | L3 脚本 PASS |

**预估**：4–6 天（parser 1.5d、emit 1.5d、enhancer+接线 1d、测试 1d、docs 0.5d）。

## 7. 风险与对策

| 风险 | 对策 |
| :-- | :--- |
| OpenAPI 特性长尾（oneOf/多态/遗留字段）导致解析崩溃 | 未识别关键字段**跳过 + 文档留 TODO 注释**，绝不 panic；畸形 spec 结构化报错 |
| AI 建议的 mock 值与 schema 类型不符 | lint 层校验类型匹配，不符丢弃回退 spec example |
| 生成巨型文档（数百操作）不可审 | `--filter <path-prefix>` 按前缀过滤（MVP 简单实现）；操作数 >50 时提示分批 |
| serde_yaml 缺位导致用户拿 YAML 来跑 | 报错信息明确指引"转 JSON 或使用在线转换"，登记 YAML 支持 backlog |
| 生成的断言在真实服务上误报（spec 与实现不符） | 这是**特性不是 bug**——契约断言红了正说明实现偏离 spec；文档 README 说明此契约语义 |
