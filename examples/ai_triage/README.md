# AI 失败智能处理示例（E2 就地归因 + E5 失败聚类）

两个**诊断期** AI 功能：测试的成功/失败 100% 由确定性断言决定，AI 只在执行结束后的
报告阶段只读介入。AI 缺失/断网/模型故障时自动降级为确定性基线（B1），绝不阻塞工作流。

| 命令 | 作用 | AI 缺失时的行为 |
| :-- | :-- | :-- |
| `rupost test <files> --explain` | 每个失败请求下方输出规则版 `[Possible Causes]`，配置 AI 后追加 `[AI Explanation]` | 规则版照常 + 黄色提示，退出码仍为 1（由测试决定） |
| `rupost ai triage <report.json>` | 读取 JSON 报告，聚类失败根因簇 | 确定性签名分组 + 黄色警告，exit 0 |
| `rupost test <files> --triage` | 同上，直接对内存结果聚类（无需先落盘报告） | 同上 |

---

## 1. 快速开始（零 Key 离线闭环）

本目录自带完整的离线演示环境：`mock_ai.json` 同时扮演**被测 401 服务**和
**OpenAI 兼容 AI 后端**，`failing_suite.http` 是一个必然失败的请求。

```bash
# 终端 1：起 mock（端口任意，示例用 9002）
rupost mock examples/ai_triage/mock_ai.json --port 9002

# 终端 2：E2 —— 就地归因（未配置任何 AI，观察规则版基线与黄色提示）
rupost test examples/ai_triage/failing_suite.http \
  --var base_url=http://127.0.0.1:9002 --explain
```

实际输出（节选）：

```text
Running 1 requests from examples/ai_triage/failing_suite.http...

 ✗ [1] GET http://127.0.0.1:9002/v1/users/1 (5ms)
    [Possible Causes]
      - 收到 HTTP 401 鉴权失败，请检查 Authorization Header、Token 是否过期或 --cookie-file 会话状态。
    ⚠ AI 归因已跳过（未检测到可用 AI 配置）

Batch Test Summary ...
```

**退出码为 1**（由失败断言决定，AI 不改变判定）。等价开关：`--ai-explain` 别名，
或环境变量 `RUPOST_AI_EXPLAIN=1`。

## 2. E2：配置 AI 后的就地归因

任意 OpenAI 兼容端点（云 API / 本地 Ollama / 内网 vLLM）均可：

```bash
export RUPOST_AI_API_KEY=sk-...
export RUPOST_AI_BASE_URL=http://localhost:11434/v1   # 本地 Ollama 示例

rupost test examples/ai_triage/failing_suite.http \
  --var base_url=http://127.0.0.1:9002 --explain
```

AI 可用时，规则版之后追加（模型按固定 JSON Schema 返回，宽容解析）：

```text
    [Possible Causes]
      - 收到 HTTP 401 鉴权失败，请检查 Authorization Header、Token 是否过期或 --cookie-file 会话状态。
    [AI Explanation]
      Root Cause:   JWT 已过期且未刷新，受保护端点拒绝请求
      Suggested Fix: 先运行登录接口刷新 token，或更新 RUPOST_AI_* 凭据环境变量
```

> 注意：本目录 `mock_ai.json` 返回的是**聚类**形状的 JSON（为 E5 准备），
> 因此 `--explain` 打到它时会收到解析失败警告并优雅回退规则版——这本身就是
> B1 降级路径的活演示。

### 风暴熔断与签名去重

- 单次运行最多对 **3 个不同错误签名**调用 AI（`MAX_AI_EXPLAIN_CALLS = 3`）；
  相同签名（同 URL 模式 + 同状态码 + 同错误类型）的后续失败自动跳过，不重复消耗配额；
- 连续 **2 次** AI 调用失败即熔断，本轮剩余失败全部走规则版；
- 配额耗尽时提示一次：`ℹ Reached per-run AI explanation limit (3); ... (tip: use --triage ...)`。

## 3. E5：失败聚类分诊

### 3.1 生成报告

```bash
rupost test tests/ --report-file target/report.json
# 终端输出不受影响；JSON 报告落盘（失败项附带脱敏后的响应片段与 expected/actual）
```

### 3.2 离线聚类（无 Key，确定性基线）

```bash
rupost ai triage target/report.json
```

实际输出（单请求失败的真实样例）：

```text
⚠ AI 不可用（未检测到配置），已输出确定性签名分组结果      ← stderr，黄色
## Failure Triage Summary (Deterministic Baseline) (1 failures across 1 requests -> 1 root-cause clusters)

### 1. [Auth] HTTP 401 @ /v1/users/:id (1 failures)
- **Affected**: /path/to/rupost/examples/ai_triage/failing_suite.http#1 (get-profile)
- **Root Cause**: 接口返回 HTTP 401 鉴权或权限拒绝
- **Suggested Fix**: 检查登录前置步骤、Bearer Token 环境变量或 Cookie 会话文件
```

大批量失败时同名签名的请求会聚合进同一簇（`count` 与 `Affected` 列表随之增长），
按影响面降序排列。

机器可读（供 CI/告警管道消费）：

```bash
rupost ai triage target/report.json --json | jq '.clusters[] | {category, count, title}'
```

```json
{
  "total_requests": 120,
  "failed_requests": 42,
  "cluster_count": 3,
  "ai_enhanced": false,
  "clusters": [
    {
      "category": "Auth",
      "title": "HTTP 401 @ /v1/*",
      "count": 31,
      "affected_requests": ["/path/to/rupost/examples/ai_triage/failing_suite.http#1 (get-profile)"],
      "root_cause": "接口返回 HTTP 401 鉴权或权限拒绝",
      "fix_suggestion": "检查登录前置步骤、Bearer Token 环境变量或 Cookie 会话文件"
    }
  ]
}
```

### 3.3 AI 语义归并

```bash
RUPOST_AI_BASE_URL=http://127.0.0.1:9002/v1 RUPOST_AI_API_KEY=demo \
  rupost ai triage target/report.json
# → ## AI Failure Triage Summary (...)   # ai_enhanced: true
```

AI 将同根因的签名桶合并（如"网关超时"桶 + "上游 5xx"桶 → 一个基础设施簇），
**守恒保证**：所有簇的 `count` 之和恒等于失败总数，LLM 只能给桶"贴标签"
（标题/分类/根因），无法移动成员或发明不存在的用例——幻觉 ID 与漏桶被引擎自动兜底。

### 3.4 免落盘直连

```bash
rupost test tests/ --triage        # 汇总后直接输出聚类报告（等价于读内存结果）
```

## 4. 报告契约（`--report-file` 产物）

强类型 `JsonFileReport`，向后兼容（新字段均带默认值，旧消费者不受影响）：

```jsonc
[
  {
    "file_path": "tests/auth/login.http",
    "results": [
      {
        "request_number": 1,
        "name": "user-login",
        "method": "POST",
        "url": "https://api.example.com/login?api_key=%5BREDACTED%5D", // URL 脱敏
        "status": 401,
        "duration_ms": 256,
        "success": false,
        "error": "请求执行失败: ... (URL 已脱敏)",
        "skipped": false,
        "assertions": [
          {
            "raw": "status == 200",
            "passed": false,
            "message": "Status code mismatch",
            "expected": "200",
            "actual": "401"
          },
          {
            "raw": "body.access_token exists",       // 敏感键断言（token/password/secret...）
            "passed": false,
            "expected": "[REDACTED]",                 // 值整体掩码——可能源自响应体敏感字段
            "actual": "[REDACTED]",
            "message": "[REDACTED]"
          }
        ],
        "warnings": [],
        "response_body_snippet": "{\"error\":\"invalid\",\"access_token\":\"[REDACTED]\"}"
        // 仅失败请求携带；JSON 敏感键递归掩码 + 512B UTF-8 安全截断
      }
    ]
  }
]
```

**脱敏承诺**：发送给 AI 与写入报告前，URL query 敏感值、JSON 敏感键
（token/password/secret/api_key/authorization…含大小写与子串变体）、错误文本内嵌
URL 全部掩码；敏感键断言的期望/实际值整体掩码。终端输出可用
`grep` 抽查原始密钥值验证（集成测试 `tests/ai_triage_test.rs` 已内置 canary 断言）。

## 5. 签名 / 桶 / 簇 速查

- **签名（signature）**：`HTTP {status} @ {路径模式}`（数字与 UUID 段折叠为 `:id`，
  取前 4 段），如 `HTTP 401 @ /v1/users/:id`；网络失败为 `NetworkError @ {模式}`；
  未定义变量优先为 `UnresolvedVariable @ {模式}`。
- **桶（bucket）**：同签名失败聚合，规则层给出默认分类与建议（确定性，最多 20 桶，
  溢出并入尾桶）。
- **簇（cluster）**：AI 把同根因的桶合并后的输出单元；`category` 固定六类
  （Auth / Upstream5xx / SchemaDrift / AssertionMismatch / NetworkAndTls /
  VariableOrConfig），未知分类无损落入 `Other(...)`。

## 6. 与 `ai assert` 的分工

| | `ai assert`（创作期） | `--explain` / `ai triage`（诊断期） |
| :-- | :-- | :-- |
| 时机 | 写测试时 | 测试失败后 |
| 输入 | 一句自然语言 | 真实请求/响应/断言上下文（脱敏后） |
| 输出 | 可粘贴的 `@assert` 行 | 根因解释 / 根因簇报告 |
| 产物 | 进入测试文件，离线可回归 | 只读建议，不改任何文件 |
