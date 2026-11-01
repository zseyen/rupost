# F7 `ai resolve` 与 F8 `ai bootstrap` 功能详述与盲区分析 (Design Deep-Dive v1.0)

> **创建日期**: 2026-10-28
> **定位**: 两项待实施 AI 功能的**实施前深潜**——功能详述、使用场景、示例代码（用法 + 实现骨架）、以及既有文档**未覆盖**的盲区清单。实施时以本文档 §4 盲区清单为强制检查表。
> **上游**: [功能路线图 §2 F7/F8](2026-10-28-ai-feature-roadmap.md)、[bootstrap 提案](2026-10-26-ai-bootstrap-proposal.md)、[总体规划 B0–B4](2026-10-27-ai-master-roadmap.md)

---

## 1. F7 `# @ai-assert` 指令 + `rupost ai resolve`（就地展开）

### 1.1 功能详述

在测试文件里**原位写下意图**，一条命令将全部指令原地展开为标准 `@assert` 断言：

| 维度 | 设计 |
| :-- | :-- |
| 指令语法 | `# @ai-assert <自然语言>`（.http）/ `@ai-assert <自然语言>`（.md 裸行，对齐 cookie_demo 既有形态）；`// @ai-assert` 兼容 |
| 识别规则 | trim 后以指令前缀开头且意图非空；文档正文提及（非行首）不命中 |
| 展开行为 | 原位替换为标准 `@assert` 行（缩进对齐；.http 前缀 `# `，.md 裸行）——**产物天然幂等**（展开后不再是 @ai-assert，重复 resolve 无操作） |
| lint 前置 | AI 返回的每条断言过 `assertion::parser` 本地预检，非法行丢弃 + 警告（**与 bootstrap lint 同源**——非法断言不得落盘） |
| 失败语义（B2） | AI 全局失败（无配置/网络/连续错误）→ **文件原样不动** + 黄色指引 + exit 1；个别指令失败 → 该行保留原文 + 行内警告 + exit 0 |
| 配额与熔断 | 单文件最多展开 `MAX_RESOLVE_DIRECTIVES = 20` 条指令；连续 2 错熔断（复用 explain 模式） |
| 原子性 | 全量内存处理 → 临时文件 + rename 原子写（复用 path_guard 模式）；Ctrl+C/崩溃不产生半文件 |

### 1.2 使用场景

| # | 场景 | 效率账 |
| :- | :-- | :-- |
| SC-A | 新手写测试：意图写在请求块末尾，不懂语法也能得到正确断言 | 免学 8 运算符语法（10 分钟 → 10 秒） |
| SC-B | 团队评审：`git diff` 直接展示"意图 → 断言"映射，审阅断言合理性 | 评审语义化（比读裸断言快） |
| SC-C | 存量测试批量升级：老文件逐处补 `@ai-assert`，一次 resolve | 与手写等价产物 |
| SC-D | 终端工作流：无需切出终端/开 GUI 对话 | 零上下文切换（就地形态） |

### 1.3 示例代码

**用法**：

```bash
# 文件内已写好意图（.http）：
### login
POST {{base_url}}/auth/login
Content-Type: application/json

{"username": "admin", "password": "***"}

# @ai-assert 登录成功且返回管理员

# 先预览（不落盘），再展开
rupost ai resolve login.http --dry-run
rupost ai resolve login.http
# [✓] 已展开 1 条指令 → login.http（skipped 0, failed 0）
```

**实现骨架**（`src/ai/resolve.rs`，复用标注）：

```rust
pub struct ResolveReport {
    pub expanded: usize,
    pub skipped: usize,                     // 熔断/配额后未处理的指令
    pub failed: Vec<(usize, String)>,       // 行号 + 原因
}

pub async fn resolve_file(path: &Path, dry_run: bool) -> Result<ResolveReport> {
    let original = std::fs::read_to_string(path)?;
    let resolved = config::resolve().await?;                 // 复用 B4 链 ✅
    let mut out = String::new();
    let mut report = ResolveReport::default();
    let mut session = DirectiveBudget::new(MAX_RESOLVE_DIRECTIVES, CIRCUIT_BREAKER_THRESHOLD);

    for (lineno, line) in original.lines().enumerate() {
        let Some(intent) = parse_directive(line) else {      // 两种前缀 + 意图非空
            out.push_str(line); out.push('\n'); continue;
        };
        if !session.take(&intent) {                          // 配额/熔断（复用 explain 模式 ✅）
            out.push_str(line); report.skipped += 1; continue;
        }
        match translated_block(&intent, &resolved).await {   // 复用 AssertionTranslator ✅
            Ok(assert_lines) => {
                // 每条过 assertion::parser 预检（Gap R7-1，见 §4）——非法丢弃
                let checked: Vec<_> = assert_lines.into_iter()
                    .filter(|l| parser_ok(&l.assert_expr, &mut report))
                    .collect();
                for l in checked { out.push_str(&render_line(line, &l)); out.push('\n'); }
                report.expanded += 1;
            }
            Err(e) => {
                report.failed.push((lineno + 1, e.to_string()));
                out.push_str(line); out.push('\n');          // 失败行保留原指令
                out.push_str(&format!("{line}  # ⚠ 展开失败: {e}\n")); // 行内警告
            }
        }
    }

    if !dry_run { atomic_write(path, &out)?; }               // 复用 path_guard 原子写 ✅
    Ok(report)
}
```

### 1.4 F7 盲区清单（既有文档未覆盖）

| # | 盲区 | 设计裁决 |
| :- | :-- | :-- |
| R7-1 | **AI 断言缺 parser 预检**——非法断言落盘后要等 `rupost t` 才暴露 | resolve 时本地过 `assertion::parser`，非法丢弃 + 警告（与 bootstrap lint 同源复用） |
| R7-2 | **.md 断言形态差异**——md 中断言为裸行（无 `# `），且须落在代码块内 | 双前缀识别；.md 输出按所在文件形态渲染（检测文件内既有 @assert 形态，缺省裸行） |
| R7-3 | **展开多行的落点**——1 条指令展开为 2–3 行断言 | 原位替换 + 缩进对齐；不移动到块末尾（用户放置位置即权威） |
| R7-4 | **幂等性** | 产物为 `@assert`（非 @ai-assert）→ 重复 resolve 天然无操作；失败行保留原指令 → 重试仅重试失败行 |
| R7-5 | **退出码语义** | AI 全局失败（文件未动）→ exit 1；部分失败（已写 + 警告）→ exit 0；`--dry-run` 恒 exit 0 |
| R7-6 | **批量 vs 逐条调用** | 逐条（隔离性：单条失败不影响其他行）+ 配额 20 + 熔断；批量单调用留作大文件优化 |
| R7-7 | **上下文增强（--with-request）**——附所在请求块给 AI | 登记增强项：MVP 不带（纯 NL 翻译已覆盖主场景） |
| R7-8 | **`--no-mask` 不适用** | resolve 无响应上下文，无脱敏开关（与 P0 的 `--last` 区分） |

---

## 2. F8 `ai bootstrap --from history`（失败聚类之外的批量杠杆）

### 2.1 功能详述（五阶段管线落地细化）

| 阶段 | 确定性 | 内容 | 复用/新增 |
| :-- | :-- | :-- | :-- |
| A 素材采集 | ✅ | `--from history --last N`（默认 10，上限 50）→ `HistoryStorage::tail` | 复用 ✅ |
| B 机械还原 | ✅ | 每条 → 请求块 + `status` 断言 | 复用 `HttpGenerator` ✅ |
| C AI 语义增强 | 🤖 | 逐条发送脱敏上下文 → `TestSynthesizer` 建议断言/`@name`/`@capture`/`@depends-on` | 新增 crate `Synthesizer`（模式复用 attribution ✅） |
| D 引擎裁决 | ✅ | 断言过 parser、`@capture→{{var}}` 闭合、`@depends-on` 仅批内 name 成环检测 | 复用 parser/命名冲突表 |
| E 回归资产 | ✅ | 原子落盘（防覆盖）→ `rupost t` 即可执行 | 复用 path_guard ✅ |

### 2.2 使用场景

| # | 场景 | 效率账 |
| :- | :-- | :-- |
| SC-A | 调试半小时（登录→建单→查询链路）→ 一条命令沉淀为带断言/捕获链/依赖的回归文件 | 1–2 小时手工 → 1 分钟 + 5 分钟审查 |
| SC-B | CI 失败回流：失败 → 修复 → 新历史 → 更好的下一轮素材（飞轮） | 资产随使用增值 |
| SC-C | 团队交接：`.http` 即文档，git pull 即同步 | 零额外文档成本 |
| SC-D | 降级自证：无 AI 环境产出机械还原版（仍是有用的 status 断言文件） | 断网可跑（B1 判据） |

### 2.3 示例代码

**用法**：

```bash
rupost ai bootstrap --from history --last 10 -o regen/auth_flow.http
# [✓] 已生成 regen/auth_flow.http：10 个请求块，8 条 @capture，26 条 @assert（AI 增强 19 条）
# [!] 2 条 AI 建议被 lint 丢弃（非法断言），详见上方警告
rupost t regen/auth_flow.http        # 产物直接可执行（自足判据）
```

**实现骨架**（`crates/rupost-ai/src/synthesizer.rs` + `src/ai/bootstrap.rs`）：

```rust
// ── crate 侧：TestSynthesizer（纯 DTO 进出，复用 attribution 模式 ✅） ──
#[derive(Serialize)]
pub struct SynthesisItem {                  // 单条历史 → 脱敏紧凑上下文
    pub index: usize,
    pub method: String, pub url: String, pub status: Option<u16>,
    pub request_body_snippet: Option<String>,   // 已脱敏 4KB
    pub response_body_snippet: Option<String>,  // 已脱敏 4KB
    pub available_names: Vec<String>,           // 批内已有 @name（依赖引用约束）
}

#[derive(Deserialize)]
pub struct SynthesisAdvice {                // 宽容解析：漏字段/未知字段不炸
    pub index: usize,
    pub name: Option<String>,
    pub assertions: Vec<String>,                // 每块 ≤ 8 条（上限防 token 失控）
    #[serde(default)] pub captures: Vec<String>,   // "token from body.token"
    #[serde(default)] pub depends_on: Vec<String>, // 仅可引用批内 name
}

// ── 主包编排（src/ai/bootstrap.rs）──
pub async fn run_bootstrap(args: &BootstrapArgs) -> Result<()> {
    let entries = collect_last(args.last)?;                          // 复用 F5 context 采集 ✅
    let base = HttpGenerator::generate_with_variables(&entries, true)?;  // B 阶段 ✅
    let advice = match synth_all(&entries).await {                   // C 阶段（逐条 + 熔断）
        Ok(a) => a,
        Err(e) => { warn!("AI 不可用（{e}），已输出机械还原版"); return write_baseline(args, base); }
    };
    let linted = lint_and_merge(base, advice)?;                      // D 阶段（裁决 + 合并）
    create_exclusive_write(&args.output, &linted)?;                  // E 阶段（防覆盖）
    print_summary(&linted);
}
```

### 2.4 F8 盲区清单

| # | 盲区 | 设计裁决 |
| :- | :-- | :-- |
| R8-1 | **素材跨会话混杂**——tail 10 可能含登录/订单/杂项混合，依赖链推断失真 | 新增 `--filter <url 前缀>`；默认全部但按 URL 模式聚类分组，跨组不推依赖 |
| R8-2 | **同 URL 多实例语义不同**（/users/1 成功、/users/2 404） | 同 URL 模式多次出现 → 取最后一次为"现势"块，历史实例仅作 @name 候选；不合并 |
| R8-3 | **依赖推断保守原则** | 仅当出现 `@capture token`（响应含 token 类字段）且后续请求引用同名字面 → 推 `@depends-on`；无强证据不推 |
| R8-4 | **`{{var}}` 模板字面量进 AI prompt**——generator 已把凭据模板化为 `{{authorization}}`，AI 可能误当数据 | system prompt 声明"`{{var}}` 为运行时变量占位，原样保留在建议中，不要当作真实值" |
| R8-5 | **断言数量上限** | 每块 ≤ 8 条（lint 层丢弃超出部分 + 警告），防单块 token 失控 |
| R8-6 | **产物来源审计** | 产物头部注释：`# generated by rupost ai bootstrap at <时间> from history(last N)` |
| R8-7 | **大量历史（上限）** | `--last` 上限 50（超出报错建议 --filter）；API 调用总量 = min(N,50) 次逐条 + 熔断 |
| R8-8 | **exit code 语义** | AI 降级机械版 → exit 0（B1）；素材空 → exit 1 报错；防覆盖失败 → exit 1 |

---

## 3. 横切盲区（F7/F8 共同 + 全局）

| # | 盲区 | 设计裁决 |
| :- | :-- | :-- |
| X-1 | **AI 调用成本不可见**——chat completions 响应含 `usage`（prompt/completion tokens），当前丢弃 | 统一在 `AiClient` 捕获 usage，`--verbose` 或环境变量 `RUPOST_AI_USAGE=1` 时逐调用打印；不改默认输出 |
| X-2 | **发给 AI 的最终 prompt 用户不可见**——排障困难（"AI 为什么给出这个建议"） | `RUPOST_AI_SHOW_PROMPT=1` 调试钩子：打印脱敏后的最终 system+user prompt（redactor 已保证无敏感值） |
| X-3 | **批量任务总时长无上限**——per-call 30s 但 N 次串行 | 各功能自带上限（explain 3 / resolve 20 / bootstrap 50），总量 = 上限 × 30s 封顶；登记不另做总预算 |
| X-4 | **模型输出语言漂移**——中文 NL 可能得到英文文案（root_cause 等） | 断言是 DSL 无语言问题；文案语言不约束（观察项，不加 prompt 强制） |
| X-5 | **resolve/bootstrap 的 prompt 模板无集中管理**——散在各文件的字符串 | 登记技术债：统一 `crates/rupost-ai/src/prompts.rs`（等第三个 prompt 出现再抽，当前 3 个不违反 DRY 临界） |

## 4. 对既有文档的登记建议

1. roadmap §2 F7 行补"示例代码见本文档"（本文件为准）。
2. bootstrap 提案 §4.1 的建议 schema 与本文档 §2.3 `SynthesisAdvice` 对齐（新增 available_names/上限字段）。
3. master plan §4：F7/F8 状态更新为"设计深潜完成，可开工"；X-1/X-2 登记为横切增强。
4. P0 测试方案不变（已交付）；F7/F8 各自测试方案在开工前按 §1.4/§2.4 盲区清单展开（流程同 E2/E5）。
