# 故障复现包（Incident Bundle）示例

一次失败的 `rupost test` 产出脱敏、可重放、可移交给同事或 CI 的目录工件——
"失败"从一段难以复现的终端输出变成可提交、可附到 issue 的事实。

本示例全部走本地 loopback Mock（`mock.json`），零公网依赖、零真实凭据。

## 快速体验

```bash
# 1. 启动本地 Mock（loopback）
rupost mock examples/incident_bundle/mock.json --port 8095 &

# 2. 运行必然失败的套件，产出复现包（仅在失败时创建目录）
rupost t examples/incident_bundle/payments.http \
  --var base_url=http://127.0.0.1:8095 \
  --incident-out /tmp/payment-failure \
  --mask-secret-value demo-secret-token-xyz
# 退出码 1（测试失败本身），目录 /tmp/payment-failure 被创建

# 3. 消费复现包：本地重放（换目标环境）与规则分诊
rupost replay /tmp/payment-failure/snapshot.json --target http://127.0.0.1:8095
rupost ai triage --snapshot /tmp/payment-failure/snapshot.json

# 4. 成功对照：全绿运行即使给了 --incident-out 也不创建目录
rupost t examples/incident_bundle/green.http \
  --var base_url=http://127.0.0.1:8095 \
  --incident-out /tmp/should-not-exist
```

## 目录契约（三文件）

| 文件 | 含义 |
| --- | --- |
| `manifest.json` | Incident schema 1.0：来源路径、环境、通过/失败/跳过计数、工件清单与脱敏声明 |
| `report.json` | JSON 报告（与 `--report json` 同一契约，可直接 `ai triage`） |
| `snapshot.json` | 执行快照 Schema v1.0，**强制** `masking: "secrets"`（键名 + 显式值双重掩码，无关闭开关） |

安全语义：

- 目标目录必须**不存在**——已存在或为符号链接一律拒绝（exit 2），绝不覆盖；
- 复现包内绝不写入原始凭据：`--mask-secret-value` 的显式值全表面替换为 `[REDACTED]`；
- 复现包**不复制**源 Markdown/`.http`（避免文档内字面凭据二次落盘），重放所需的一切
  都在 `snapshot.json` 里；
- 工件创建失败返回 exit 2，不掩盖测试本身的 exit 1 结果。

## 边界说明

- 掩码后 body diff/验签类结论按"数据不完整"降级（replay 加载时会明示警告）；
- 复现包含整套非 teardown 交互（登录等上游依赖是重放的必要上下文），不摘除非失败请求；
- 退出码契约：全绿 0 / 测试失败 1 / 工件创建失败 2。exit 2 三来源鉴别（stderr 前缀）：
  `error: ...`=clap 参数错误 / `[ERROR] 解析错误: ...`=AST 错误 / `[ERROR] 故障复现包创建失败: ...`=工件 I/O；CI 判读：exit 2 一律视为配置/路径问题，重跑无意义。
