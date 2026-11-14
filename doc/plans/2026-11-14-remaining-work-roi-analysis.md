# 未执行项详析与投产比分析（Remaining Work ROI Analysis）

- **文档代号**: `remaining-work-roi`
- **日期**: 2026-11-14
- **状态**: 决策参考（随执行推进回收更新）
- **上游底账**: [`2026-11-12-replay-reuse-backlog.md`](./2026-11-12-replay-reuse-backlog.md)（22 项中 21 ✅、B-6◐、E-6 deferred）与 code review 修复（`3d9a9d2a`）
- **用途**: 对当前全部未执行内容做逐项详析、优劣势与投产比（ROI）评估，为排期决策提供依据

---

## 0. 背景与口径

截至本文撰写，插件体系与免重填主线的**核心功能与安全线已全部闭环**：M1–M7 全量落地、
replay-reuse 底账 22 项中 20 项完成（B-6◐、E-6 裁决 deferred）、code review P0×2/P1×10 全部修复、
四层门禁全绿（72 套件 + verify_features + run_all 含 rustls 后端外网 HTTPS 用例）。

本文盘点的是**尚未执行**的内容，按六个来源归类：
- **A 组**：裁决性 deferred（代码可做，卡在评审决策）
- **B 组**：登记"备忘"但未接线的代码缺口（小改动）
- **C 组**：IDE 生态最后一公里
- **D 组**：验证边界（需 CI/平台首跑）
- **E 组**：P2/生态期（06 §4.5 明确裁决不做——列出仅供未来推翻，**不建议排期**）
- **G 组**：流程收尾（推送/清理）

投产比评级口径：
- **工作量**: S（≤2h）/ M（半天–1 天）/ L（≥2 天）
- **收益**: H（直接改善用户可感行为或堵住真实风险）/ M（体验/完备性提升）/ L（边际）
- **ROI**: ★★★（立即做）/ ★★（值得排期）/ ★（维持现状或 deferred）

---

## 1. A 组：裁决性 deferred（代码可做，卡在评审决策）

### A1. B-6◐ error-Detail：断言失败条目补 error 明细

- **内容**：快照 `ExecutionSnapshotEntry.error` 目前仅在**无响应**（网络异常）时填充。
  本项将"有响应但断言失败"的条目也填充 `ExecutionErrorDetail`（kind=AssertionFailed、
  message、duration_ms、occurred_at），使 504/断言失败现场可追溯失败类别与耗时。
- **未执行原因**：02 §2 契约把 error 定义为"正常收到响应时为 None"——本项是**语义修改**
  （error 的取值域从"仅网络异常"扩展到"含断言失败"），按契约治理须评审批准，不是单纯字段追加。
- **涉及面**：`src/history/snapshot_io.rs`（error 判定分支）、`model.rs` 注释、
  `02` 契约文档裁决小节、`snapshot_adapter`/triage 消费确认、组合测试。
- **工作量**: M（评审 30min + 实现/测试 2–3h）
- **优势**:
  1. 504 现场复盘可读失败类别（当前只能从 assertions 反推）；
  2. AI triage 归因输入更完整（error.message 是归因特征之一）；
  3. error 与 success=false 的口径自洽（消除断言失败但 error=null 的认知摩擦）。
- **劣势/风险**:
  1. 下游消费者若假设 `error != null ⇔ 网络异常` 会误读——CHANGELOG 须显式登记语义演进；
  2. 旧快照（error=null 的断言失败条目）与新快照并存，消费者需按 masking/schema 无关的
     "容忍未知"原则处理（实际是"容忍缺失"，风险低）。
- **ROI**: ★★★（收益直接、成本低、风险可控——唯一门槛是一次显式评审签字）
- **建议**: 提交契约评审，批准后随下一批实施。

### A2. E-6 captured_keys：快照记录捕获变量键名

- **内容**：快照（文件级或条目级）记录本次运行捕获的变量**键名**清单（值永不落盘），
  使黑匣子可回答"捕获了什么字段"。
- **未执行原因**：两项裁决——①键名本身可能敏感（`api_key`、`sms_code` 类名外泄信息面）；
  ②需 options→presenter→save→write 四层透传，侵入面与收益不成比例（当时裁决）。
- **工作量**: S–M（代码 ~2h + 隐私评审）
- **优势**: 复盘证据链更完整；可支撑"断言建议生成"类未来功能。
- **劣势/风险**: 键名外泄面（可缓解：键名过 is_sensitive_key 过滤后再落）；透传侵入。
- **ROI**: ★（收益边际、有隐私争议）
- **建议**: 维持 deferred；若未来做"断言自动生成"再重启。

---

## 2. B 组：登记备忘但未接线的代码缺口

### B3. TUI/Web 交互入口的插件/门禁选项面透传

- **内容**：`build_interactive_executor()` 当前用 `PluginRuntimeOptions::default()`——
  `allow_missing_plugins` / `strict_plugins` / `plugin_timeout` / `plugin_env` / `strict_vars`
  在 TUI/Web **不可达**。本项增加 `rupost.toml [plugins]` 段（或 Web 请求参数）透传这些选项。
- **未执行原因**：批 1 落地时按最小正确性收口（默认选项已保证"本机装了就能跑、没装阻断+处方"），
  选项面透传登记为备忘。
- **涉及面**：`variable/config.rs`（新段解析）→ `executor_factory` 签名 → TUI/Web 两 handler → 测试。
- **工作量**: M（4–6h）
- **优势**:
  1. TUI/Web 里"必选插件缺失"从硬阻断变为可按项目配置降级（处方提示仍在）；
  2. 插件超时/环境变量放行对交互入口生效（SigV4 冷启动类插件可用性）；
  3. 与 CLI 旗标语义对齐，消除"交互入口是二等公民"的认知。
- **劣势/风险**:
  1. 配置面扩大（新 toml 段需 schema/文档同步）；
  2. Web API 若走请求参数透传需考虑多租户滥用（建议仅 toml 段）。
- **ROI**: ★★★（TUI/Web 插件日常使用的必需补齐；成本中等无风险）
- **建议**: 下一批实施（半天）。

### B5. Web 轨道 `--var-state` 持久化

- **内容**：R-4 的 Web 会话变量为内存态（服务重启即失）；本项把 `run_variables`
  接到 `--var-state` 式的落盘（opt-in）。
- **未执行原因**：设计取舍——Web 工作台是长驻 dev 服务，重启场景罕见；
  落盘扩大 token 的磁盘暴露面（与 S 组防线方向相反）。
- **工作量**: S（~2h，复用 `var_state.rs`）
- **优势**: 服务重启不丢登录态。
- **劣势/风险**: token 明文（或需脱敏）落盘路径 +1，与 history/快照防线的一致性要重审。
- **ROI**: ★（场景罕见 + 安全面扩大）
- **建议**: 维持现状（内存态）；若用户提出重启丢会话痛点再评估 opt-in。

### B6. verify_features.sh 补 Web 会话延续冒烟

- **内容**：R-4 的守护在 `tests/web_session_reuse_test.rs`；集成冒烟脚本
  `tests/verify_features.sh` 的步骤 15 未覆盖 Web 会话链路。
- **工作量**: S（~1h，复用既有 web 测试的 pty/oneshot 模式或直接子进程 + curl）
- **优势**: 三层校验的"集成冒烟层"覆盖 R-4（当前只有单测层）；防未来重构回归。
- **劣势/风险**: 无实质风险（脚本既有模式扩展）。
- **ROI**: ★★★（低成本补齐冒烟盲区）
- **建议**: 随下一批随手落地。

### B4. strict_vars 交互入口接线

- **内容**：同 B3——`strict_vars` 仅 `suite_setup.rs:102`（CLI 套件路径）接线；
  TUI/Web 随 B3 的选项面一并可达。
- **ROI**: 并入 B3，不单列。

---

## 3. C 组：IDE 生态最后一公里

### C7. VSCode 扩展本体

- **内容**：E-2 已产出四段 schema（pluginDirectives / directiveSyntax / builtinMacros /
  manifestSchema），但用户可感的 IDE 形态仍缺：①`.http/.md` TextMate 语法高亮；
  ②基于 schema 的 `@` 指令与宏补全 provider（含参数 snippet 模板）；③manifest JSON 校验挂接；
  ④扩展打包与发布（marketplace 或 vsix 分发）。
- **未执行原因**：06 §4.5 把"TUI/Web 补全"登记为 P2；E-2 交付的是**原料**（schema 可被
  任意编辑器消费），扩展本体是独立工程。
- **工作量**: L（语法 + 补全 provider + LSP 或 JSON schema 挂接 + 测试 + 打包，2–3 天起步，
  长期维护随 rupost 演进）
- **优势**:
  1. 画像 1（张晨）的核心诉求用户可感闭环——这是项目的"门面功能"；
  2. 拉新与演示价值高（IDE 体验是 API 工具的第一印象战场）。
- **劣势/风险**:
  1. 独立 codebase（TypeScript）引入新维护面与发布渠道责任；
  2. schema→补全的映射需随 rupost 指令集演进同步（建议 CI 校验 schema 时效）。
- **ROI**: ★★（高影响/中成本——建议独立立项，先做"JSON schema 挂接 + 基础语法"最小版）
- **建议**: 立项评估；最小版（schema 挂接 + 语法高亮）可先于完整补全 provider。

### C8. schema 挂接文档

- **内容**：README / `doc/guides/user_guide.md` 补"如何把 `rupost plugin schema` 的输出
  挂到编辑器"的最小示例（VSCode `json.schemas` association + manifest 校验一段）。
- **工作量**: S（~1h）
- **优势**: 1 小时让 schema 原料从"能导出"变成"能用"；C7 前的过渡方案。
- **劣势/风险**: 无。
- **ROI**: ★★★
- **建议**: 随下一批文档顺手落地。

---

## 4. D 组：验证边界（需 CI/平台首跑）

### D9. musl-static CI job 首跑

- **内容**：job 已就位（静态链接断言 + ≤20MB 体积断言 + cmake for aws-lc-rs），但未推送、
  无运行记录。aws-lc-sys 的 cmake/bindgen 首跑风险、体积实测值均待验证。
- **工作量**: S（推送即触发；首跑问题预计 0–2h 修复）
- **优势**: C-1 容器叙事从"工程配置"变为"已验证事实"；体积 DoD 实测值落袋。
- **劣势/风险**: aws-lc-rs musl 首跑可能暴露工具链问题（已有预案：cmake 已装，必要时
  切 `rustls-no-provider` + ring）。
- **ROI**: ★★★（已投入成本的收尾，必做）

### D10. Windows 平台验证

- **内容**：CI 矩阵已有 windows-latest 编译+测试 job；history 0600 的 Windows 分支是
  有意 no-op（ACL 兜底，CHANGELOG 已登记）。剩余的是：Windows 实机冒烟（shebang 插件
  不可用是已知边界，native exe 插件路径未实测）。
- **ROI**: ★（覆盖度边际——musl job 优先）

### G14. 提交推送与合并

- **内容**：8 个提交（`9370fcbf`→`3d9a9d2a` + 7 批全部）在本地 default 线，未推送/未合 main。
  D9 依赖本步。
- **工作量**: S（~30min 含 CI 观察首跑）
- **ROI**: ★★★（必做——所有已验证工作不推送等于未交付）

### G15. 工作区清理

- **内容**：`.workspace/replay-reuse-impl`（空提交）、`.workspace/plugin-closeout`（空 wip）
  回收（`jj workspace forget`）；`replay-reuse` bookmark 前移至最新。
- **工作量**: S（~15min）
- **ROI**: ★★（仓库卫生）

---

## 5. E 组：P2/生态期（有裁决背书，不建议排期——仅供未来推翻）

| 项 | 一句话理由 |
|---|---|
| `@pipeline` / `@use` 多行语法 | 阶段流 + depends-on 已覆盖主诉求，语法糖收益低 |
| PostResponse / OnFailure / Harvest 阶段 | 保留字已占位，触发场景（响应解密/失败归因插件）尚无真实需求输入 |
| `plugin sync --regenerate` | stat 指纹闭环已使缓存自动重建 |
| `plugin validate` / doctor | `plugin list` 三态 + 处方已覆盖验证诉求 |
| miners 路由化 / MCP Server | AI 命令族稳定后再排期（master roadmap） |
| TUI/Web 的 `@` 补全 | 并入 C7 扩展范畴 |
| `$md5` / `$random_string` / `$random_email` | 按需添加（每个 ~30min，出现真实需求时顺手做） |
| 正则断言 | `contains/matches` 语义边界需设计，无真实诉求输入 |
| RLIMIT 配额 | 5s 熔断 + 组级 kill 已兜底主要风险 |
| 全量 sha256 指纹 | 4KB+哈希双指纹已防投毒 |
| `fallback.builtin` 降级链 | 与"引擎绝不注入魔法值"裁决张力大，需强理由 |
| 在线市场 / 热重载 | 生态期 |
| Windows shebang 透明代理 | 跨平台 CI 批次 |
| `ttl` 缓存语义 | cache_scope 三值已覆盖 |

---

## 6. 汇总：投产比矩阵与推荐组合

| # | 项 | 工作量 | 收益 | ROI | 依赖 |
|---|---|---|---|---|---|
| G14 | 推送合并 | S | H（交付落地） | ★★★ | 无 |
| D9 | musl 首跑 | S | H（叙事验证） | ★★★ | G14 |
| C8 | schema 挂接文档 | S | M–H（IDE 立即可用） | ★★★ | 无 |
| B6 | verify 补 Web 会话冒烟 | S | M（冒烟盲区） | ★★★ | 无 |
| G15 | 工作区清理 | S | L（卫生） | ★★ | 无 |
| B3/B4 | TUI/Web 选项面透传 | M | H（交互一等公民） | ★★★ | 无 |
| A1 | error-Detail | M（含评审） | H（现场追溯） | ★★（评审门槛） | 契约评审 |
| C7 | VSCode 扩展 | L | H（门面） | ★★（独立立项） | 无 |
| A2 | E-6 captured_keys | S–M | L | ★ | 隐私评审 |
| B5 | Web var-state 落盘 | S | L | ★ | 用户痛点输入 |

### 推荐执行组合

1. **包 1（立即，~2h）**：G14 推送 → D9 观察 musl 首跑 → G15 清理 → C8 挂接文档 → B6 冒烟步骤。
   ——全部 S 级，收尾已验证的工作并把低垂果实摘完。
2. **包 2（下一批，~半天）**：B3/B4 TUI/Web 选项面透传（含 toml 段 schema/文档）。
   ——TUI/Web 插件日常使用的必需补齐。
3. **包 3（评审驱动）**：A1 error-Detail 提交契约评审，批准后半天实施。
4. **独立立项**：C7 VSCode 扩展（最小版先行）。
5. **维持现状**：A2/E-6（deferred）、B5（内存态）、E 组全部。

---

## 7. 一句话结论

**未执行的内容没有"必须马上做的功能缺口"**——核心与安全线已闭环，剩余是
1 个评审门槛项（A1）、2 个半天级补齐包（B3/B4 + C8/B6）、1 个独立立项（C7）、
以及推送收尾（G14/D9）。按上述组合推进即可在不扩大风险面的前提下清空全部低垂果实。
