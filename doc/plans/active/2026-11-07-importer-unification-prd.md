# RuPost 多格式导入器统一 PRD（CLI × Web 双端同源）

> **文档标识**：2026-11-07-importer-unification-prd
> **状态**：Proposed（待评审 → Accepted 后进入执行队列）
> **优先级**：排在 Web 安全批次 / 基础功能完善 / CLI 加固之后（用户裁决：后移，且必须与 CLI 功能结合）
> **关联**：[Web 工作台加固计划](2026-11-07-web-workbench-missing-features-and-hardening-plan.md) 战役 A-2、[import curl 交付记录](../archive/)（F6）、[CLI 安全评估](../../dev/2026-11-07-cli-security-assessment.md) P2-1

---

## 一、背景与问题

现状存在**两套并存、职责重叠的导入实现**：

| 位置 | 已有能力 | 缺口 |
| :--- | :--- | :--- |
| `src/importer/curl.rs`（CLI） | `rupost import curl`：POSIX 子集分词 + curl flag 决策表 + **HttpGenerator 发射管线**（凭据模板化 `{{变量}}`、`@name` 语义命名、status 断言、`--templatize-host`） | 仅 curl 单格式 |
| `src/web/importer.rs`（Web） | curl/HTTPie/RawHTTP 解析 + **HAR/OpenAPI/Postman 格式嗅探**（`sniff_format` 识别三类 JSON） | **三格式识别了但不会解析**（checkpoint `:376` "已支持 HAR/OpenAPI/Postman" 为失实宣称，需勘误） |

问题：① HAR/Postman/OpenAPI 用户资产无法迁移；② Web 侧解析与 CLI 侧解析、`src/cli/parser.rs` 三处重复，未来新格式将三倍维护；③ 装入器领域逻辑寄居在 `src/web/` 适配层，违反领域层归属。

## 二、目标与非目标

**一句话定义**：把"外部请求格式 → RuPost 活文档"的装入能力收敛为 `src/importer/` 领域层的单一实现，CLI 与 Web 是它的两个适配端。

**非目标**（防摇摆，对齐既有裁决）：
- 代理抓包（`rupost record`）——Won't do（2026-10-27 裁决）；
- gRPC proto / GraphQL schema 导入——Won't do（同竞争裁决）；
- curl `$'..'` ANSI-C 引导语法——维持显式拒绝；
- Postman 脚本（`pm.test` 复杂链）完整转译——只转换简单 status 断言，其余留人工。

## 三、用户故事（Stage 0 合规）

- **前端开发者**：Given 浏览器导出的 HAR 抓包，When 我 `rupost import capture.har -o api.http`，Then 得到带 `@name` 序号命名、Host 模板化为 `{{base_url}}`、凭据已模板化的可执行用例。
- **QA**：Given 团队存量 Postman Collection v2.1，When 导入，Then 请求树按目录顺序平铺为块，`pm.test` 中的 status 断言转为 `# @assert status == 200`，disabled header 被剔除。
- **后端**：Given OpenAPI 3.0 文档，When 导入，Then 每个 path×method 生成契约块，`responses.200` 生成 `@mock-default` 兜底变体（mock 服务器立即可以仿真该 API）。
- **工作台用户**：Given Web 装入向导，When 粘贴/拖入 HAR，Then 与 CLI 完全同源的解析与发射结果（双端同一测试守护）。

## 四、需求清单

| # | 需求 | 优先级 | 验收标准 |
| :-- | :--- | :--: | :--- |
| R1 | **HAR 1.2 解析**（`src/importer/har.rs`）：`log.entries[].request`，Method/URL/Query/Headers/PostData；过滤浏览器伪头（`:` 开头）与 content-length；`@name` 序号命名（`har_request_N`，URL 尾段优先）；`templatize_host` 复用 `UrlTemplatizer` | P0 | 决策表单测（伪头过滤/MIME/序号命名）+ 往返：生成文件 `rupost t` 可解析 |
| R2 | **Postman v2.1 解析**（`src/importer/postman.rs`）：递归 `item[]` 树；`request.url.raw`（对象/字符串双形态）；`disabled` header 剔除；`body.raw` 直传；`pm.test` 中 `to.have.status(N)` → `# @assert status == N`；条目 `name` 为块名 | P1 | 决策表单测（递归嵌套/disabled/断言转换）+ 往返测试 |
| R3 | **OpenAPI 3.0 / Swagger 解析**（`src/importer/openapi.rs`）：遍历 `paths`；server url 作 `{{base_url}}` 候选；请求体示例（`example`/`examples`/schema 合成）直传；`responses.200` → `@mock-default` 变体（复用 web importer 既有 mock 生成参数） | P1 | 决策表单测 + mock 仿真探针（`/api/v1/mock/simulate`）命中兜底 |
| R4 | **CLI 扩展**：`rupost import <file>` 支持自动嗅探 + `--format har\|postman\|openapi\|curl` 显式覆盖；`sniff_format` 从 `src/web/importer.rs` **迁入 `src/importer/`**；保留 `-o` 防覆盖与 AGENTS §5 trailing_var_arg 陷阱拦截 | P0 | CLI 集成测试：三格式端到端 + 嗅探歧义用例（Postman vs 任意 JSON）|
| R5 | **Web 收敛**：`/api/v1/import` 与 `/import/curl` 委托 `src/importer/` 同一实现；`src/web/importer.rs` 收敛为薄适配（HTTP 请求/响应模型转换）；curl/HTTPie/raw-http 三份解析拉平到领域层 | P1 | 双端同源测试：同一输入 CLI 与 Web 产出逐字节一致 |
| R6 | **安全不变量**（继承 F6 + 评估报告 P2-1）：发射统一走 `HttpGenerator`（凭据模板化、status 断言）；body 行首为 `###`/`# @` 的内容行发射告警（不改写）；所有解析器对超深嵌套/超大 JSON 有界（复用 serde 宽容回退） | P0 | 对抗测试：注入样例（复用 2026-11-07 评估 PoC 样例）不产生可执行走私 |

## 五、技术要点

- **发射管线单一化**：三个解析器只产出 `ParsedRequest`/`HistoryEntry` 中间表示，发射全部经 `HttpGenerator::generate_with_variables`——与 import curl 完全同源（凭据模板化、命名、断言策略免费继承）。
- **域内复用**：`UrlTemplatizer`（`src/http/url.rs`）、`parse_http_header`（`src/parser/mod.rs`）、`sniff_format`（迁入）。
- **放置纪律**：`src/importer/` 不依赖 `src/web/`；`src/web/handlers/import.rs` 单向依赖 `src/importer/`（Adapter → UseCase/Domain）。
- **勘误联动**：交付时修正 `checkpoint.md:376` 与 `doc/architecture/directory_structure.md` 的"已支持"失实表述，登记真实状态。

## 六、提交切分（Accepted 后执行）

1. `refactor(importer): unify curl/httpie/raw-http parsing into src/importer domain`（R5 前半 + R4 嗅探迁移，纯移动零行为变化）
2. `feat(importer): HAR 1.2 importer with CLI --format wiring`（R1 + R4）
3. `feat(importer): Postman Collection v2.1 importer`（R2）
4. `feat(importer): OpenAPI importer with mock-default synthesis`（R3）
5. `feat(web): delegate /api/v1/import to unified importer domain`（R5 收尾 + R6 告警）
6. `docs: correct multi-format claims and register importer unification`（勘误 + 目录结构文档）

## 七、验收门禁

`cargo test --workspace` 全绿 + clippy/fmt 双零 + `verify_features.sh` + `run_all.sh`；新增对抗测试（注入样例、双端同源、往返）全部与修复同提交。
