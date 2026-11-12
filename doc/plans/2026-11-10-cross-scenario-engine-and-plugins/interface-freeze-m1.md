# M1 接口冻结清单 (Interface Freeze List)

- **状态**: Frozen @ M1 交付（jj `2d800e9b` / `5bbc29c2` / `d7894800`）
- **用途**: 06 M1/M2 出口条件（05 ADR）。下列接口为 M2~M5 与外部插件生态的**稳定锚点**，任何变更必须走独立文档评审并更新本清单版本。
- **违反后果**: M3 IPC 输入/输出 JSON、M4 路由键、M5 AI 消费者均以本清单为准；就地变更将击穿"原子化提交"承诺并造成生态破坏性变更。

## 1. 领域实体（`src/plugin/types.rs`）

| 冻结项 | 契约 | 代码锚点 |
| :--- | :--- | :--- |
| `PluginStage` | 枚举 `Prepare/Transform/Sign`（MVP 注册面）+ `PostResponse/OnFailure`（保留字，03 §3.1；P2 放行前 manifest 中出现即错误） | `src/plugin/types.rs` |
| `PluginDirective` | `{directive: String, name: String, stage: PluginStage, optional: bool, params: HashMap<String,String>}`；**无 priority 字段**（05 ADR-D6 防复活） | `src/plugin/types.rs` |
| 执行仲裁 | 阶段不可逆升序 + 同阶段纯 FIFO 书写行序（稳定排序）；`@pipeline` 为 P2 且与阶段序冲突 = 语法错误 exit 2 | `PluginPipeline::sort_directives` |
| 可选声明 | **唯一拼写 `?optional`**；`optional=true`/`stage=`/`priority=` 均为显式 ParseError（含迁移指引） | `src/parser/metadata.rs::parse_plugin_directive` |
| 参数分词 | 引号感知（`"..."`/`'...'` + `\` 转义），含空格参数值不撕裂 | `tokenize_plugin_args` / `strip_surrounding_quotes` |
| `PluginPatch` | `{extra_headers, remove_headers, mutated_body, set_variables, warnings}`，Header 大小写不敏感 Upsert | `src/plugin/patch.rs` |

## 2. 内置宏契约（`src/variable/resolver.rs`）

| 冻结项 | 契约 |
| :--- | :--- |
| 函数宏拼写 | `{{$hmac_sha256(key, msg)}}`、`{{$sha256(msg)}}`、`{{$random_int(min, max)}}`（闭区间） |
| 输出形态 | 摘要为小写十六进制；`$random_int` 输出十进制整数字符串 |
| 求值序 | 函数宏先于简单变量；宏参数在求值时做嵌套 `{{...}}` 插值（守护测试锁定，RFC 4231 Case 2 复现） |
| 参数分词 | 引号感知 + 顶层逗号；含逗号/空格参数不撕裂 |
| 失败语义 | 参数个数/取值非法 → 保留字面量，由 `unresolved_placeholders` 显式告警（反静默） |
| 依赖 | `sha2 0.10` / `hmac 0.12`（纯 Rust，零系统依赖） |

## 3. 快照黑匣子契约（`src/history/model.rs`，Schema v1.0）

| 冻结项 | 契约 |
| :--- | :--- |
| 版本 | `schema_version: String = "1.0"`；演进 = 只增字段/禁改语义/破坏性变更递增版本（02 §2.6） |
| 文件结构 | `{schema_version, generated_at, environment, runner_fingerprint{os,arch,rupost_version}, masking, entries[]}` |
| 条目结构 | `{request_number, name, file_path, success, outcome, status, duration_us, plugin_duration_us, request, response, error, timing_waterfall, socket_diagnostics, assertions, warnings}` |
| 单位制 | 时序字段一律 `_us`（微秒 u64）；`ExecutionErrorDetail.duration_ms` 沿用 history 既有定义 |
| 键集合稳定 | 条目核心 Option 字段序列化恒在（None → null），对齐 report_schema 键集合稳定哲学；`body_sha256` 仅截断时存在 |
| 时序语义 | `None` = 连接复用未发生；`total_us` = 已发生阶段之和；`duration_us − total_us = plugin_duration_us` |
| 截断 | 响应体 >256KB（`DEFAULT_MAX_SNAPSHOT_BODY_BYTES`）截断 + `body_truncated: true` + 截断前全文 SHA-256 |
| 权限 | 落盘强制 `0600`（02 §2.7，`write_execution_snapshot`） |
| 消费入口 | `read_execution_snapshot`（插件/miner）；`read_snapshot_suite_compat`（replay 双格式兼容：新契约优先，旧 SnapshotSuite 回退） |
| 排除规则 | `is_teardown` 条目不进快照（P1-11 防重放 DELETE） |

## 4. 已知缺口（冻结边界的诚实声明）

1. `timing_waterfall.tls_handshake_us` 仅在 diagnose 报告可得时填充（reqwest 公开 API 不暴露握手时序）；
2. `SocketDiagnostics.tls_cipher_suite/tls_version` 恒为 None（CertInfo 未采集，best-effort）；
3. ~~`plugin_duration_us` 恒为 None~~ ✅ M3 已接线（`TestResult.plugin_duration_us` → 快照字段）；`DegradedPass` ✅ M3 已接线（`TestResult.degraded` 驱动）；
4. ~~`--max-snapshot-body-bytes` CLI 旗标 M3 接线~~ ✅ M6-4 已接线（`save_batch_snapshot` 参数化上限，缺省 256KB 不变；截断 + 截断前完整 Body SHA-256 验真）；
5. 套件级 source 字段不在契约中（来源逐条目落在 `entry.file_path`）；
6. ~~**函数宏参数不含 `)}}` 序列**~~ ✅ 已修（`resolve_fn_macros` 状态机扫描器：引号感知 + 括号深度 + 转义处理；守护测试 `test_fn_macro_scanner_closing_braces_newlines_and_parens`）；
7. ~~**HeaderMap 采集链的多值/顺序限制**~~ ✅ 已修（`from_parsed` 改 `append` + 快照侧 `Vec<(String,String)>` 有序对；Set-Cookie×N 保留与掩码守护测试在 `snapshot_contract_test.rs`）；
8. ~~02 §3.3 消费侧 64MB 体积上限~~ ✅ M5 已在 `ai triage-snapshot` 消费者内置；`read_execution_snapshot` 通用入口内置校验 ✅ M6 已补齐；
9. **参数转义语义已统一**（M1 评审修复）：`\"` → `"`，反斜杠不进参数值；`$random_int` 无参历史形态继续可用（向后兼容，文档只承诺参数化形态）；
10. **插件运行时声明契约（v2.1.1 新增，03 §3.1）**：manifest `runtime: {interpreter, min_version?, modules?}`——`interpreter` 存在性 = readdir 级 PATH 预检（**不 spawn 版本 probe**）；预检未过 → 插件 `pending` 状态（03 §3.1.1 三态机），与"缺失"共享降级开关；失败分类学新增 `runtime_missing` / `dependency_missing`（stderr 表驱动特征，03 §8.3）。✅ M6-1/M6-2 已落地（`preflight_runtime` 零子进程预检 + `PluginHealthStatus` 三态响应式重算 + `plugin list` 三态与三选一处方）；
10.5 **时序采集契约（v2.1.2，M7-A 已落地）**：机制已按探针实测修订为 `TimingDnsResolver`（`reqwest::dns::Resolve`）+ `TimingConnectorLayer`（tower Layer）双钩子 + `task_local!` 单连接槽位（替代原"自建 TCP connect"措辞，详见 m7-timing-connector-design.md M7-A 修订版）；快照 `timing_source`（connector/probe_degraded/none）与 `timing_attribution`（exact/approximate）字段已落盘；HTTPS 无 `--debug` 时 `tcp_connect_us` 为 (tcp+tls) 合算、`tls_handshake_us` 为 None（诚实声明）；代理 → approximate；并行池竞争 → approximate（`reconcile_timing_metadata` 后置收敛）；`--timing off` → waterfall=null + source=none（02 §2:135 已兑现）；`tls_handshake_us` 默认 None（@diagnose 重档补）；
11. ~~**M6 剩余缺口收尾**~~ ✅ M6 全项落地：exit 2 AST 归位（`tests/ast_exit_code_test.rs`）、DoD 性能验证四测试（`tests/plugin_dod_test.rs`：sleep 3s 时延隔离 / 100 请求 ≤2 子进程 / fail-fast 组级 kill / 并发×插件）、HeaderMap 保序（见 #7）；#11 保留项仅剩 `SocketDiagnostics.tls_cipher_suite/tls_version`（见 #2，best-effort 诚实缺口）。

## 5. 二进制体积基线（06 M1 出口条件）

| 度量 | 值 |
| :--- | :--- |
| 审查基线（main 侧历史测得） | 18,965,952 bytes |
| M1 交付后（本线 release 构建，含 sha2/hmac） | 18,516,896 bytes |
| 净增量 | **−438.5 KiB**（预算 ≤+500 KiB，达标；线下行差异主导，宏依赖增量被吸收） |

> M2/M3 每次交付后在本清单追加新行，增量 > +500 KiB 即触发 DoD 告警。
