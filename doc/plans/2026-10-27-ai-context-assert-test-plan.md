# RuPost AI 迭代测试方案：上下文感知断言与零配置接入

> **创建日期**: 2026-10-27
> **性质**: 领域专项测试规约——以 [全量测试方案](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-25-test-strategy-plan.md) 为父纲，遵循其 L0–L5 分层模型与测试设计三原则（离线隔离 / 数据驱动 / 极值验证）
> **被测对象**: [AI 迭代 PRD](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-ai-context-assert-and-zero-config-prd.md)（`--last`/`--response` 上下文断言 + 零配置接入链）
> **当前基线**: 全量 525 测试 0 失败；本迭代预计新增 ~38 个测试用例

---

## 1. 测试对象与范围

| 单元 | 位置 | 测试层 |
| :-- | :--- | :--- |
| 脱敏截断器 `redactor` | `src/ai/redactor.rs`（新增） | L1 |
| 配置解析与本地探测 `config` | `src/ai/config.rs`（新增） | L1 + L2 |
| 素材采集与 prompt 拼装 `context` | `src/ai/context.rs`（新增） | L1 + L2 |
| CLI 参数语义（`--last`/`--response`/`--no-mask`） | `src/cli.rs` | L1（parse 断言）+ L2 |
| translator system prompt 微调 | `crates/rupost-ai/src/translator.rs` | L1（mockall，1 用例） |
| 端到端命令行为 | `rupost ai assert` 二进制 | L2 + L3 |
| 真机 Ollama 零配置 | 用户机器 11434 | L5（发版前） |

**不在范围**：crate `client` 4 测试与 translator 既有 3 测试（Phase 0 已覆盖且不动）。

## 2. 分层与隔离手段

| 层 | 载体 | 本迭代隔离手段 |
| :-- | :--- | :--- |
| **L0** | 提交前三步 | `cargo fmt --check` + `clippy --all-targets --all-features -D warnings` + `cargo test --workspace` |
| **L1** | 各模块 `mod tests` | 纯函数决策表；探测经**函数参数注入**候选地址，零网络依赖（wiremock 仅在需要 HTTP 行为时使用） |
| **L2** | `tests/ai_assert_test.rs` 扩充 | 子进程二进制 + `rupost mock`（Phase 0 既有范式）；`TempDir` 重定向 `HOME`（隔离用户级配置）；`RUPOST_HISTORY_DIR` 注入（隔离历史，复用 `history_cli_test.rs` 既有机制）；mock **Body+Contains 条件变体反向锁定脱敏契约** |
| **L3** | `tests/verify_features.sh` 步骤 7 扩充 | 真实请求产生真实 history → `ai assert --last 1` 全链闭环，零外网 |
| **L5** | 真机冒烟 | 有 Ollama 的开发机：零配置直接可用（US3 实景）；断网无配置 → 回退指引（US4 实景） |

**关键测试设计**：脱敏发生在"发送之前"这条红线，用 mock 条件变体做**反向锁定**——`mock_ai_context.json` 的成功变体条件为 `Body Contains "{{MASKED}}"`，兜底变体返回 500。上下文未脱敏则永远打不到成功变体，测试必然失败。此模式可直接推广到 bootstrap Phase 1 的脱敏测试。

## 3. L1 单元测试内容（数据驱动决策表）

### 3.1 `redactor` 决策表（10 用例）

| ID | 输入 | 期望输出 |
| :-- | :--- | :--- |
| TC-R01 | 头 `Content-Type`/`Accept`/`User-Agent` | 白名单内，原样保留 |
| TC-R02 | 头 `Authorization`/`Cookie`/`Set-Cookie`/`x-api-key`（含大小写变体） | 值 → `{{MASKED}}`，键名保留 |
| TC-R03 | JSON 键 `password`/`token`/`secret`/`key`/`authorization` 及 `Password`/`PASSWORD` 变体 | 值 → `{{MASKED}}` |
| TC-R04 | 嵌套 `{"data":{"password":"x"}}` | 内层值脱敏，**结构保留** |
| TC-R05 | 数组内对象 `{"users":[{"token":"t"}]}` | 数组元素内脱敏 |
| TC-R06 | 非 JSON 文本体 | 原样（仅参与截断，不做键匹配） |
| TC-R07 | 体积边界：4095 字符 / 4096 字符 | 前β不截断；后者截断并标注 `[truncated]` |
| TC-R08 | 空对象 `{}` / 空头集 | 原样，无异常 |
| TC-R09 | 对已脱敏文本二次执行 | 幂等，不二次替换 |
| TC-R10 | `--no-mask` 模式 | 不脱敏，仅截断（调用方负责打印风险提示） |

### 3.2 `config` 决策表（11 用例）

| ID | 场景 | 期望 |
| :-- | :--- | :--- |
| TC-C01 | env 与 `~/.rupost/ai.toml` 并存 | env 胜出 |
| TC-C02 | file 与本地探测并存 | file 胜出 |
| TC-C03 | 三级全缺 | 进入探测；探测失败 → 回退指引错误 |
| TC-C04 | 合法 `ai.toml`（base_url + model） | 正确解析 |
| TC-C05 | `ai.toml` 含 `api_key` 字段 | **拒绝该字段并警告**（Key 只走 env 的安全边界）；其余字段可用 |
| TC-C06 | `ai.toml` 畸形 TOML | 警告 + 降级到下一优先级（不 panic） |
| TC-C07 | base_url 为本地地址（`localhost`/`127.0.0.1`/`::1`）且无 Key | 允许，Key 用占位符（Ollama 不校验）——**对 PRD §5 的细化**，见 §9 |
| TC-C08 | base_url 为远程地址且无 Key | 错误含可执行指引（export Key） |
| TC-C09 | 探测成功（注入 wiremock URL 返回 `{"data":[{"id":"m1"}]}`） | 取 `data[0].id` 为模型 |
| TC-C10 | 探测失败（注入不可达地址） | 返回 None → 上层回退指引 |
| TC-C11 | 探测端点非 200 / 非 JSON | 视为不可用 |

### 3.3 `context` 采集与拼装（6 用例）

| ID | 场景 | 期望 |
| :-- | :--- | :--- |
| TC-X01 | `--last 1`，history 有 2 条 | 取最近 1 条，含全量响应体 |
| TC-X02 | `--last 3`，history 有 5 条 | 取 3 条并按时间序拼装 |
| TC-X03 | 空 history + `--last` | 明确错误（"暂无历史记录，先执行一次请求"），exit 1 |
| TC-X04 | `--response` 文件不存在 | IO 错误含路径，exit 1 |
| TC-X05 | `--response` 超大文件 | 走 4KB 截断路径 |
| TC-X06 | user prompt 拼装格式 | 含"期望：\<自然语言\>"与"实际响应（已脱敏）："两段；脱敏后上下文嵌入（`{{MASKED}}` 在场、原始密钥不在场） |

### 3.4 CLI 解析与 crate（2 用例）

| ID | 场景 | 期望 |
| :-- | :--- | :--- |
| TC-A01 | `--last 1 --response f` | clap 参数冲突错误（`conflicts_with`），非零退出 |
| TC-T01 | translator 收到含上下文的 user prompt（mockall `withf`） | system prompt 含"以实际响应为准"语义、user prompt 原样透传 |

## 4. L2 集成测试内容（`tests/ai_assert_test.rs` 扩充，7 新增 + 3 存量不动）

**公共编排**（复用 Phase 0 的 `spawn_mock`/`wait_for_port`/`run_ai_assert` 范式，扩展支持 HOME/HISTORY_DIR 注入）：

| ID | 对应 PRD | 场景与断言 |
| :-- | :--- | :--- |
| TC-I01 | US1 + US6 | `TempDir` HOME + `RUPOST_HISTORY_DIR` 注入种子历史（响应含 `Authorization: Bearer eyJ...` 头与 `{"data":{"token":"x","role":"admin"},"password":"hunter2"}` 体）→ `mock_ai_context.json`（成功变体条件 `Body Contains {{MASKED}}`，兜底 500）→ `ai assert --last 1 "..."` → **exit 0 即证明脱敏先于发送**；输出含 `# @assert body.data.role == "admin"` |
| TC-I02 | US2 | `--response <临时文件>`（内容含敏感键）→ 同上断言路径 |
| TC-I03 | US2 | `--last 1 --response f` 同给 → 非零退出 + 参数冲突提示 |
| TC-I04 | US3(部分) | HOME 重定向 + `~/.rupost/ai.toml`（base_url 指向本地 mock）+ 零 env → 成功 |
| TC-I05 | US3 | HOME 重定向 + 零 env + `RUPOST_AI_PROBE_URL` 指向 mock → 成功 + 输出含一次性后端提示文案 |
| TC-I06 | US4 | `RUPOST_AI_PROBE_URL=127.0.0.1:1`（死口）→ exit 1 + 指引含三选一文案（env / ai.toml / Ollama） |
| TC-I07 | US1 边界 | `--last 1` 但 history 为空 → exit 1 + "暂无历史"提示 |

**新 fixture**：`examples/ai_assert/mock_ai_context.json`——`POST /v1/chat/completions` 双变体：① 条件 `Body Contains "{{MASKED}}"` → 200，content 为 `["status == 200", "body.data.role == \"admin\"", "body.data.token exists"]`；② 兜底 → 500（"redaction missing"）。

## 5. L3 脚本扩充（`verify_features.sh` 步骤 7 追加）

1. 扩展 `examples/ai_assert/mock_ai.json`：增加 `POST /auth/login` 路由 → `{"data":{"token":"tk_123","role":"admin"}}`（fixture 同时服务 L3 与示例文档）。
2. 新增断言序列：
   - `$RUPOST_BIN test` 跑一个临时 `login.http`（对 mock 9001 发真实请求，**产生真实 history**）；
   - `ai assert --last 1 "验证登录返回管理员"`（零 env、本机无配置场景由脚本沙箱保证：`env -u` 清场）→ 输出含 `body.data.role` 级断言（mock 变体命中即证明脱敏链路真实生效）；
   - 命中失败（500 兜底）时脚本 RED 退出——`set -e` 下沿用 `|| AI_EXIT=$?` 捕获模式。

## 6. L5 真机冒烟（发版前，人工）

*   有 Ollama 的机器：清空全部 `RUPOST_AI_*`/`OPENAI_API_KEY` → `rupost ai assert "status is 200"` 直接成功（US3 实景，L2 无法覆盖真实 11434）。
*   断网 + 无任何配置 → 三选一可执行指引（US4 实景）。

## 7. 用例 → PRD 验收追溯矩阵

| PRD 验收 | 覆盖用例 |
| :--- | :--- |
| US1 上下文断言 | TC-I01、TC-X01/X02/X06、TC-R03/R04/R05 |
| US2 响应文件 | TC-I02/I03、TC-X04/X05 |
| US3 零配置探测 | TC-C09、TC-I04/I05、L5 真机 |
| US4 回退指引 | TC-C10/C11、TC-I06 |
| US5 优先级与安全边界 | TC-C01–C08 |
| US6 数据最小化（红线） | TC-R01–R10、TC-I01（mock 反向锁定）、TC-X06 |
| 成功指标 1–4 | 指标 1←L5+I05；2←I01（`body.data.*` 路径断言）；3←R 矩阵+I01 grep；4←`redactor`/context 模块被 Phase 1 复用（架构评审点） |

## 8. 实施顺序（TDD，与 PRD §7 提交对齐）

1. **红**：先落 §3.1 redactor 决策表测试（模块骨架 + 全部断言）→ **绿**：实现 `redactor.rs`。同法推进 `config`、`context`。
2. 提交 1（feat）落地时 L1 应全绿；提交 2（test）落地 §4 L2 + fixture；提交 3（docs）落地 §5 L3 与 examples 增补。
3. 每提交前三步照旧；提交 2 起跑 `cargo test --workspace` 全量回归（基线 525 + 新增 ≥ 563）。

## 9. 测试设计驱动的 PRD 细化发现（建议回写）

| 发现 | 建议 |
| :--- | :--- |
| 无 Key 的本地端点（Ollama 不校验鉴权）若强制要求 env Key，会重新引入配置摩擦（与迭代目标矛盾） | 采纳 TC-C07 规则：**本地地址允许占位 Key；远程地址必须显式 Key**。建议回写 PRD §5 |
| US3 真实 11434 在 CI 不可依赖、US4 死口场景在有 Ollama 的开发机不可复现 | 探测候选经函数参数注入（L1）+ `RUPOST_AI_PROBE_URL` 测试钩子（L2，仅测试使用、不入用户文档）；US3 全实景由 L5 兜底 |
| 脱敏红线的行为验证若只断言输出文本，无法证明"发送前"时序 | 确立 **mock Body-Contains 反向锁定**模式（§2），bootstrap Phase 1 脱敏测试复用 |
