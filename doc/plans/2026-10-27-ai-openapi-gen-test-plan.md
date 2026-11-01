# RuPost P2 测试方案：OpenAPI 导入 + AI 生成 Mock 与断言

> **创建日期**: 2026-10-27
> **性质**: 领域专项测试规约——父纲 [全量测试方案](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-25-test-strategy-plan.md)，遵循 [开发计划](file:///Users/zsyzzx/project/rust/rupost/.workspace/rupost-ai/doc/plans/2026-10-27-ai-openapi-gen-plan.md) 的 5 提交序列
> **被测对象**: `src/openapi/`（parser/model/emit，确定性）+ `crates/rupost-ai::SpecEnhancer`（AI 增强）+ `ai gen` 命令编排
> **当前基线**: 527 测试 0 失败；本迭代预计新增 ~45 个用例

---

## 1. 测试分层与隔离手段

| 层 | 载体 | 本迭代手段 |
| :-- | :--- | :--- |
| **L0** | 提交前三步 | fmt / clippy -D warnings / cargo test --workspace |
| **L1** | `src/openapi/` 各 `mod tests` + crate `mod tests` | 决策表；fixture 内嵌字符串；**不读文件系统** |
| **L2** | `tests/ai_gen_test.rs`（新增） | 子进程 + `rupost mock`（Phase 0 既有范式）+ TempDir 沙箱 + **自足闭环**（生成物 mock 起服打自己） |
| **L3** | `verify_features.sh` 步骤 8 | petstore fixture → gen → mock → t 全链，零外网 |
| **L5** | 真机 | 用真实 Swagger 工具导出的规格（petstore 官方 JSON）人工冒烟 |

**核心测试设计——自足闭环**：`ai gen` 产物是含 `@mock-when` 的活文档，因此可验证最强的契约：**生成物用自己的 mock 起服，对自己的断言全绿**。生成物若缺请求块/mock 规则/断言任一环，闭环即断。这条测试同时锁死"文档三合一"的正确性，且零外网。

## 2. 测试 Fixture 设计

| Fixture | 用途 |
| :-- | :--- |
| `tests/fixtures/petstore_min.json` | 最小 OpenAPI 3.0 规格手写件：3 操作覆盖矩阵——`GET /pets`（200 数组 + query 参数）、`POST /pets`（201 + required 请求体）、`GET /pets/{id}`（200 / 404 双响应 + 路径参数）；含 `components/schemas` 局部 `$ref`、`example`、description |
| 同目录变形件（测试内 `json!` 拼装或字符串替换） | 畸形 JSON / 缺 info / 循环 `$ref` / 外部 `$ref` / Swagger 2.0 / 空 paths / YAML 扩展名 |
| `examples/ai_assert/mock_ai_gen.json` | mock AI 后端：`/v1/chat/completions` 固定返回合法增强建议 JSON（与 §4 契约对齐）；再加一个 `/v1/chat/completions-404` 规则或用变体返回垃圾/500 供降级路径 |

## 3. L1 单元测试内容

### 3.1 `parser` 决策表（12 用例）

| ID | 输入 | 期望 |
| :-- | :--- | :--- |
| TC-P01 | 合法最小 spec | operations 数量/方法/路径正确 |
| TC-P02 | 局部 `$ref`（`#/components/schemas/Pet`） | 正确解引用（含嵌套数组 `array of $ref`） |
| TC-P03 | 循环 `$ref`（A→B→A） | 检测并报错（带引用链），不死循环 |
| TC-P04 | 外部 `$ref`（`Pet.json`） | 结构化报错"不支持外部引用" |
| TC-P05 | 多方法同路径（get+post） | 各自生成 operation |
| TC-P06 | 无 `operationId` | 名称派生（method_path 小写化） |
| TC-P07 | `parameters` 层级合并（path 级 + operation 级） | 合并去重 |
| TC-P08 | 请求体 `application/json` schema + example | body 模型含 schema 与示例值 |
| TC-P09 | 多响应码（200/404）+ 各自 example | 多响应模型 |
| TC-P10 | 畸形 JSON | 错误含行列定位，非 panic |
| TC-P11 | 缺 `info`/`paths` 必填字段 | 结构化报错指明字段 |
| TC-P12 | Swagger 2.0（含 `"swagger": "2.0"`） | 明确报错并指引升级 3.x |

### 3.2 `emit` 决策表（10 用例）

| ID | 输入模型 | 期望产物特征 |
| :-- | :--- | :--- |
| TC-E01 | 单 GET 无参操作 | 请求块 + 兜底 `@mock-when`（无条件 200）+ `status == 200` 断言 |
| TC-E02 | query 参数（含 example/default） | URL 带查询串；`@mock-when query.<k> == <v>` |
| TC-E03 | 路径参数 `/pets/{id}` | 路由 `:id` 形态 + mock 体 `{{id}}` 渲染 |
| TC-E04 | POST + required 请求体 | 请求体 + `@mock-when` 响应体 + required 字段 exists 断言 |
| TC-E05 | 多响应码 | 主变体（首个 2xx）+ 次变体（条件或注释形态） |
| TC-E06 | schema 无 example | 从 `default`/类型桩值生成（string→语义占位，integer→1） |
| TC-E07 | 服务器 URL `https://api.x.com/v1` | 主机收敛为 `{{base_url}}`，保留 path 前缀 `/v1` |
| TC-E08 | `@name` 派生 | operationId 优先；无则 method_path；可被 `@depends-on` 引用 |
| TC-E09 | 未识别 OpenAPI 特性（webhooks） | 跳过 + 产物内 `# TODO(openapi):` 注释，不 panic |
| TC-E10 | 断言语法合法性 | 所有机械断言可被 `assertion::parser` 解析（性质测试，遍历生成断言逐条喂 parser） |

### 3.3 `SpecEnhancer` 与编排层（8 用例）

| ID | 场景 | 期望 |
| :-- | :--- | :--- |
| TC-S01 | AI 返回合法建议 JSON | 断言/mock 值被合并；`[✓]` 统计行正确（增强条数） |
| TC-S02 | AI 返回非法断言（`body.x ~= 1`） | lint 丢弃该条 + 警告列出原因，合法条保留 |
| TC-S03 | AI 返回非 JSON/包 ```json 围栏 | 围栏剥离成功；纯散文 → TranslationFailed → 降级基线 |
| TC-S04 | AI mock 值类型与 schema 不符（string 位给 int） | lint 丢弃回退 spec example |
| TC-S05 | 无 Key / HTTP 401 / 连接拒绝 | 各自降级机械基线 + 黄色 `[!]`（原因可区分）+ **exit 0** |
| TC-S06 | `--no-ai` | 跳过 C 阶段，零网络请求（不发 chat completions） |
| TC-S07 | `-o` 目标已存在 | 报"文件已存在"不覆盖，exit 1 |
| TC-S08 | 发送上下文截断 | schema 摘要超 4KB 截断 + `[truncated]`（复用 P0 redactor） |

## 4. L2 集成测试内容（`tests/ai_gen_test.rs`，6 用例）

公共编排：TempDir 沙箱 + 子进程 `rupost mock`（AI 后端）+ `env` 注入（复用 Phase 0 `run_ai_assert` 范式，抽出 `run_ai_gen`）。

| ID | 场景 | 断言 |
| :-- | :--- | :--- |
| TC-I01 | **自足闭环**（dogfooding）：petstore_min + mock AI → `ai gen -o api.md` → `rupost mock api.md --port P` → `rupost t api.md --var base_url=http://127.0.0.1:P` | gen exit 0；**t 全绿**（生成物的 mock 满足生成物的断言）；产物含 `@mock-when` 与 `# @assert` |
| TC-I02 | 降级：`--no-ai`（或 AI mock 返回 500） | 产物仍含请求块 + status 断言；警告含"机械基线"；**t 自足闭环仍全绿**（基线也必须自足） |
| TC-I03 | AI 输出垃圾建议 | 合法断言保留、非法丢弃有警告；闭环仍全绿 |
| TC-I04 | 防覆盖：二次运行同命令 | exit 1 + "已存在"，原文件内容不变（mtime/hash 比对） |
| TC-I05 | 畸形 spec（外部 $ref / Swagger 2.0） | exit 1 + 指引文案（各自区分） |
| TC-I06 | 空结果防护：spec 全部操作被过滤（`--filter` 无命中） | exit 1 + 明确提示，不产出空文档 |

## 5. L3 脚本与 L5 真机

*   **L3（verify_features.sh 步骤 8）**：`gen`（petstore fixture，零 Key dogfooding 指向 mock AI）→ `mock` 起服 → `t` 自测 → 全绿校验输出含 `[PASS]`；失败分支沿用 `|| AI_EXIT=$?` 捕获模式。
*   **L5（真机，发版前）**：官方 petstore OpenAPI JSON（真实 Swagger 工具导出件）走一次完整闭环；真机 Ollama 零配置跑 `--spec`（复用 P0 零配置链）。

## 6. 用例 → 计划验收追溯

| 开发计划验收 | 覆盖用例 |
| :--- | :--- |
| 目标 2 产物三合一 | TC-E01–E09、TC-I01 |
| 目标 3 降级语义 | TC-S03/S05/S06、TC-I02/I03 |
| 目标 4 防覆盖 | TC-S07、TC-I04 |
| 目标 5 自足自证 | TC-I01/I02（L2）、L3 步骤 8 |
| 非目标边界 | TC-P04（外部 ref）、TC-P12（Swagger 2.0） |
| 风险 1 特性长尾 | TC-P10/P11、TC-E09 |
| 风险 4 YAML 缺位 | 集成补充：`.yaml` 扩展名 → 明确报错指引 |

## 7. 实施顺序（TDD，与 5 提交对齐）

1. 提交 1：先红（TC-P01–P12 骨架）→ 绿（parser）。`$ref` 循环检测与错误定位是重点。
2. 提交 2：TC-E 决策表先行；**TC-E10（断言过 parser）作为 emit 的性质测试**，保证发射器永不产出引擎不认识的断言。
3. 提交 3：TC-S 表先行（lint 表密度最高——它是 AI 输出的信任边界）；降级矩阵逐条红绿。
4. 提交 4：TC-I 自足闭环（先手写期望产物对照，再上 dogfooding）。
5. 提交 5：L3 脚本 + 文档。每提交前三步照旧，提交 2 起全量回归（基线 527 → 预计 ≥ 572）。
