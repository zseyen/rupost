# RuPost 文件夹/多文件测试高级控制流 Future Backlog

本文件记录了在 Sprint 2.5 阶段为保障产品核心功能发布而临时裁剪的高级特性。这些特性将在后续版本中依据优先级统一排期实现。

---

## 📋 裁剪特性清单

### 1. 条件执行 (`@skip-if`)
*   **状态**：已完成语法预研，延期实现。
*   **需求描述**：在 CI/CD 中能够根据环境变量、用例执行状态等条件动态跳过特定请求或文件（如生产环境跳过危险请求）。
*   **示例语法**：
    ```http
    ### @skip-if {{env.STAGE}} == "prod"
    DELETE https://api.example.com/dangerous-cleanup
    ```
*   **技术架构要点**：
    *   在解析层定义并解析 `skip_if: Option<String>`。
    *   在执行时，复用 `VariableResolver` 对该表达式进行环境插值。
    *   在 `src/runner/executor.rs` 外部提供独立的表达式求值纯函数，将条件求值与 HTTP 执行逻辑彻底解耦。

### 2. 循环执行 (`@loop`)
*   **状态**：已完成语法预研，延期实现。
*   **需求描述**：对特定写接口进行重复压测，或重复触发以制造测试数据。
*   **示例语法**：
    ```http
    ### @loop 5
    POST https://api.example.com/data/generate
    ```
*   **技术架构要点**：
    *   不修改单次执行的 `execute_one` 接口，在其外部通过平铺循环或并发池调用，最终汇总产生多个测试结果项。

### 3. 可视化 HTML 报告与表现层模板 (Sprint 4)
*   **状态**：延期实现。
*   **需求描述**：提供美观、直观的单页 HTML 测试汇总报告，供 Jenkins / GitHub Pages / 团队内部展示。
*   **技术架构要点**：基于 `handlebars-rust` 或 `tera` 对 `BatchExecutor` 产生的 JSON 进行静态渲染。

---

## 📌 2026-10 竞争格局复审登记（来源：[竞争格局复审](file:///Users/zsyzzx/project/rust/rupost/doc/competition/2026-10-25-competitive-landscape-review.md)）

### 4. gRPC / GraphQL 专用支持 — **Won't do（当前）**
*   **状态**：显式取舍，非遗忘。Posting / Bruno / Hoppscotch / Resterm 已将其做成通用客户端标配，RuPost 定位"终端回归引擎"而非通用客户端，跟进无差异化收益。
*   **重开条件**：若做，优先与流式叙事一致的方向（gRPC-Web / server streaming 场景的剧本断言），复用现有 `AssertTarget` 统一断言体系。

### 5. HTTP/3 支持
*   **状态**：登记观察。xh 与 curl 已支持；RuPost 经 reqwest 的能力边界待确认后再排期。

### 6. `generate` 命令的 AI 延伸（历史 → 测试建议）
*   **状态**：**已正式设计**（2026-10-26，从"远期构想"升级为待实施方案）——演进为 [`rupost ai bootstrap` 自举测试方案](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-26-ai-bootstrap-proposal.md) Phase 1（历史→语义增强测试文件，含脱敏/引擎 lint/降级语义）。前置依赖：AI 激活 Phase 0（[AI 激活 PRD](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-26-ai-activation-prd.md)）。

---

## 📌 2026-10-27 对抗审查裁决登记（来源：[生成语义与调试取证执行计划](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-generation-semantics-execution-plan.md)）

> 背景：云调试/自动日志/录制需求经 7 个对抗性子代理审查后降级收敛，以下为显式取舍（比照 gRPC Won't do 先例，防止摇摆）。

### 7. 代理抓包（`rupost record --port`）— **Won't do（当前）**
*   HTTPS MITM 需 CA 信任链，是 Reqable/Charles/mitmproxy 的重战场；`reqable_analysis` 自身结论即"互补而非竞争"。
*   **翻案条件**：出现"无 GUI 环境批量捕获且用户拒绝装 mitmproxy"的真实需求证据；或反向代理模式（base_url 指向 rupost，复用 axum_adapter）被独立验证有需求。

### 8. 生产环境平台日志 adapter（Loki/SLS/CLS/ES）— **Won't do（当前）**
*   依赖"组织已有集中日志平台 + 愿意签发只读凭据"两个外部前提；0 收入阶段无付费钩子形态。
*   **翻案条件**：对齐产品策略变现阶梯二（团队 SaaS）的付费钩子形态。

### 9. kubectl 委托多服务日志聚合 — **推迟（探路先行）**
*   内建唯一保留项为通用 `--on-failure` 钩子（见执行计划 Phase 2d），kubectl 聚合让用户经钩子自行接 stern 管道验证需求。
*   **前置实验**（编码前必须）：CRI 多行日志时间戳形态（已证伪"续行无时间戳"假设）、目标服务 trace 回传现状、`kubectl logs -l --all-containers --prefix --timestamps` 组合行为。

### 10. `history export` 桩转实（T2 事后补录）— **推迟（搭车项）**
*   与 `generate` 功能重叠；前提（history 缺 body）已被代码推翻。随 replay per-entry 结果（Phase 2a）同批搭车，约 20 行。
*   **前置决策**：旧历史条目 body=None 的导出语义（跳过 or 占位）。

### 11. 生成语义批次（Phase 1）
*   语义化命名 / Host 模板化 / 动态值反向识别 / 链式值溯源（复制型）——设计与量级见[执行计划 §3-§4](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-generation-semantics-execution-plan.md)；其中动态值识别即 ai bootstrap 机械降级路径，与 A1 激活存在复利关系。
*   `rupost g --failed` 失败选择器、`--emit-env`、`--format md`：backlog 观察，有真实抱怨再排。

### 12. TUI 历史反向还原路径审计 — **已完成（2026-10-28，T10）**
*   **审计结论**：`format_request_snapshot_to_http`（`src/tui/state/history.rs`）原样写入全部 Header/URL/Body，调用点仅 `sync.rs`/`confirm.rs` 两处、产物只进**内存编辑器**（`editor_file_path = None`、`is_dirty = false`），TUI 全目录无剪贴板/文件导出路径——**无新暴露面**（凭据的持久化明文本就是 history 本身，属本机存储）。
*   **显式裁决**：TUI 还原**有意保留凭据真值**——供未来内联编辑重发（模板化会破坏重发语义）；对外分享走 `export --redact`（脱敏）或 `generate`（模板化），三者分工明确互不越界。
*   **契约锁定**：`test_history_restore_keeps_credentials_verbatim_by_design`（`src/tui/state/history.rs`）——若未来有人给还原路径加模板化/脱敏，此测试先行失败提醒重新裁决。
