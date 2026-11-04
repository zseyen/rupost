# 调试链全流程演示（Debug Chain Demo）

按 **画像 × 开发阶段** 实跑 RuPost 全部调试能力的最小可复现用例集。统一使用说明见 [调试指南](../../doc/guides/debug_guide.md)。

## 文件

| 文件 | 用途 |
| :-- | :-- |
| `mock_backend.json` | 订单系统 mock（**正常态**）：登录按 Body 密码条件变体、资料按 Authorization 头变体、报表固定 500（带 `X-Request-Id: trace-anchor-42`）、配额 quota=100 |
| `mock_upgraded.json` | 同系统的**升级故障态**：profile→500、orders→503（模拟"后端改了行为"） |
| `suite.http` | 五请求套件：login（@capture token）→ profile（级联依赖）→ orders → report（服务端故障）→ billing（契约断言故意失败） |
| `green.http` | 联调回放基线：全绿三连（login/profile/orders） |
| `run_demo.sh` | 一键实跑全部演示阶段 |

## 一键演示

```bash
cargo build --release
./run_demo.sh
```

## 阶段 × 画像 × 功能映射

| 阶段 | 画像 | 演示的功能 | 实跑看点 |
| :-- | :-- | :--- | :--- |
| 1 开发期 | 小李·后端 | `--only` 定点重跑、`--explain` 就地归因、AI 降级提示 | 规则版同时给出"捕获失败 + 401 鉴权"两条类目 |
| 2 测试期 | 小赵·QA | `--report-file` + `ai triage` 签名聚类 | 双簇（AssertionMismatch / Upstream5xx）、计数守恒、`<file>#<n>` 定位 |
| 3 联调期 | 小张·前端 | `--save-snapshot` 录制、`replay --target` 定位到条、exit 1/0 | `Failed entries` 清单、CI 门禁双向退出码 |
| 4 取证 | 小李·后端 | `--on-failure` 钩子 + trace 锚点协议 | mock 注入的 `X-Request-Id` 被 `RUPOST_FAILED_TRACE_ID` 捕获；无 trace 头时为空串 |
| 5 沉淀 | 全员 | `history export` → `replay` 供给链、`ai bootstrap --no-ai` 机械基线 | 供给链全绿；body 明文凭据被安全审计点名 |

## 注意

- 全程零外网、零 AI 配置（有意展示 B1 降级路径）；
- 演示历史隔离在 `.demo-history/`（`RUPOST_HISTORY_DIR`），不污染真实 `~/.rupost`；
- mock 密码条件值 `hunter2` 仅用于本地演示。
