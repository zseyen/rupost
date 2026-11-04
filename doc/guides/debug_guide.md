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

## 9. 端到端实战演练：一次 401 的完整排障

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

---

## 10. 命令速查表

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
