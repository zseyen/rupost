# RuPost 能力与云接入总览：实现功能 / 接入形式 / 云原生方案（含查漏补缺）

> **创建日期**: 2026-10-28 ｜ **性质**: 三块内容的汇总与查漏补缺——①实现功能盘点 ②云开发接入形式 ③云原生介入方案。查漏结论已落为主执行文档的队列卡（云接入批），本文是**分析底稿与边界登记**。
> **事实源分工**: 队列裁决 = [主执行文档](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-master-execution-runbook.md) ｜ 需求三维 = [需求总矩阵](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-28-requirements-matrix.md) ｜ 本文 = 形态分析与查漏记录
> **基线**: 主线 `4b7b7fc4`；E2/E5 与治理批在 `feature/API调试功能完善` 分支待合并

---

## 一、实现功能盘点

### 1.1 已交付底座（主线，代码实证）

**17 项核验能力**（全文档审计确认与代码一致）：`.http`/`.md` 双格式解析、8 运算符断言、`@capture` + 四层级联变量、动态变量、目录递归 DAG（Kahn/环检测/并行/状态克隆）、网络诊断（DNS/TCP/TLS/TTFB + X.509）、Mock 服务器（Trie/条件变体/快照转译）、SSE 剧本（含 LLM 流式规整）、WebSocket 剧本（软匹配/重连自愈）、历史 + `generate`、别名与 curl/httpie 内联、TUI 全套（双 Tab/过滤/主题/预折行）、Cookie 三模式 + 环境隔离、`init` 三模板、快照录制/重放 MVP、`export` 命令导出（凭据脱敏）。

**2026-10-27/28 会话增量**：凭据模板化（`{{变量}}`，泄密 P0 修复）、语义化命名（`### GET /users`）、`--templatize-host`、`ai assert` Phase 0（B2 兜底 + crate 激活）。

### 1.2 分支待合并（`feature/API调试功能完善`）

E2 `--explain` / E5 `ai triage`（Step 0–3 全链 + review 加固：`src/ai` 提取、强类型报告 + `--report-file`、attribution + signature-bucket triage、CLI 接线）；**T12 治理批同分支已交付**（审计 P0 清理、归档横幅批、dev/progress_summary 合并、P1 修正批）。

### 1.3 已立项待做

见主执行文档队列：T10 审计（P0 下一个动作）、T1/T2 生成语义收尾、DBG-1~3、M1（P0 上下文/零配置 + import curl + E2 合并）、M2（E5 + bootstrap）、M3（MCP）、T8 junit、T9 分发、T11 探路、DX-1~5。

### 1.4 功能层查漏结论

**无新增漏项**——功能层的全部 open 项均已在队列或矩阵登记（T10 审计、X 系列候选、审计留痕候选、backlog 卡全集）。本次查漏确认的两处"实现 vs 文档"错位（E2/E5 分支状态、T12 分支状态）已由需求总矩阵 v1.1 修正。

---

## 二、接入形式：云开发的四种工作形态 × 五方案

### 2.1 四种云开发工作形态

| 形态 | 描述 | 对 rupost 的含义 |
| :-- | :--- | :--- |
| 甲 · 云 IDE 纯浏览器 | Codespaces web / Gitpod | 有终端；工作区卷持久化 |
| 乙 · 本地 IDE + 远程容器 | Remote-Containers / Gateway | 终端在远端，文件双向同步 |
| 丙 · SSH 云工作站 | 纯终端，无本地回传 | TUI 体验最优场景 |
| 丁 · 云端 AI Agent 主操作 | background agents | Agent 经工具调用操作——MCP 的场景 |

**关键判断**：甲乙丙三种形态里，"单二进制 + 本地文件"身份**原样成立**（容器内工作区就是它的"本地"）；真正的新命题是丁形态，以及**状态随容器生命周期漂移**这一共性问题。

### 2.2 五方案对比

| 方案 | 形态 | 其他产品先例 | 优势 | 劣势 | 定位一致 | 时机 |
| :-- | :--- | :--- | :--- | :--- | :-- | :--- |
| **A 原生终端直装** | 容器/云机内单二进制 | curl/Hurl/HTTPie；k9s 装跳板机 | 零架构改动；单二进制优势最大化；文件随工作区 Git 持久化 | `~/.rupost` 状态随容器丢失（需 volume 指引）；TUI 远程依赖终端能力（OSC 52/鼠标/真彩） | ✅ | **现在**（随 T9） |
| **B Feature 分发** | devcontainer feature + brew/Nix 壳 | devcontainer features 生态、mise 插件 | 一行接入；团队环境统一；维护极低（安装脚本薄壳） | 不新增能力；多生态多格式薄壳 | ✅ | T9 后 1–2 天 |
| **C Server/守护进程** | `rupost serve` 状态驻留 | wiremock/prism、LocalStack、**k6 CLI+Cloud（阶梯结构最像的先例）** | 状态服务端驻留；无终端可消费；阶梯二自然形态 | 违背本地优先；鉴权/多租户安全面陡增 | ⚠️ gate 阶梯二，"无状态执行器"变体守 Git 事实源 | 商业化期 |
| **D MCP Server** | Tool 协议（run_request/get_history/mock） | Postman MCP、Playwright MCP、GitHub MCP | 形态丁的正面回答——Agent 缺"网络手脚"；B0 解耦；M1/M2 产线复用 | 依赖 MCP 生态；Agent 触发真实请求需 X 护栏转正 | ✅ **已裁决 M3** | M3 |
| **E CI/CD Action** | 官方容器 action | Hurl/newman/k6 action | S5 回归正式形态；junit 消费；版本 pin | 依赖 T8；属流水线接入非开发接入 | ✅ | 随 T8 |
| **F Web UI** | 浏览器客户端 | Hoppscotch、Postman web | 浏览器即达 | GUI 红海；身份稀释 | ❌ 反模式维持 | 不做 |

### 2.3 接入层查漏（3 项，均已落队列）

1. **OSC 52 剪贴板从未入队**——吸收路线图标"推荐优先落地"，但主执行文档无此卡（SSH 场景 TUI 的关键体验件）→ **CLOUD-2**；
2. **devcontainer feature / 官方 Action 不在任何卡上**——纯薄壳但没人负责 → **CLOUD-1** / 随 T8；
3. **容器内状态语义未定义**——history/cookies 在临时容器中即失，"有状态 volume 挂载"还是"无状态运行"无文档说法 → **CLOUD-3**（无状态为默认语义，volume 为可选）。

---

## 三、云原生方案：验证平面站位

### 3.1 平面站位（三平面选一）

数据平面（mesh/eBPF）❌ Won't do ｜ 控制平面（CRD/Operator）⏳ gate ｜ **验证平面 ✅**——契约仿真（S2 Mock）、部署前回归（S5 DAG）、部署后冒烟/灰度比对（replay + Smart Diff）、运行期取证（R7 锚点）四个验证时刻全覆盖，这就是 rupost 对云原生应用的介入位置。

### 3.2 三堵墙（集群外客户端的极限）

①**网络墙**：ClusterIP 服务须逐个 port-forward，集群内 DNS 写不进用例；②**路径墙**：port-forward 绕过 mesh/网关/NetworkPolicy，验证真实性打折；③**权限墙**：prod 只读取证需要集群内身份与可审计位置。

### 3.3 介入形态对比

| 形态 | 其他产品先例 | 优势 | 劣势 | 定位一致 | 时机 |
| :-- | :--- | :--- | :--- | :-- | :--- |
| **A 集群外客户端**（现状） | kubectl、Newman、Hurl | 零足迹；全套能力原样 | 三堵墙全吃 | ✅ | 默认形态 |
| **B 镜像 Job/CronJob** | curl CronJob 冒烟、Trivy 镜像、TestKube 的底层模式 | **三堵墙一次拆完**（真实路径 + 集群内 DNS + RBAC 身份）；CronJob 化 = 生产冒烟回归（A4 云原生落地） | 用例需进集群（ConfigMap/Git sync——GitOps 化后回到事实源）；报告多一跳；无状态执行 | ✅ | **下一步**（随 T9） |
| **C CRD/Operator** | TestKube（Test CRD）、k6-operator、Chaos Mesh、Flagger | 测试即声明式资源——GitOps diff/审计/回滚免费；失败阻断发布（A4 升维为交付门禁） | 重工程；TestKube/k6-operator 已占位（红海）；与单二进制极简身份张力最大 | ⏳ gate | 需求证据后 |
| **D GitOps Hook** | Argo CD PostSync / Flux 钩子（migration Job 同款） | **C 的八成价值、B 的一成成本**——部署即冒烟 | 绑定 GitOps 工具；阻断语义不如 CRD 正式 | ✅ | 随 B 文档化 |
| **E mesh/eBPF/sidecar** | Istio 镜像、Keploy、Pixie | — | 流量镜像已有 Won't do；sidecar 运维负担 | ❌ | 维持 |

### 3.4 云原生层查漏（4 项）

1. **镜像打包完全缺位**：仓库无 Dockerfile、T9 卡无镜像项——而镜像形态（B/D）是云原生介入的最小可行形态 → **CLOUD-1**（multi-arch、non-root）；
2. **GitOps hook 无示例**：Argo PostSync 用法零文档 → CLOUD-1 附 `examples/gitops/`；
3. **X 护栏的集群内形态未设计**：safety 声明经环境变量注入 Job spec——随 X 系列立项时补；
4. **边界未登记**：消息队列/非 HTTP 协议（Kafka 等）测试从未显式排除——云原生应用常有 MQ 组件，不登记会被当成缺口 → 并入 Won't do（与 gRPC 同理由：HTTP 验证平面定位）；serverless 无需专项（API 网关端点 = HTTP 已覆盖）；有状态集群内常驻不做（与 C gate 同理由）。

---

## 四、查漏补缺总表

| # | 发现 | 来源层 | 处置 | 落点 |
| :-- | :--- | :-- | :--- | :--- |
| G1 | 容器镜像打包缺位（无 Dockerfile、T9 无此项） | 云原生 | T9 扩展 | runbook T9 卡 |
| G2 | devcontainer feature / 官方 Action 无归属 | 接入 | 新增薄壳卡 | runbook CLOUD-1 |
| G3 | OSC 52 推荐优先但从未入队 | 接入 | 正式入队 | runbook CLOUD-2 |
| G4 | 容器内状态语义未定义 | 接入 | 无状态默认 + volume 可选，文档化 | runbook CLOUD-3 |
| G5 | GitOps hook 示例缺位 | 云原生 | CLOUD-1 附 examples/gitops/ | runbook CLOUD-1 |
| G6 | MQ/非 HTTP 协议边界未登记 | 云原生 | Won't do 补一行（与 gRPC 同理由） | runbook Won't do |
| G7 | X 护栏集群内注入形态 | 云原生 | 随 X 系列立项补设计 | 需求总矩阵候选注记 |
| G8 | E2/E5 与治理批分支状态未反映 | 实现 | 需求总矩阵 v1.1 已修 | 已闭环 |
| G9 | 功能层 open 项复核 | 实现 | 无新漏（全在队列/矩阵） | — |

---

## 五、行动落点

1. **已落队列**（本次）：runbook T9 卡扩展（镜像 + feature + Action 仓库）、云接入批 CLOUD-1~3、Won't do 补 MQ 边界；
2. **待裁决**：`feature/API调试功能完善` 分支合并时机（E2/E5 + 治理批）；X 系列转正（R7 前置）；C server 形态（阶梯二 gate）；
3. **阶段验收参考**：devcontainer feature 一行接入可用；镜像 multi-arch 可拉取；GitOps hook 示例端到端绿；TUI over SSH 完成 OSC 52 复制。
