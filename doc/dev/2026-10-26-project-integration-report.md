# RuPost 全文档整合审计与项目知识报告 (2026-10-26)

> **范围**: `doc/` 全部 98 个 Markdown 文件（14 个分区、约 24,800 行）+ 根目录 README / CHANGELOG / checkpoint / AGENTS + examples/README
> **方法**: 三个独立审计子代理分区深读（plans+dev / architecture+debug+apidoc+folder_execution+ai+assertion+cookie+script / guides+competition+tui+plugin+根文档），关键争议结论由主审在代码与真实二进制上二次实证
> **性质**: 知识整合与一致性审计报告（快照日期 2026-10-26）；进度事实仍以 [progress_summary.md](file:///Users/zsyzzx/project/rust/rupost/doc/plans/progress_summary.md) 为唯一来源

---

## 1. 执行摘要

**一句话结论**：RuPost 的文档生态呈清晰的**三层沉积结构**——2026-02 设想层、2026-06/07 交付高峰层、2026-10 治理层——层间缺乏状态同步机制，导致约 **40 处**矛盾/过时表述；但**对外宣称基本诚实**（用户可见能力 17/20 与代码完全一致），**对内漂移是主要风险**。

| 维度 | 结论 |
| :--- | :--- |
| 文档总量 | 98 文件 / 14 分区 / ~24.8k 行；现行有效约 **31 份**，历史归档约 **40 份**，未实现设想约 **27 份** |
| 最严重矛盾 | `doc/plugin/feasibility_report.md` Phase 1 六项勾选 `[x]`（含"2 个示例插件、覆盖率>80%"）vs **src/ 零 plugin 代码、Cargo.toml 零 wasmtime 依赖** |
| 最大战略-代码断点 | `crates/rupost-ai`：竞争复审定性的"最高杠杆激活项"，实际是**无引用方死代码**，且与 `src/http/llm_adapter.rs` 功能重叠（双实现悬空） |
| 文档健康度最佳实践 | `doc/competition/competitor-analysis-and-differentiation.md` 的**自我降级横幅**——全库唯一显式声明被新文档取代的旧文档，建议推广为制度 |
| 已排期待修 | AGENTS.md §5 CLAP 规则（实证过时）、`history show` help 注释（与实现相反）——均已绑定 P0 测试提交序列，未执行 |

---

## 2. 项目知识全景

### 2.1 定位演进链（从文档堆中还原的产品观）

```
原始大纲 (todo.md, 9 条需求)
  → Phase 1-4 计划 (project_plan.md)
  → 产品战略 (rupost_product_strategy.md, 2026-05: "AI 增强的 API 测试平台", @ai_assert/@bench 愿景)
  → 实际演进偏离：WS/SSE/Mock/快照/TUI 先行 (2026-06~07)
  → 质量加固 + 测试治理 (2026-10)
  → 定位收敛 (2026-10 竞争复审): "面向实时 AI API 时代的终端回归引擎"
     —— 不做终端 Postman（红海），不做 AI 测试生成平台（Keploy 占位），
        做 AI 时代接口回归的终端标准件（A1-A5 竞争点全部已兑现）
```

### 2.2 交付能力全景（用户可见宣称 × 代码核验）

以下 17 项能力经审计逐项核验**与代码一致**：`.http`/`.md` 双格式解析（Markdown 活文档）、8 运算符断言、`@capture` + 四层级变量、动态变量 `$uuid/$timestamp/$random_int`、目录递归 DAG（Kahn/环检测/状态克隆）、网络诊断（DNS/TCP/TLS/TTFB + X.509）、Mock 服务器（Trie/变体/快照转译/`.md` 编译）、SSE 剧本（`stream.llm.content`/`@stream_to`）、WebSocket 剧本（软匹配/重连/Cookie 继承）、历史与 `generate`、别名与 curl/httpie 内联、TUI 全套（双 Tab/`/`过滤/`,`主题/pP 路径/预折行/鼠标/E 外编/n 快建）、Cookie 三模式 + 环境隔离（`cookies_dev.json`，`t` 路径经 `runner/app.rs:74` 同样生效——子代理误报已纠正）、`init` 三模板。

夸大/过时的 3 处宣称：Mock"热加载"（无 watch 实现）、`sse_testing.md` 的 `rupost mock --port 8080`（file 为必填，命令必失败）、`examples/README.md` 清单止于 2026-06（漏 8+ 新示例集）。

### 2.3 功能演进时间线（20 个里程碑，自 plans 分区还原）

| 时期 | 里程碑 |
| :--- | :--- |
| 2026-01 前 | Stage 2-4：`.http` 解析 / 批量执行 MVP（33 测试）；dev 分区快照冻结（01-22） |
| 2026-02 | **设想层爆发**：插件系统（WASM 七篇）、AI 三方案（嵌入/Sidecar/MCP）、动态脚本（Rhai）、生产调试评估、apidoc OpenAPI、TUI 施工计划——**均未实施** |
| 2026-04~05 | AI 蒸馏 PRD、离线知识库设想（未做）；产品战略定稿 |
| 2026-06 | 交付高峰：生产调试阶段 1/2（.env 级联 + diagnose）→ Sprint 1（UA/Cookie 管道）→ Sprint 2（多文件 DAG + 并行 + JSON 报告）→ Markdown 生命周期 + Mock → Sprint 4（LLM 流式 + `init`）→ 启发式拆分 → 生产调试阶段 7（Mock 服务器） |
| 2026-06~07 | Sprint 5（WebSocket）；架构重构（Parser 解耦 / CLI/URL 组件化 / 下沉 reporter·scanner·url）；快照重放 MVP + `replay` 命令；Sprint 7（TUI 引导/自适应/鼠标） |
| 2026-07 后半 | 历史存储优化设计（未做）；Pending_Features 全量同步（07-19）；TUI 性能迭代（预折行/双 Tab/历史反向还原） |
| 2026-10 | 质量加固战役 Phase 0-4（25 项闭环，318→468 测试）；TUI v3.3（安全三防线/端口化/组件拆分/`/`过滤/主题槽位化）；**治理层三件套**：测试方案（468 基线 + 19 域矩阵）、P0 执行方案（待执行）、竞争复审（定位收敛） |

### 2.4 架构现状锚点

Clean Architecture 三层（Core / UseCase / Adapter）+ MVU TUI，现行导航以 [directory_structure.md](file:///Users/zsyzzx/project/rust/rupost/doc/architecture/directory_structure.md) 为准（其自身 2 处小错见 §4-P2）。19 功能域 × 测试覆盖矩阵见[测试方案 §4](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-25-test-strategy-plan.md)。

---

## 3. 文档地图与健康度

### 3.1 分区健康度表

| 分区 | 文件数 | 现行 | 归档 | 设想/矛盾 | 定位与判语 |
| :--- | :-- | :-- | :-- | :-- | :--- |
| `plans/` | 25 | 8 | 11 | 6 | **项目主记忆**。progress_summary（唯一事实源）+ 2026-10 三件套高度同步；旧 PRD 交付物路径有漂移 |
| `dev/` | 14 | 4 | 9 | 1 | **化石层**。仅 development_workflow / Pending_Features / github_actions / architecture_and_extension（部分）仍有效 |
| `architecture/` | 8 | 1 | 4 | 3 | 仅 directory_structure 现行；master_plan 的 `src/core`、`src/services` 从未存在 |
| `guides/` | 10 | 9 | 1 | — | **健康度最高**（面向用户，与代码同步好） |
| `competition/` | 6 | 1 | 4 | 1 | 2026-10 复审为权威；底稿已自降级（标杆实践） |
| `tui/` | 6 | 2 | 2 | 2 | keybindings（带状态标注）现行；technical_architecture 链接指向外部 worktree 已失效 |
| `cookie/` | 7 | 1 | 5 | 1 | cookie_test_specification 现行；元数据解析三篇是被采纳/否决的决策过程记录 |
| `plugin/` | 7 | 0 | 0 | 7 | **全分区悬空**：2026-02 预研包，无搁置声明，README 目录树含不存在的文件 |
| `ai/` | 4 | 0 | 0 | 4 | 方案 A/B/C 均未按文实施；实际走了第四条路（rupost-ai crate + llm_adapter），且 crate 未接线 |
| `debug/` | 3 | 0 | 1 | 2 | 14 项功能中 4.5 项落地；大量命令面（logs/trace/health/record）不存在 |
| `apidoc/` | 4 | 1 | 2 | 1 | phase2 已 100% 落地（路径漂移）；OpenAPI 文档生成零落地 |
| `folder_execution/` | 2 | 1 | 0 | 1 | 主体落地；`@loop`/`@skip-if`/HTML 报告未做 |
| `script/` | 1 | 0 | 0 | 1 | Rhai 选型方案，零落地（与 Sprint 3 PRD、master_plan P2.5 同属"脚本三胞胎"设想） |
| 根 + examples | 5 | 3 | 0 | 2 | README/CHANGELOG/checkpoint 现行；examples/README 过时 |

### 3.2 现行有效文档核心集（"读懂项目只需这 9 份"）

1. [progress_summary.md](file:///Users/zsyzzx/project/rust/rupost/doc/plans/progress_summary.md) — 进度唯一事实源
2. [directory_structure.md](file:///Users/zsyzzx/project/rust/rupost/doc/architecture/directory_structure.md) — 代码导航
3. [development_workflow.md](file:///Users/zsyzzx/project/rust/rupost/doc/dev/development_workflow.md) — 7-Stage 研发规范
4. [test-strategy-plan.md](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-25-test-strategy-plan.md) — 测试父纲（19 域矩阵）
5. [competitive-landscape-review.md](file:///Users/zsyzzx/project/rust/rupost/doc/competition/2026-10-25-competitive-landscape-review.md) — 战略定位
6. [user_guide.md](file:///Users/zsyzzx/project/rust/rupost/doc/guides/user_guide.md) + 各专题 guides — 用户能力面
7. [keybindings.md](file:///Users/zsyzzx/project/rust/rupost/doc/tui/keybindings.md) — TUI 按键（带实现状态）
8. [Pending_Features.md](file:///Users/zsyzzx/project/rust/rupost/doc/dev/Pending_Features.md) — 未实现清单
9. [CHANGELOG.md](file:///Users/zsyzzx/project/rust/rupost/CHANGELOG.md) + [checkpoint.md](file:///Users/zsyzzx/project/rust/rupost/checkpoint.md) — 变更与节点

### 3.3 悬空设想清单（27 份，按沉没文档投资排序）

| 设想域 | 文档体量 | 现状断点 |
| :--- | :--- | :--- |
| 插件系统（WASM） | 7 篇 ~3.0k 行 | 零代码；且与 master_plan 的"外部脚本 MVP"路线**互相冲突**；竞争复审未提 |
| AI 方案包（嵌入/Sidecar/MCP） | 4 篇 | 实际走第四条路且 crate 未接线（见 §5.1） |
| 生产调试阶段 4/5/6/8 | 3 篇 ~3.3k 行 | 脱敏加密/凭证注入/日志聚合/WS 时序回放均未做 |
| 脚本引擎"三胞胎"（script + Sprint3 PRD + master_plan P2.5） | 3 处 | Rhai 选型完成，零落地 |
| OpenAPI 文档生成 | 2 篇 | 零落地 |
| 快照重放 roadmap 五项 | 1 篇 | 掩码/智能 diff/虚拟响应/事后 export（即现在的桩）/加密均未做 |
| 历史存储优化 | 1 篇 | HistoryConfig 未实现，与现行 20MB/10000 硬编码双文档并存 |
| 离线知识库 / AI 蒸馏 | 2 篇 | 零落地 |
| 契约蓝图（@extends-spec/# @spec-/SSE·WS Schema） | 2 篇 | 零落地 |
| Sprint 6 HTML 报告、Postman 导入、@loop/@skip-if、JUnit | 散布 | 老牌待办，多清单重复登记 |

---

## 4. 一致性审计发现（整合去重，按误导风险分级）

### P0 —— 会直接误导读者/决策（4 处）

1. **`doc/plugin/feasibility_report.md` Phase 1 六项勾选 `[x]`**（WASM 集成/本地加载/权限检查/2 示例插件/覆盖率>80%）vs 零代码零依赖；整个 `doc/plugin/` 无搁置声明，README 还列出不存在的 security.md/lifecycle.md/examples/。
2. **`doc/dev/progress_summary.md` 三重自相矛盾**：头部"最后更新 2026-01-22 归档" vs 正文含 2026-07 Refactor/Sprint 5/7/TUI 段落（归档后仍被编辑）；"断言系统已完成" vs 文末"断言验证系统←下一个目标"；"240+ 测试" vs 现状 468。
3. **`doc/tui/usage_and_testing.md` §4** 宣称编辑器"任意键盘字符输入 + Ctrl+S 保存" vs 实际只读（与同目录 keybindings.md 互相矛盾）。
4. **`doc/dev/QUICK_REFERENCE.md`**：列出不存在的 `parser/converter.rs`、`runner/reporter.rs`；"33/33 测试"、"断言/变量/Markdown 未实现"全部与现实相反。

### P1 —— 事实性错误或已排期未执行（9 处）

5. AGENTS.md §5 CLAP 规则（实证：子命令模式双序皆可，真实陷阱在内联模式）——已绑定 P0 测试提交 4。
6. `history show` help "1 is the oldest" vs 实现 1=最新——已绑定 P0 测试提交 3。
7. `sse_testing.md` 场景 1 的 `rupost mock --port 8080`（缺必填 file，必失败）。
8. `mock_server.md` "热加载"宣称（无 watch/notify）。
9. `2026-10-18-quality-hardening-execution-plan.md` 头部"Phase 4 待执行、基线 412"未回写（实际 Phase 0-4 全闭环、468）。
10. `file-format-spec.md`：@timeout 写"毫秒"（实际支持 `5s/1000ms`）、"暂不支持变量捕获"（已实现）。
11. `production_debug_development_plan.md` 命令形态 `rupost history replay` vs 实际顶层 `rupost replay`；debug 文档群大量不存在命令面（record/compare/logs/env use/`--compare-with`）。
12. 路径漂移群：apidoc/phase2 与 assertion-data-flow 引用 `src/runner/reporter.rs`（实际 `src/reporter.rs`）等约 6 处；`plans/progress_summary.md` Sprint 2 行内 `src/runner/scanner.rs` 旧行未随"下沉"行更新。
13. `rupost_product_strategy.md` 超前宣称（@ai_assert/@ai_fix/@bench）无代码对应。

### P2 —— 链接失效与陈旧瑕疵（约 12 处）

`tui/technical_architecture.md` 全部代码链接指向 `.gemini/antigravity/worktrees/` 外部目录；README 等 `file:///` 绝对路径跨机失效；keybindings.md 尾部 `github.com/yourname/rupost` 占位符；plugin_interface.md 文末残留 AI 对话；`directory_structure.md` 引用已删除的 `metadata_types.rs`（质量加固清理的死代码）、scanner.rs 归属章节错位、`@ws-send` 描述失实（实际 SEND/EXPECT DSL）；examples/README 清单过时；CHANGELOG 仅 Unreleased 无版本号；cookie 元数据文档停留在 5 种（实际 15 种）；`优化.md` 将已实现的 TraceHeader/Cookie 标为"P3 预留"。

### 审计方法勘误（体现报告可信度）

子代理审计有 2 处误报，经主审在代码与真实二进制上实证**推翻**：① "`--debug`/`--debug-on-failure` 不存在"——实为全局参数（cli.rs:31/35，二进制实测生效）；② "cookie 环境隔离未实现、--cookie-file 原样使用"——`resolve_cookie_path` 在 `runner/app.rs:74`（`t` 命令路径）与 `cli.rs:270`（内联路径）双接线生效。

---

## 5. 战略断点与行动整合

### 5.1 最大战略-代码断点：rupost-ai 的"双实现悬空"

三路证据交叉指向同一事实：① 竞争复审将 A1（LLM 流式闭环）列为最高杠杆激活项；② ai 分区审计发现方案 A/B/C 均未按文实施，实际走的是第四条路；③ 架构审计发现 `crates/rupost-ai` 无引用方（主包 Cargo.toml 无此依赖），其 translator 与 `src/http/llm_adapter.rs` 的 OpenAI/Anthropic 帧规整**功能重叠、双实现并存**。激活 A1 前必须先做架构决策：**合并**（llm_adapter 吸收 crate 或反向）还是**接线**（主包依赖 crate 并删除重叠代码）。这是 2026-10-26 之后的第一优先级架构决策。

### 5.2 行动清单（整合三份 2026-10 文档 + 本次审计增量）

| 优先级 | 行动 | 来源 | 状态 |
| :--- | :--- | :--- | :--- |
| P0 | 13 个 P0 测试落地（4 个 jj 提交，含 AGENTS §5 与 show help 两处修正） | 测试执行方案 | 待执行 |
| **P0（新增）** | rupost-ai 架构决策：合并 or 接线 | 本次审计 §5.1 | **已决策**：方案 A"接线 + 职责文档化"，且设计期逐行核实纠正了"双实现"定性（crate 资产为 AssertionTranslator 创作辅助，与 llm_adapter 流式职责不同层）——见 [AI 激活 PRD](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-26-ai-activation-prd.md) |
| P1（新增） | 文档治理批次：dev/progress_summary 与 QUICK_REFERENCE 加归档横幅、plugin 分区加搁置声明（含路线冲突说明）、usage_and_testing §4 对齐只读现实、sse_testing 命令修正、mock 热加载措辞降级、quality-hardening 头部回写 | 本次审计 §4 | 建议一个原子提交批处理 |
| P1 | 测试方案 P1 清单（8 测试：SSE 上浮/capture 专项/JSON schema 契约等） | 测试方案 §6 | 排期中 |
| P2（新增） | "自我降级横幅"制度化：所有设想类文档头部强制状态声明（效仿 competitor-analysis 底稿模式）；链接修复批（外部 worktree/file://绝对路径/占位符） | 本次审计 §4-P2 | 治理建议 |
| P2 | future_backlog 已登记项（gRPC Won't do、HTTP/3、generate AI 延伸） | 竞争复审 §4 | 已登记 |

### 5.3 知识治理建议（防止三层沉积再发散）

1. **现行文档核心集制度**（§3.2 的 9 份）之外的一切文档默认"历史"，新增文档进入核心集需在 progress_summary 登记。
2. **设想文档生命周期**：立项即挂 `状态：设想中` 横幅；被取代/搁置时更新横幅而非删除（保留决策史）；超过两个季度未启动的设想移入 future_backlog 一行条目 + 原文归档。
3. **PRD 路径保鲜**：交付物路径漂移是最高频矛盾（本次发现 8+ 处）——progress_summary 交付物列建议写"模块域"而非具体文件路径（如 `src/runner/engine/` 而非逐文件）。
4. **单一进度源纪律重申**：dev/ 旧快照类文档（progress_summary/QUICK_REFERENCE/todo_prioritized）建议物理移入 `doc/dev/archive/` 或加横幅，消除同名双 progress_summary 的误读风险。

---

## 6. 附录：审计覆盖与证据方式

- 分区审计 A（plans+dev，39 文件）、B（architecture/debug/apidoc/folder_execution/ai/assertion/cookie/script，30 文件）、C（guides/competition/tui/plugin+根文档，29 文件）；争议结论二次实证（grep 代码 + 真实二进制运行）。
- 能力宣称核验基线：`src/cli.rs`（子命令/全局参数全集）、`src/assertion/parser.rs`（8 运算符）、`src/variable/resolver.rs:20-22`（动态变量）、`src/tui/handlers/`（按键）、`src/runner/app.rs:74`（cookie 环境隔离）、`src/parser/metadata.rs`（15 种元数据）、`tests/`（468 基线）。
- 本报告不修改任何被审计文档（纯审计快照）；§5.2 行动清单中"新增"项待确认后排期。
