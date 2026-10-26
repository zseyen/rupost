# AI 切入点补充分析：场景 × 画像 × 效率形态（v1）

> **创建日期**: 2026-10-27
> **方法**: 以[总体规划](file:///Users/zsyzzx/project/rust/rupost/.workspace/rupost-ai/doc/plans/2026-10-27-ai-master-roadmap.md)的效率准入公式（省下时间 > 上下文切换 + AI 等待 + 结果审查）与三形态判定法（就地 / 批量 / 认知）为滤网，系统盘点 [doc/ai/](file:///Users/zsyzzx/project/rust/rupost/doc/ai/) 与 [doc/debug/](file:///Users/zsyzzx/project/rust/rupost/doc/debug/) 全部文档中的潜在切入点
> **输入文档**: ai/feasibility_report（场景 12.1–12.7）、ai/development_roadmap、ai/technical_proposals、debug/production_debugging_features、debug/production_debug_mode_business_analysis、debug/production_debug_mode_assessment

---

## 1. 用户画像扩展（合并 debug 文档角色 × 既有画像）

| 画像 | 来源 | 高频场景 | AI 卡位 | 兜底级别 |
| :-- | :--- | :--- | :--- | :--- |
| **后端开发者（微服务）** | debug 商业分析场景 1（评分★★★★★） | 生产 500，测试环境无法重现；多服务日志人肉拼接（传统 2–4 小时） | E2 失败归因、E3 日志根因（远期） | B2 |
| **QA 工程师** | debug 商业分析场景 2（★★★★☆） | 上线后验证一致性；CI 大批失败需人工分组 | **E5 失败聚类 triage**、炼 bootstrap、E6 spec 影响分析 | B1/B2 |
| **前端开发者** | debug 商业分析场景 3（★★★☆☆） | 后端未就绪，需要真实感 mock | `ai gen` 的 mock 同源生成（**确定性为主，AI 仅增强示例值**） | B1 |
| **技术支持/现场工程师** | debug 商业分析场景 4（★★★★★） | 客户报障信息不全，需完整上下文重现 | replay（确定性已覆盖大头）；E1 快照 diff 归因辅助 | B1 |
| **实时应用开发者** | debug 商业分析场景 5（★★★☆☆） | WS 连接异常难重现 | E9 WS 帧序列归因（依托已交付的 A2 剧本素材） | B2 |
| 新手调试者 / 零门槛评估者 / 隐私内网用户 | [AI 迭代 PRD §2](file:///Users/zsyzzx/project/rust/rupost/.workspace/rupost-ai/doc/plans/2026-10-27-ai-context-assert-and-zero-config-prd.md) | （已分析，不重复） | TUI 内嵌补全、零配置链 | B2/B4 |
| 资产建设者 | 旅程设计画像 D | 调试成果沉淀 | 炼 bootstrap | B1 |

**画像结论**：debug 文档把目标人群从"写测试的人"扩展到"**排查问题的人**"——后者的痛点强度（小时级 vs 秒级）和付费意愿（商业分析 ★★★★★）都更高，而 AI 在"排查"侧的杠杆天然更大（归因、聚类、日志综合都是认知型工作）。这印证了效率修正的方向：AI 主战场应从"写断言"转向"**失败处理**"。

## 2. 补充使用场景（效率账）

| # | 场景 | 现状人工成本 | AI 形态 | AI 后成本 | 依赖基建 |
| :-- | :--- | :--- | :--- | :--- | :--- |
| SC1 | 微服务生产问题定位 | 人肉 grep 多服务日志 + 拼链路：**2–4 小时** | E3 认知 | 10–30 分钟（AI 汇总多源日志 + trace + 请求上下文给根因链） | 日志集成（未交付）→ 远期 |
| SC2 | 测试通、生产败的差异定位 | 肉眼比对两环境响应/配置：10–30 分钟 | E4 认知（就地于 compare 输出） | 1 分钟归因 | env compare（确定性先行） |
| SC3 | CI 一夜红了 50 条 | 逐条打开看，区分"同一根因"还是"N 个问题"：1–2 小时 | **E5 批量聚类**：AI 读 JSON 报告聚类失败，输出"3 个根因簇 + 各自代表用例" | 5 分钟，且修复按簇批量进行 | **无（JSON 报告已交付）** |
| SC4 | 断言失败后读懂原因 | 开 devtools 对比 actual/expected + 猜：10–20 分钟 | E2 认知（就地于 `t` 失败输出，AI 版 [Possible Causes]） | 30 秒根因假设 | **无（TestResult 上下文已交付）** |
| SC5 | 快照对比读懂 diff | 肉眼读 JSON diff 判断"哪些变化要紧"：10 分钟 | E1 认知（就地于 compare/replay --compare） | 30 秒语义归因（"token 字段消失 → 契约变更"） | snapshot compare（确定性先行） |
| SC6 | 不知道请求体格式（12.1/12.2） | 翻文档/问同事：5–15 分钟 | E7 就地（TUI 内 body 模版补全；本地 Trie + LLM 兜底） | 5 秒 | TUI（已交付） |
| SC7 | 边界用例设计（12.6.2） | 手工想空参/非法值矩阵：30 分钟/接口 | E8 批量（schema → 边界断言/用例变体，gen 管线延伸） | 10 秒生成 + 5 分钟审 | openapi parser（P2 基建） |
| SC8 | WS 会话异常排查 | 肉眼看帧序列找断点：20 分钟 | E9 认知（帧序列 diff 归因，A2 素材） | 1 分钟假设 | 无（A2 已交付） |

## 3. 新切入点总表（E 系列）

判定标准：效率公式过线 + 素材/基建就绪度。

| ID | 切入点 | 来源文档 | 形态 | 素材/基建 | 判定与优先级 |
| :-- | :--- | :--- | :-- | :--- | :--- |
| **E2** | **断言失败 AI 归因**（嵌入 `t` 失败输出；debug 文档的规则版 [Possible Causes] 的 AI 升级） | debug/features §6 | 认知+就地 | TestResult 全量上下文**已交付**，零新增基建 | ✅ 过线，**高优**（与 TUI 补全同为 P0 批次） |
| **E5** | **CI 失败聚类 triage**（读 JSON 报告，聚类根因簇） | 旅程 S6 延伸 | 批量认知 | JSON 报告**已交付** | ✅ 过线，**高优**（QA 画像最强痛点；一次调用替代小时级人工） |
| E1 | 快照对比 AI 差异归因（就地于 compare/replay --compare） | debug/features §3 | 认知+就地 | 依赖确定性 compare 先行 | 跟随（compare 落地后立项） |
| E4 | 环境差异归因（test 通/prod 败） | debug/features §7 | 认知+就地 | 依赖 env compare | 跟随 |
| E6 | spec 变更影响分析 | 本轮新增（ journey S6） | 批量认知 | 依赖 P2 openapi parser | 跟随 P2 |
| E7 | TUI body 模版补全（本地 Trie + LLM 兜底） | ai/feasibility 12.1/12.2 | 就地 | TUI 已交付 | ✅ 过线，与 TUI 断言补全同批 |
| E8 | 边界用例/断言生成 | ai/feasibility 12.6.2 | 批量 | gen 管线延伸 | ✅ 过线，作为 P2 增强项（非独立功能） |
| E9 | WS 帧序列归因 | debug 商业分析场景 5 | 认知 | A2 剧本素材已交付 | 低优候选（需求验证后） |
| E3 | 多源日志根因分析（SSH/K8s/ES/Loki + trace + 请求上下文） | debug/features §9–§12 | 认知 | **依赖日志集成整套基建（未交付）** | 远期——先有确定性日志集成，AI 才有素材；不因 AI 而提前建基建 |

**不过线（明确不跟）**：`ai explain` 作为独立命令（并 入 E2 就地形态）；依赖服务健康检查的 AI 化（debug 规则版 [Impact Analysis] 已足够，规则可确定性表达的不用 LLM）；通知/盯盘类（与 AI 无关）。

## 4. 关键洞察：debug 文档改写了 AI 的主战场

1. **"排查问题的人"比"写测试的人"痛点更深**（小时级 vs 秒级），而归因/聚类/日志综合恰好是 LLM 的认知强项——**AI 主战场从"写"转向"失败处理"**，与效率修正结论相互印证。
2. **debug 文档里已有三处"规则版智能"**（断言失败建议、日志 pattern 分析、依赖 Impact Analysis）——AI 版是它们的自然升级，而非新造场景；且都天然嵌入既有输出流（就地形态），无上下文切换税。
3. **素材依赖分层清晰**：E2/E5/E7/E9 零新增基建（素材已交付，可立即做）；E1/E4/E6 跟随确定性功能落地；E3 被日志基建挡住——**顺序不能倒置：先确定性基建，后 AI 增强**（与总体规划 B1 管线一致）。

## 5. 对总体规划的修订建议

1. §4 路线图"二期候选"扩充为：`ai explain` 并入 **E2（失败归因就地化）**；新增 **E5（CI 失败聚类）**、E7（TUI body 补全，与断言补全同批）。
2. E1/E4/E6 登记"跟随确定性基建"批次；E3 登记远期（日志基建之后）。
3. E8 并入 P2 的增强项，不独立立项。
