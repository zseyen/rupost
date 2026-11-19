# 已执行方案结论存档（Executed Plans Digest）

> **定位**：本文件是 2026-11-19 文档治理的**萃取存档**——被删除的已完成执行文档（PRD/实施计划/评估报告/化石层规划件，约 130 份）中仍具长期价值的**架构决策、需求要点、规范纪律、经验教训与未落地设想**在此沉淀；原文一律以 git 历史为准（本文撰写前的任意提交均可找回）。
> **不承载**：进度（→ [progress_summary.md](../plans/progress_summary.md)）、未排期待办（→ [future_backlog.md](../plans/future_backlog.md)）、现行架构事实（→ [directory_structure.md](directory_structure.md) 与 [features/](features/)）、审查跟踪（→ [architecture_review_2026-11.md](architecture_review_2026-11.md)）。

---

## 1. 产品定位演进链（战略结论）

```
原始大纲（todo.md 9 条）→ Phase 1-4 计划（project_plan.md）
  → 产品战略 2026-05（"AI 增强的 API 测试平台"，@ai_assert/@bench 愿景——超前宣称无代码对应）
  → 实际演进偏离：WS/SSE/Mock/快照/TUI 先行（2026-06~07）
  → 质量加固 + 测试治理（2026-10）
  → 定位收敛 2026-10（竞争复审）：终端回归引擎
  → 定位定稿 2026-11-13（差异化报告）：
     「面向代码库的、本地优先的 API 活文档验证与故障复现工具」
```

**差异化报告核心结论**（2026-11-13，最高价值战略文档）：
- 不做"又一个 Postman"，不做"Rust 版 Hurl"；文档是契约，失败是可移交的工件
- 三支柱：①活文档验收（PRD 同时审文案与断言）②复现包（失败 → 脱敏/可重放/可归因工件）③本地诊断（无云依赖，AI 仅是受脱敏/配额/确定性基线约束的增强层）
- Phase 0 会话隔离 ✓ / Phase 1 复现包 ✓ / Phase 2 契约漂移门禁 ✓（WP-P2b/P2c）/ Phase 3 互操作 → **移交社区**（OpenAPI 导入、k6 桥接——设计骨架见 git 历史 `doc/plans/2026-11-13-phase01-remediation-plan.md` WP-P3a/P3b）
- **明确不做**（纪律清单）：账号/云同步/监控 SaaS、GUI collection 对等、核心链 AI 决策、自建压测引擎、无用户需求的协议/插件面扩张
- 成功指标基线：失败→首次可重放 <2min、复现包干净环境成功率 ≥90%、工件敏感信息泄漏 0、无 AI 配置核心工作流可用性 100%

**优先级战略遗产**（feature_prioritization 2026-05，已被上表取代但方法论可复用）：Cookie=生死线 P0 / AI=爆点 P1 / TUI=体验承载 P1.5 / 生产调试=利器 P2 / 脚本=护城河 P2.5 / 插件=最后（"过早平台化陷阱：没有用户量没人写插件"）。实际演进验证了其中 Cookie 先行与插件延后两条。

## 2. 领域设计结论（仍有效的决策与规范）

### 2.1 脱敏判定引擎（redaction，2026-11-10 PRD+实施方案，已落地）
- **判定优先级终态（SSOT）**：`@sensitive` > 配置 `extra_keys` > 内置硬键 EXACT（authorization/cookie 等 8 键，**永远不可豁免**）> 豁免检查（`@no-redact`/`ignore_keys` 只作用于 SUBSTRINGS 模糊层）> 内置 SUBSTRINGS > 不敏感
- **规范化纪律**：装配期与查询期统一 `trim().to_ascii_lowercase()`（RFC 7230 大小写不敏感）
- **五盲区经验**：①脱敏面必须 Header/URL Query/Body JSON 三出口同策略 ②审计收集与纯计算解耦（并行安全）③硬键护栏防穿透（对硬键的 ignore 声明 → 告警 + 强制敏感）④多值标注解析（逗号/空白混合切分、去重）⑤模板化变量名卫生清洗（`-`/`.` → `_`）
- **零分配热点**：内置词表编译期预排序切片 + 二分查找；自定义词表预分配 HashSet
- **工业界标杆吸收**：Sentry 的 EXACT+SUBSTRING 二元组与 safe_fields 豁免哲学、Bruno 的活文档就地声明、GH Actions 的"过度脱敏事故"反面教训（坚持键形态优先）
- **二期登记（未实施）**：R7 Sensitivity 枚举分级 / R8 `--redact safe|typed|shape` 档位（typed 携带键类、shape 保长——须文档化前缀泄露与同值关联风险）/ R9 值形态识别（JWT/hex-32 形状正则，generate 侧禁用）

### 2.2 重放与免重填（replay-reuse 底账 2026-11-12，22 项中 21 落地）
- **主题命题**：重放 = 信息重放，比对只是副产品；用户要的是"登录态/捕获 token/参数直接复用"
- **四轨道模型**：cookie 文件（跨运行✓）/ capture 变量（进程内）/ history 字节（免登录重放）/ 快照字节——`--with-deps` 闭包执行 + `--var-state` 落盘补齐了跨运行断点
- **未落地**：E-6 captured_keys 入快照（deferred：键名本身可能敏感 + 四层透传侵入；重启条件 = "断言自动生成"类功能立项）；方向性登记：统一"会话资产"抽象（token/cookie/变量单一 session store，R 系落地前不做）、重放 `--target` 同源警告、黄金样本变更检测实践规约

### 2.3 插件生态与 AST 桥（readiness assessment 2026-11-13）
- **基座满足度矩阵**：运行时基座（协议稳定/manifest schema/命令通道/TOFU）≈80% 满足；生态基座缺口 C4（import/export 管线无扩展点）/C6（无安装命令）/C7（命令型示例缺，后补 cmd-hello）/C8（作者契约测试工具缺）
- **形态裁决**：OpenAPI 导入/k6 导出**移交社区插件形态**（2026-11-13 范围裁决，核心不实施）；k6 不适合纯第三方插件承载（解析语义分叉损害"文档即契约"）——AST 桥解除该障碍
- **AST 桥契约要点**（`rupost ast` → AstDocument v1.0，已落地）：三版本轴互不相干（manifest_version / hook protocol_version / AstDocument schema_version）；命令通道无协议版本是设计而非缺陷（`RUPOST_BIN` 注入 = 单一二进制真相源，零版本漂移）；核心承诺 1.x 字段只增不改/不删/语义稳定；**作者六守则**：永远用 RUPOST_BIN、首步校验 schema 前缀、白名单取用容忍未知字段、缺席即降级、README 登记兼容声明、不缓存 AST 跨版本
- **路径 B（import/export 管线插件扩展点）裁决不做**：等出现多个真实第三方格式需求再立项

### 2.4 Web/TUI 工程纪律（mock workbench alignment 2026-11-16，已落地）
- **axum 0.8 陷阱**：动态路径段必须 `"/history/{id}"`，旧 `:id` 语法会运行时 panic
- **DTO 兼容铁律**：web types 新增字段必须 `#[serde(default)]`；改 DTO 后必须 `cargo check --bins --tests`（结构体字面量在 tests/ 同步）
- **只读守卫**：变更类 handler 首行 `state.readonly` 检查 403
- **防任务泄漏**：事件通道 sender 只能移入服务任务、严禁留存于管理结构——Drop 后 recv() 返回 None 让消费协程干净退出
- **词法路径提取**：Mock 路由禁止 `url::Url::parse`（会把 `{id}` 编码为 `%7Bid%7D`）

### 2.5 统一基础服务五准则（unified-services 2026-11-16，已落地）
①领域层零感知（依赖倒置：存储/注册表只收注入的 PathBuf/配置，禁 `std::env::var` 与硬编码 `.rupost`）②serde 向后兼容 ③CLI/TUI/Web 三端同源（WorkspaceConfig 单点快照）④安全边界（[paths] 沙箱囚禁 project_root；AI 凭据仅 `api_key_env` 或 0600 credentials.toml，动态解析的 key 自动注册脱敏词表）⑤Simplicity First（不建上帝对象/单用工厂/多余依赖）

### 2.6 二进制级测试纪律（binary-testing 2026-11-16，T1-T4 已落地）
- **五条军规（测试侧）**：`Command::cargo_bin` 统一定位；路径断言走 posix 归一；TOFU 一律指纹预置禁交互等待；动态内容（端口/时间戳/绝对路径）禁入黄金文件；统一注入 `NO_COLOR=1`
- **pytest 异构三不可替代**：①异构交叉验证（Python hashlib 独立复算 sha256，防"实现自证"）②手动指南 1:1 参数化（文档腐烂由 CI 报警）③独立断言生态（jsonschema/参数化）
- **exit 2 三来源鉴别**：clap 参数错误 `error: ` / 解析错误 `[ERROR] 解析错误: ` / 契约基线 I/O `[ERROR] 契约基线工件错误: `
- **trycmd→assert_cmd 决策记录**：trycmd 1.2 沙箱不自动携带 fixture 文件——改 assert_cmd 精确输出断言形态，零沙箱斗争
- **nextest 纪律**：改变执行拓扑（每用例独立进程），先 optional job 试点再转正

## 3. 安全评估结论（2026-11-07 CLI 安全评估）

**四攻击面结论**：输入面 1 项 P2（import/generate 生成物结构污染——三类走私全被既有防线阻断，无可执行注入，登记"告警不改写"设计任务）；执行面无缺陷；写入面 P1（`--report-file` 静默覆盖+跟随 symlink——**已修复**：suite_presenter 覆盖提示已落地）；泄露面无新发现。

**防御已验证充分面清单**（复用价值：安全审计基线）：`@depends-on` 沙箱多根 allowlist、Web PathSandbox（穿越/隐藏/扩展名/悬空软链）、Host 门禁 RFC 9112 Default-Deny、readonly 三端点 403、静态资产扩展名白名单、`--on-failure` 钩子（env 传递无 shell 拼接/10s kill_on_drop/URL 脱敏）、JSON 报告 body_snippet 脱敏 512B 截断、Mock 仅绑 127.0.0.1、Cookie 0600+TOCTOU 原子创建、历史明文凭据=有意设计（本机存储，外分享走 export --redact/generate 模板化）。

**流程建议（部分未落地）**：`cargo audit` 未纳入 make gate——建议增设 `make audit` 目标（发版前仪式）。

## 4. 测试完备性审计结论（2026-11-09 全域走查）

- 总体：无 P0；抽查 5 组关键守护全部真红（反假守护纪律成立）；生产 unwrap 卫生良好（cookie 25 个/metadata 33 个全在 tests）
- 结构性缺口三项——**后均闭环**：Web 前端 JS 测试基建（现 `tests/web/app_test.mjs` + CI job ✓）、TUI 三文件直测（现 tui_key_actions/features_full/ws_session ✓）、formatter 零直测（后续批次补齐）
- 永久有效的方法论：机械盘点（`#[test]` 计数 + to_str 残留 + unwrap 热点全量扫描）+ 定点读码甄别 + 守卫删实现必红抽验

## 5. 文档治理经验（2026-10-26 全库普查，本次治理的直接依据）

- **化石层教训**：dev/ 旧快照（QUICK_REFERENCE 含幽灵路径与相反结论、progress_summary 三重自相矛盾）证明"归档不清理 = 误导源"；本次治理是其终局执行
- **知识治理四制度**（本次起生效）：①现行核心集之外默认"历史"，入核心集需 progress_summary 登记 ②设想文档生命周期：立项挂"设想中"横幅，取代/搁置更新横幅而非静默；超两季度未启动 → future_backlog 一行条目 + 原文归 git ③PRD 交付物路径写"模块域"不写逐文件路径（防腐最高频矛盾）④单一进度源：同名双 progress_summary 是误读源，必须消除
- **审计方法论**：分区审计 + 争议结论二次实证（grep 代码 + 真实二进制运行）——子代理审计 2 处误报被实证推翻（`--debug` 全局参数存在性、cookie 环境隔离双接线）
- **自我降级横幅**（doc/competition 底稿首创）：被取代文档头部显式声明取代关系——本存档即该制度的延续
- **定位演进链**见 §1；悬空设想 27 份的沉没投资教训：WASM 插件 7 篇/AI 三方案/Rhai 脚本/OpenAPI 文档生成/离线知识库均零落地——设想≠承诺，落地前不进核心集

## 6. archive 历史方案里程碑结论（85 份，2026-11-19 删除）

- **2026-q2（11 份）**：Stage/Sprint 时代设计——多文件 DAG（Sprint 2）、Markdown 生命周期、Mock 服务器、URL 拼接、网络诊断、WebSocket（Sprint 5）、启发式拆分全部落地；未落地：Sprint 3 Rhai 脚本（与 script/ 三胞胎同案，零落地）、离线知识库
- **2026-q3（6 份）**：快照重放 roadmap（掩码/智能 diff/错误脱敏落地；虚拟响应/加密重放未做）、历史存储优化（tail O(N) 已落地，HistoryConfig 未做）、生产调试（阶段 1/2/7 落地；4/5/6/8 未做）、TUI 迭代 v2（落地）
- **2026-q4（34 份）**：质量加固 25 项、TUI v3、测试方案 19 域矩阵、AI 全族（assert/context/zero-config/triage/attributioin/bootstrap/resolve）、生成语义 T1/T2、会话隔离、复现包、契约漂移、流式调试——**全部落地**；未落地：AI OpenAPI-gen P2（`rupost ai gen`）、AI 蒸馏、能力云接入
- **legacy_research（34 份）**：2026-02 设想层（普查定性"悬空设想"）：插件 WASM 7 篇（被 Stdio IPC 体系取代）、AI 方案 A/B/C（被第四条路取代）、Rhai 脚本、OpenAPI 文档生成、TUI 早期方案（被 tui/ 现行档取代）——决策过程记录，结论已吸收进各自现行档案
- 原文找回：`git log --diff-filter=D --name-only -- 'doc/plans/archive/*'` 或任意 2026-11-19 之前的提交

## 7. 未落地设想登记索引（已并入 future_backlog 者）

变量加密（SOPS-like `ENC[...]`）、`rupost doc export`（.http → OpenAPI 3.1 + Redoc 托管，"RuPost 扮演 Converter"路线）、Phase 3 社区移交双项（OpenAPI 导入/k6 场景导出）、AI OpenAPI-gen（`rupost ai gen`）、快照虚拟响应/加密重放、C7 VSCode 扩展独立立项、E-6 captured_keys（隐私评审后）、统一会话资产抽象、黄金样本变更检测实践——明细与裁决见 [future_backlog.md](../plans/future_backlog.md)。
