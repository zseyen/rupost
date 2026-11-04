# RuPost 调试指南 (Debug Guide) —— 全链路调试功能统一说明

本指南是 RuPost 调试能力的**唯一统一入口**：从发起请求、网络初诊、失败归因、批量分诊、服务端取证，到快照重放定位、修复验证与成果沉淀。所有命令示例均已对当前版本 CLI 核实。

一次 API 调试在时间轴上展开为八个环节（调试链 D0–D7），本指南按链路组织：

| 环节 | 你要做的事 | 对应命令/功能 | 详见 |
| :-- | :--- | :--- | :--- |
| D0 触发 | 发起请求（内联/文件/TUI） | `rupost GET ...` / `rupost t` / `rupost tui` | §1 |
| D0+ 定点 | 只重跑某一条请求 | `t api.http#3` / `--only` | §1.3 |
| D1 观察 | 看清响应与断言结果 | 终端高亮 / `--report json` | §1.2 |
| D2 初诊 | 分辨网络通/断、TLS/证书问题 | `--debug-on-failure` / `diagnose` | §2 |
| D3 归因 | "为什么这条失败" | `--explain` | §3 |
| D4 分诊 | "50 条红里哪几簇" | `ai triage` / `--triage` | §4 |
| D5 取证 | 拿着 trace-id 查服务端日志 | `--on-failure` | §5 |
| D6 修复验证 | 改一处 → 重放验证 | `replay` / `history export` | §6–7 |
| D7 沉淀 | 调试成果 → 回归资产 | `ai bootstrap --from history` | §8 |
| 画像与需求 | 谁在用、解决什么问题、边界在哪 | 画像 × 需求映射 × 四场景实战 | §9–10 |

---

## 1. 触发、观察与单请求重跑 (D0/D1)

### 1.1 发起请求

```bash
# 内联模式（httpie 风格）——注意全局参数必须紧跟主程序名
rupost --debug GET https://api.example.com/users

# 文件模式：单个 .http / .md 文件，或整个目录（DAG 拓扑 + 可并行）
rupost t examples/basic.http
rupost t examples/batch/ --mode parallel --concurrency 4
```

### 1.2 观察结果

* **终端高亮输出**：逐请求展示状态码、断言与捕获结果，批量运行附汇总面板；
* **机器可读报告**：`rupost t suite/ --report json`（stdout）或 `--report-file target/report.json`（落盘，含断言 expected/actual 与脱敏响应片段，供 §4 的 `ai triage` 消费）。

### 1.3 单请求重跑（修复验证的最小回路）

改完一处参数后不必重跑整个文件——两种定点方式：

```bash
# 片段语法：只跑第 3 条请求
rupost t api.http#3

# 正则机制：按文件内序号或 @name 选中（两者互斥）
rupost t api.http --only 3
rupost t api.http --only login
```

* 未选中的请求会被**跳过标记**（不发包），`request_number` 保留文件内原序号，与批量报告、`--report json` 中的 `<file>#<n>` 标识可互相定位；
* 越界序号会得到可读错误（列出合法范围/可用 `@name`），不会 panic；
* 被选中请求若引用了未捕获变量（上游未跑），按既有语义走"未定义变量警告 + 字面发送"，终端显式提示。

---

## 2. 网络初诊 (D2)

### 2.1 失败时自动诊断

```bash
# 失败的请求自动附带 DNS/TCP/TLS/TTFB 拆分诊断；成功时零输出（零噪音）
rupost t suite/ --debug-on-failure

# 每个请求都做全程诊断（含成功请求）
rupost t api.http --debug
```

诊断信息同时进入 JSON 报告的 `diagnose_report` 字段（`--report json` 时机器可读）。网络层拆分与 X.509 证书健康分析的完整说明见[网络诊断工具指南](file:///doc/guides/network_diagnostics.md)。

### 2.2 主动诊断任意 URL

```bash
rupost diagnose https://api.example.com   # 别名：rupost d <URL>
rupost d https://api.example.com --report json   # 13 字段机器可读报告
```

---

## 3. 失败归因：`--explain` (D3)

断言失败时在失败请求下方**就地**输出原因分析与修复建议：

```bash
rupost t api.http --explain
# 等价别名与环境变量开关：--ai-explain / RUPOST_AI_EXPLAIN=1
```

输出分两段：

```text
  [Possible Causes]
    - HTTP 401 鉴权失败，请检查 Authorization Header、Token 是否过期或 --cookie-file 会话状态。
  [AI Explanation]                ← 仅当配置了 AI（OpenAI 兼容后端 / 本地 Ollama）
    Summary: 认证失败导致后续字段断言未命中
    ...
```

* **确定性规则版基线永远输出**（401/403 鉴权、404 路由、400/422 参数、429 限流、5xx 服务端、超时/DNS/TLS 分类目）；
* AI 根因按**不同错误签名**去重调用（Top-3 配额 + 连续 2 错熔断），AI 缺失/故障时自动降级为规则版 + 黄色提示；
* **退出码严格由测试本身决定**——AI 只读介入，不改变成败判定。

---

## 4. 批量分诊：`ai triage` (D4)

CI 一夜红了 50 条时，把失败聚成根因簇：

```bash
# 方式一：直接在批量运行后输出分诊
rupost t suite/ --triage

# 方式二：对已落盘的报告离线分诊（CI 产物二次分析）
rupost t suite/ --report-file target/report.json
rupost ai triage target/report.json
rupost ai triage target/report.json --json   # 机器可读聚类结果
```

输出形如：

```text
[Cluster #1] [Auth] 鉴权失败 (31 tests)
  Signature : HTTP 401 @ /api/v1/*
  Sample    : tests/auth/login.http#1 (user-login)
  ...
```

* 确定性签名分桶（数字/UUID 折叠 `:id`）保证**计数 100% 守恒、零幻觉 ID**；
* 无 AI 时降级输出确定性分组 + 黄色警告，exit 0。

---

## 5. 深挖取证：失败钩子与 trace 锚点 (D5)

API 层归因后要追服务端日志时，让 rupost 把锚点交到你的命令手里：

```bash
rupost t suite/ --on-failure 'stern deploy/api --grep $RUPOST_FAILED_TRACE_ID'
```

每个失败请求触发一次 shell 命令，注入以下环境变量：

| 变量 | 含义 |
| :-- | :--- |
| `RUPOST_FAILED_URL` | 失败请求 URL（**已脱敏**，敏感 query 掩码） |
| `RUPOST_FAILED_STATUS` | 响应状态码（无响应时为空） |
| `RUPOST_FAILED_TRACE_ID` | 按 `Traceparent > X-Request-Id > X-Trace-Id` 优先级提取；全部缺失时为空串 + warnings 可见 |

* 钩子的 stdout/stderr 一律透传到 rupost 的 **stderr**——`--report json` 的 stdout 保持机器可读纯净；
* 10s 超时强制终结整个进程组（`sh -c 'a | b'` 管道钩子的子孙进程一并清理）；钩子超时/非零退出**不改写测试退出码**、不阻塞 Fail-Fast；
* 服务端日志聚合（stern/kubectl）完全委托用户管道，rupost 只提供锚点。

---

## 6. 快照重放与失败定位 (D6)

把录制好的快照打到另一套环境，失败**定位到条**：

```bash
rupost replay snap.json --target http://staging.example.com
rupost replay snap.json -t http://127.0.0.1:8080 -v   # -v 显示响应体详情
```

输出示例（一成 一败）：

```text
[*] Replaying request #1 [GET http://origin/ok]...
  [✓] Status matched: 200 (in 3.5ms)
[*] Replaying request #2 [GET http://origin/login]...
  [✗] Status mismatch: expected 200, got 401
──────────────────────────────────────────────────
  Total: 2, Passed: 1, Failed: 1
  Failed entries:
   - #2 GET http://staging.example.com/login (expected 200, got 401)
```

关键语义：

* **`--target` 覆写**：只改写协议/主机/端口，Path 与 Query 原样保留；
* **失败定位到条**：汇总后输出 `Failed entries` 清单（序号 1-based，与逐条打印的 `#N` 一致；mismatch 显示 expected/got，网络错误显示 error 原因）；
* **CI 门禁语义**：任一条目失败进程 **exit 1**，全部成功 exit 0（快照文件损坏/无法解析同样非零）；
* 比对语义为**状态码精确相等**（录制的响应状态 vs 实际状态）。

---

## 7. 历史导出与供给链闭环 (D6)

### 7.1 历史导出为可重放快照

```bash
rupost history export --last 5 -o snap.json    # 最近 5 条历史 → 快照文件
rupost history export -o snap.json             # 缺省最近 1 条
```

* `--last N` = 最近 N 条**历史条目**（导出顺序为旧→新，即重放执行序）；
* 旧条目无响应体（`body=None`）时**占位导出**（body 为 `Old snapshot: body not recorded`），条目不丢失、可追溯；
* 空历史：黄色警告 + 不写文件 + exit 0；
* 输出文件已存在时静默覆盖；导出文件**不脱敏**（与测试时 `--save-snapshot` 同一暴露面，请勿分享给不可信方）。

### 7.2 供给链闭环：历史 → 快照 → 重放

```bash
rupost history export --last 5 -o snap.json
rupost replay snap.json --target http://staging.example.com
```

导出的快照可直接被 `replay` 加载——历史里录下的问题，随时打到灰度环境复现验证。

> 录制侧：批量/单文件测试加 `--save-snapshot` 可在测试同时录制快照（详见 `rupost t --help`）。

---

## 8. 调试成果沉淀 (D7)

```bash
# 历史 → 语义增强回归文件（AI 缺失时降级为机械基线，exit 0）
rupost ai bootstrap --from history --last 20 -o regression.http

# 依据最近响应上下文生成精确断言
rupost ai assert "返回的 data.items 数组非空" --last 3
```

生成的回归文件可直接 `rupost t regression.http` 执行，形成"调试一次、长期回归"的闭环。

---

## 9. 用户画像与需求分析

### 9.1 谁在使用（画像 × 环境 × 主场景）

| 画像 | 环境 | 主场景 | 核心调试需求 | 首选能力入口 |
| :-- | :-- | :--- | :--- | :--- |
| **小李 · 后端开发** | 开发/测试 | 单接口失败排查 | 失败只给 expected/actual，手动比对平均 10–20 分钟/条；改完参数被迫重跑全文件；查服务端日志要切 k9s 逐 Pod 翻 | `--explain`、`t #3`、`--on-failure` |
| **小赵 · QA/回归** | 测试（CI 靶场） | CI 批量回归 | 一夜 50 红平铺，人工分诊 1–2 小时；重放只知道"挂了几个"不知道挂了哪几条；CI 需要可编程的成败信号 | `--triage`/`ai triage`、`replay` 定位到条 + exit 1、`history export` |
| **小张 · 前端联调** | 开发/联调 | 契约与环境差异比对 | 录制过的交互要在灰度/本地复现，定位"哪个接口行为变了" | `history export` → `replay --target` |
| **评估者** | 本地 | 快速上手与离线验证 | 零配置跑通闭环，不被外部服务卡住 | `init`、`mock`、`bootstrap --no-ai` |
| **流式场景标签（小王）** | 开发 | SSE/WS 实时链路调试 | 流式剧本、软匹配断言、断线自愈——**边界：快照重放不覆盖流式帧**（replay 走 HTTP 执行器，快照模型不含 SSE/WS 帧数据） | `@websocket`/SSE 剧本（见[协议专章](file:///doc/guides/websocket_testing.md)） |

### 9.2 需求 → 能力映射（每条需求都有"下一条路"）

| 用户需求（痛点实证） | 能力 | 兜底分级 | 机器可验验收 | 章节 |
| :-- | :--- | :-- | :-- | :-- |
| 失败只有原始 diff，单条归因 10–20 分钟 | `--explain` 就地归因 | **B1**（规则版确定性基线 + AI 增强） | verify_features 步骤 8（无 Key 降级锁定） | §3 |
| 修复回路断裂：改一处要跑全文件 | `t api.http#3` / `--only` | **B0 确定性** | 8 项 wiremock 零发包集成测试 | §1.3 |
| CI 50 红平铺，人工分诊 1–2 小时 | `ai triage` 签名聚类 | **B1**（分桶计数 100% 守恒） | 计数守恒 + 零幻觉 ID 测试 | §4 |
| 查服务端日志要切 k9s 逐 Pod 翻 | `--on-failure` + trace 锚点 | **B0 确定性** | 假脚本捕获 env 三元组集成测试 | §5 |
| 重放只回聚合计数，CI 无门禁信号 | replay per-entry + 失败 exit 1 | **B0 确定性** | 混合守恒/退出码双向/stdout 断言 | §6 |
| 历史无法变成可重放资产（供给链断） | `history export` 桩转实 | **B0 确定性** | 导出→重放闭环测试 | §7 |
| 网络通/断/TLS 问题难分辨 | `--debug-on-failure` / `diagnose` | **B0 确定性** | 四项守护 + 13 键 schema 契约 | §2 |
| 调试成果散逸，无法防复发 | `ai bootstrap --from history` | **B1** | 无 Key 机械降级离线闭环 | §8 |

### 9.3 兜底哲学：没有 AI 也能走完修复回路

所有 **B1** 能力（`--explain`/`ai triage`/`ai bootstrap`）都以**确定性规则版为基线**：AI 缺失、无 Key 或调用失败时自动降级（规则版原因 / 确定性签名分组 / 机械模板），黄色提示一行、退出码不变。配合全部 **B0** 确定性能力（定点重跑、重放定位、取证锚点、初诊），**AI 完全缺席时开发者仍能走通约 80% 的修复回路**——AI 是增强，不是依赖。

### 9.4 边界与安全注记

* **凭据外发面**：快照与历史**原样保存凭据头**；`replay`/`export` 把录制凭据打向 `--target` 指向的环境——请勿指向不可信方，对外分享请走 `export --redact`（终端命令导出）路径；
* **AI 出站默认关**：AI 归因/聚类仅在配置后主动触发，且发送前脱敏截断（4KB/条）；执行期零 AI（测试成败永远由确定性断言决定）；
* **取证只给锚点**：`--on-failure` 只暴露 URL/状态码/trace-id，日志聚合展示完全委托用户管道（stern/kubectl），rupost 不内建集群操作；
* **重放不覆盖流式**：快照模型不含 SSE/WS 帧，流式链路用剧本重跑验证。

---

## 10. 场景化实战演练

### 10.1 后端小李：401 归因与修复（SC-1）

```bash
# 1) 跑批量，红了
$ rupost t suite/
  Total: 5, Passed: 4, Failed: 1
  ✗ [2] POST http://api.example.com/login (401)

# 2) 就地归因
$ rupost t suite/ --explain
  [Possible Causes]
    - HTTP 401 鉴权失败，请检查 Authorization Header、Token 是否过期...

# 3) 改完 token 后只重跑失败那条
$ rupost t suite/ --only login
  ✓ [2] POST http://api.example.com/login (200)

# 4) CI 里配置失败取证钩子，trace-id 自动送进日志管道
$ rupost t suite/ --on-failure 'stern api --grep $RUPOST_FAILED_TRACE_ID'

# 5) 沉淀为回归资产，防止复发
$ rupost ai bootstrap --from history --last 20 -o regression.http
```

**链路复盘**：D0 触发 → D3 归因（规则版直接给出鉴权类目）→ D6 定点重跑（不跑全文件）→ D5 取证（可选项，接日志管道）→ D7 沉淀。全程 AI 缺席也能走完（步骤 2/5 自动降级）。

### 10.2 QA 小赵：CI 一夜 50 红（SC-2）

```bash
# 1) CI 夜跑落盘强类型报告（终端输出不受影响）
$ rupost t suite/ --report-file target/report.json

# 2) 早晨离线分诊——不重跑、无 AI 也出确定性根因簇
$ rupost ai triage target/report.json
  [Cluster #1] [Auth] 鉴权失败 (31 tests)
    Signature : HTTP 401 @ /api/v1/*
    Sample    : tests/auth/login.http#1 (user-login)
  [Cluster #2] [Server] 服务端异常 (9 tests)
  [Cluster #3] [Timeout] 请求超时 (10 tests)

# 3) 只修 Cluster #1 的登录服务，然后导出历史 → 重放验证
$ rupost history export --last 50 -o snap.json
$ rupost replay snap.json -t http://staging.internal
  Total: 50, Passed: 48, Failed: 2
  Failed entries:
   - #13 GET http://staging.internal/api/v1/orders (expected 200, got 500)
   - #37 GET http://staging.internal/api/v1/orders/88 (expected 200, got 500)

# 4) 退出码就是 CI 门禁：有失败即非零，接入现有流水线
```

**链路复盘**：D4 分诊把 50 红收敛为 3 簇（计数守恒，无幻觉 ID）→ 修根因簇 → D6 重放定位到条 → 剩余 2 条属于另一簇，继续按签名排查。

### 10.3 前端小张：联调环境差异定位（SC-3）

```bash
# 1) 联调时对真服务录制（连同 --save-snapshot 生成快照）
$ rupost t contract.http --save-snapshot snap.json

# 2) 后端同学改了部署 → 本地/灰度重放，定位行为变化的接口
$ rupost replay snap.json -t http://127.0.0.1:8080
  Total: 4, Passed: 3, Failed: 1
  Failed entries:
   - #2 POST http://127.0.0.1:8080/login (expected 200, got 401)
```

**能定位什么**：接口下线/路径变更（404）、鉴权行为差异（401/403）、服务端异常（5xx）、超时劣化——**状态码级**契约差异全部定位到条。**边界**：body 字段级差异比对（状态码不变、内容变了）属 T4 Smart Diff 规划项，当前版本建议配合 `--explain` 的规则版提示或对录制响应用 `ai assert` 生成断言覆盖。

### 10.4 后端小李：生产灰度取证（SC-4）

```bash
# 取证钩子按团队管道定制——rupost 只给锚点，聚合展示完全委托用户
$ rupost t suite/ --on-failure \
    'curl -s "$SLACK_WEBHOOK" -d "{\"text\":\"[$RUPOST_FAILED_STATUS] $RUPOST_FAILED_URL trace=$RUPOST_FAILED_TRACE_ID\"}"'

# 或直连服务端日志聚合（stern 按 trace-id 过滤多 Pod）
$ rupost t suite/ --on-failure 'stern deploy/api --since 5m --grep $RUPOST_FAILED_TRACE_ID'
```

**注意**：`RUPOST_FAILED_URL` 已脱敏（敏感 query 掩码）；trace 头全缺失时变量为空串且 `TestResult.warnings` 有一条提示；钩子 10s 超时强制整组终结，不会卡住测试流水线。

---

## 11. 命令速查表

| 场景 | 命令 |
| :-- | :-- |
| 只重跑某条 | `rupost t api.http#3` ／ `rupost t api.http --only @name` |
| 失败自动诊断 | `rupost t ... --debug-on-failure` |
| 主动诊断 URL | `rupost d <URL> [--report json]` |
| 失败就地归因 | `rupost t ... --explain` |
| 批量失败聚类 | `rupost t ... --triage` ／ `rupost ai triage report.json` |
| 失败取证钩子 | `rupost t ... --on-failure '<cmd>'` |
| 快照重放定位 | `rupost replay snap.json -t <url> [-v]` |
| 历史导出快照 | `rupost history export --last N -o snap.json` |
| 报告落盘 | `rupost t ... --report-file report.json` |
| 成果沉淀 | `rupost ai bootstrap --from history -o regression.http` |

---

## 相关文档

* [网络诊断工具指南](file:///doc/guides/network_diagnostics.md) —— D2 瀑布图与证书分析细节
* [批量测试指南](file:///doc/guides/batch_testing.md) —— DAG 依赖、并行模式与状态克隆
* [变量与环境管理指南](file:///doc/guides/variables_and_environments.md) —— 级联覆盖与 `.env`
* [测试指南](file:///doc/guides/testing_guide.md) —— 断言语法与测试组织
