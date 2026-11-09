# RuPost AI 总体规划（AI Master Roadmap）

> **创建日期**: 2026-10-27
> **定位**: AI 能力线的**单一事实源**——收拢定位哲学、架构不变量（含兜底机制分级）、阶段路线、命令族总览与治理规则。各专项（PRD/测试方案）以本文档为父纲。
> **上游**: [竞争复审 A1](file:///Users/zsyzzx/project/rust/rupost/doc/competition/2026-10-25-competitive-landscape-review.md)、[bootstrap 提案](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-26-ai-bootstrap-proposal.md)、[开发使用旅程](file:///Users/zsyzzx/project/rust/rupost/.workspace/rupost-ai/doc/plans/2026-10-27-ai-developer-journey-design.md)

---

## 1. 定位与哲学（三句话）

1. **AI 是增强，不是依赖**：任何 AI 功能在没有 AI（未配置/断网/模型故障）时必须仍然可用或优雅退出，绝不阻塞主工作流。
2. **创作期参与，执行期退场**：AI 只在写测试时帮忙；产物是普通 `.http`/`.md`，测试执行确定性、离线可复现。
3. **AI 建议，引擎裁决**：AI 输出永远经引擎 lint/解析校验后才落盘，非法建议丢弃并警告，绝不直接信任。

## 2. 架构不变量：兜底机制分级（本规划的宪法条款）

> **不变量**：删除整个 AI 层（不配置、不编译、不部署），rupost 的核心价值链 `解析 → 执行 → mock → 回归` 完整无损。

| 级别 | 兜底内容 | 机制 | 状态 |
| :-- | :--- | :--- | :--- |
| **B0 结构兜底** | 测试执行（`t`/`mock`/`replay`/TUI）**运行期零 AI** | 依赖图硬保证：引擎模块对 `rupost-ai` 零依赖；AI 调用只存在于 `ai` 命令族 | ✅ 已成立 |
| **B1 功能兜底（增强型命令）** | `ai gen` / `ai bootstrap` 在 AI 缺失时降级为**确定性基线**（OpenAPI 机械还原 / history 机械还原），输出黄色 `[!]` 警告（原因可区分），**exit 0** | 五阶段管线中的 B 阶段独立可用；降级矩阵逐原因覆盖 | gen：P2 已设计；bootstrap：提案已设计 |
| **B2 功能兜底（纯变换型命令）** | `ai assert` 无确定性基线可言 → 结构化错误 + 可执行指引（export Key / 用户级配置 / 本地 Ollama），**exit 1**，绝不 panic/挂起（客户端 30s 超时） | 错误路径测试锁定 | ✅ 已交付 |
| **B3 编译期可剥离** | crate 边界使 AI 可整体 feature-gate 或移除而不触碰引擎代码 | `crates/rupost-ai` 不依赖 rupost 内部类型 | ✅ 结构就绪；**当前不启用编译期 gate**（reqwest 已共享、体积论据弱、运行期降级已足够）——若未来发布"无 AI 发行版"再启用，登记 |
| **B4 配置兜底链** | 环境变量 > `~/.rupost/ai.toml`（无 Key）> 本地 Ollama 探测 > 优雅报错 | P0 PRD 已立项 | 已立项 |

**判定规则（新增 AI 功能的准入检查）**：每个 AI 功能立项时必须声明属于 B1 还是 B2，并在其测试方案中包含对应降级用例（无 Key / AI 故障 / 输出非法三种至少各一）。无降级用例 = 测试方案不通过。

## 3. 哲学的算法化：五阶段管线模板

所有 B1 型（增强型）AI 功能复用同一条管线，AI 只占 C 阶段一环：

```
A 素材采集（确定性：spec 文件 / history / 快照）
B 机械还原（确定性基线，独立可用——B1 兜底的实体）
C AI 语义增强（可失败、可跳过、可替换：云 API / Ollama / mock）
D 引擎裁决（lint / parser / MockCompiler 校验，非法丢弃 + 警告）
E 回归资产（普通 .http/.md，Git 可审，离线可跑）
```

已完成验证：`ai bootstrap`（提案）与 `ai gen`（P2 计划）均按此模板设计；后续新增强型功能直接套用，不再重新发明降级语义。

## 4. 阶段路线图

| 阶段 | 交付 | 兜底级别 | 旅程卡位 | 状态 |
| :-- | :--- | :-- | :--- | :--- |
| **Phase 0** | `ai assert` 接线 + 离线 dogfooding + crate 激活 | B2 | S4 门槛 | ✅ 已交付（2026-10-27） |
| **P0** | `--last` 上下文断言 + 脱敏 + 零配置探测链 | B2 + B4 | S0/S4 | PRD/测试方案已立项 |
| **Phase 1** | `ai bootstrap --from history`（历史 → 语义增强回归资产） | B1 | S4 跃迁（最高竞争价值） | 提案已立项，下一主线 |
| **P2** | `ai gen --spec`（OpenAPI 导入 + AI 生成 mock/断言） | B1 | S1 冷启动 | 开发/测试计划已立项 |
| **二期候选** | **E2 失败归因就地化**（嵌入 `t` 失败输出）；**E5 CI 失败聚类 triage**；E7 TUI 内嵌补全（断言 + body 模版）；`--response` 上下文 | B2 | S3/S6 留存 | 候选（E 系列筛选见[切入点补充分析](file:///Users/zsyzzx/project/rust/rupost/.workspace/rupost-ai/doc/plans/2026-10-27-ai-entrypoints-supplement.md)） |
| **跟随批次** | E1 快照 diff 归因、E4 环境差异归因（随确定性 compare）；E6 spec 影响分析（随 P2 parser）；E8 边界用例（并入 P2 增强） | B1/B2 | — | 登记，随依赖基建分期 |
| **中期** | MCP Server（暴露 `run_request`/`get_history`/`mock` 为 Tool）；E3 多源日志根因分析（随日志基建） | —（B0 不变） | 生态位（"AI 的网络手脚"） | 中期，待 ai 命令族稳定 |
| **远期** | L4 Spec→用例矩阵深化；YAML 规格支持（依赖评审后引入维护中 fork） | B1 | S1 增强 | 远期登记 |

**优先级裁决规则**（来自[旅程设计](file:///Users/zsyzzx/project/rust/rupost/.workspace/rupost-ai/doc/plans/2026-10-27-ai-developer-journey-design.md)）：消除旅程断点越早的功能越优先；S4（资产化）> S1（冷启动）> S3/S6（留存）。
**执行顺序（2026-10-27 裁决）**：以 [MVP 整合计划](file:///Users/zsyzzx/project/rust/rupost/.workspace/rupost-ai/doc/plans/2026-10-27-ai-mvp-consolidation-plan.md) 为准——M1 地基与即时效率（P0 + import curl + E2）→ M2 批量与认知（E5 + bootstrap）→ M3 供给侧（MCP MVP）→ fast-follow（`@ai-assert` resolve、OpenAPI 确定性导入；P2 AI 增强部分**停泊待验证**）。

## 5. 命令族总览与兜底行为

| 命令 | 变换方向 | AI 缺失时的行为 | 状态 |
| :-- | :--- | :--- | :--- |
| `ai assert "<nl>"` | 自然语言 → `@assert` 行 | exit 1 + 三选一可执行指引 | ✅ 已交付 |
| `ai gen --spec <oas.json>` | OpenAPI → 活文档（mock + 断言） | 机械基线 + `[!]` + exit 0 | 已立项 |
| `ai bootstrap --from history` | 历史/快照 → 回归测试文件 | 机械还原 + `[!]` + exit 0 | 已立项 |
| `ai explain`（候选） | 请求/响应/错误 → 归因解释 | exit 1 + 指引（纯变换型，无基线） | 候选 |
| `t` / `mock` / `replay` / `h` / TUI | — | **永远零 AI，与 AI 层无关** | ✅ B0 |

配置面统一：`RUPOST_AI_API_KEY`（或 `OPENAI_API_KEY`）> `RUPOST_AI_BASE_URL` / `RUPOST_AI_MODEL` / `~/.rupost/ai.toml` > 本地探测；密钥永不落盘、永不入日志；所有 AI 命令共享 P0 落地的 `src/ai/{config,context,redactor}`。

## 6. 治理规则

1. **准入**：新 AI 功能立项必须声明 B1/B2 级别 + 旅程断点映射，二者缺一不立项。
2. **测试**：降级用例（无 Key/AI 故障/输出非法）是测试方案的强制组成（P2 测试方案 §3.3/§4 已示范）。
3. **文档**：每个 AI 命令的 README 必须包含"无 AI 时会发生什么"一节。
4. **裁剪**：反模式清单（聊天面板/运行期 AI/code-first 文档流水线/评分体系/云端同步）继续有效，见旅程设计 §5。
5. **变更**：修改本规划的兜底级别定义需视为架构决策，走独立文档评审。

## 7. 阶段成功指标（汇总各专项）

| 阶段 | 机器可验指标 |
| :-- | :--- |
| Phase 0 | ✅ workspace 测试 ≥ 7 全绿；`grep use rupost_ai` 非空；verify 步骤 7 PASS（已达成） |
| P0 | 零配置 Ollama 机器首跑即成功；脱敏矩阵全绿 |
| Phase 1 | 生成物 `rupost t` 断网全绿；mock AI/真实 API/Ollama 三后端产物均有效；`--no-mask` 醒目提示 |
| P2 | 自足闭环（gen → mock → t 全绿）；降级基线同样闭环全绿；防覆盖 |
| 全局 | `cargo test --workspace` 全绿；每个 AI 功能的降级用例存在于测试方案 |

---

**一句话**：AI 线的全部规划服务于一个可证伪的承诺——**拔掉 AI，rupost 依然是完整的回归引擎；插上 AI，调试成果和存量规格变成确定性的回归资产。**
