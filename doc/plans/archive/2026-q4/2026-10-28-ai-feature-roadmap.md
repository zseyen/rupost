# AI 功能清单 · 优先级 · 示例代码 (Roadmap Consolidated v1.0)

> **创建日期**: 2026-10-28
> **定位**: 全部 AI 相关功能的**单一清单**——状态、优先级、示例代码（用法 + 实现骨架）、缺口登记。
> **优先级裁决**: 效率公式（省时 > 切换+等待+审查）× 旅程断点 × 竞争空位 × 依赖就绪度。
> **上游**: [总体规划](2026-10-27-ai-master-roadmap.md) / [MVP 整合计划](2026-10-27-ai-mvp-consolidation-plan.md) / [旅程设计](2026-10-27-ai-developer-journey-design.md)

---

## 1. 功能清单与优先级

### ✅ 已交付（本轮迭代，596 测试 0 失败）

| # | 功能 | 优先级履行 | 示例入口 |
| :- | :- | :- | :- |
| F1 | `ai assert` NL→断言 | — | `examples/ai_assert/` |
| F2 | `test --explain` E2 就地归因 | P1（已清） | `examples/ai_triage/README.md` §2 |
| F3 | `ai triage` / `--triage` E5 聚类 | P1（已清） | 同上 §3 |
| F4 | `--report-file` 强类型报告 | — | 同上 §4 |
| F5 | B4 配置链 + `--last/--response/--no-mask` | P0（已清） | `examples/ai_assert/README.md` |

### 🔜 待实施（按优先级排序）

| 优先级 | # | 功能 | 主体 | 判定依据 | 依赖 |
| :- | :- | :-- | :-- | :-- | :-- |
| **P0** | F6 | `import curl` → `.http` | **通用**（`src/importer/`，非 AI） | 每日高频、零等待零审查；与 exporter 互逆 | 无 |
| **P0** | F7 | `# @ai-assert` + `ai resolve` | AI（`src/ai/resolve.rs`） | 就地形态消灭上下文切换；全组件现成 | F5 |
| **P1** | F8 | `ai bootstrap --from history` | AI（批量） | 批量杠杆、竞争空位（vs Keploy） | F5 redactor ✅ |
| **P1** | F9 | MCP Server MVP（3 Tool） | **通用**（`src/mcp/`，供给侧） | 契约驱动 agent 闭环；包壳既有命令 | 无硬依赖 |
| **P2** | F10 | `ai gen --spec`（确定性导入先行） | 通用+AI | S1 冷启动坡道；AI 增强停泊 | F6 importer 架构 |
| **P2** | F11 | TUI 内嵌补全（断言 + body 模版） | AI（就地） | 形态最优但 TUI 异步改造回归面大 | F5 |
| 登记 | F12–F17 | E1/E4/E6/E8/E9/E3；agent 化端点；junit 报告 | — | 全部被基建依赖挡住或需求未验证 | 各随基建 |

### 🚫 Won't do（裁决存档）

运行期 AI（`@ai_assert` 指令/自动修复判优）、AI 聊天面板、code-first 文档流水线、AI 评分体系、云端同步、Postman 导入（P2 未裁决）、Swagger 2.0/YAML 规格。

---

## 2. 示例代码（用法 + 实现骨架）

### F6 `import curl`（P0，通用模块）

**用法**：

```bash
# 单条：浏览器 DevTools → Copy as cURL → 直接粘贴
rupost import curl "curl -X POST 'https://api.example.com/login?next=/home' \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer eyJ...' \
  -d '{\"username\":\"admin\"}'" -o login.http

# 批量：文件内每行一条 curl
rupost import curl curl_dump.txt --into suite.http
rupost test suite.http          # 立即可执行
```

**实现骨架**（`src/importer/curl.rs`，通用模块——与 `src/exporter/` 对称）：

```rust
/// POSIX 子集分词器：单引号/双引号/反斜杠转义（浏览器 Copy as cURL 的常规形态）。
/// 长尾（$'..'、<(process)）显式报"暂不支持"，OCP：tokenizer 独立可替换。
fn tokenize(input: &str) -> Result<Vec<String>, CurlParseError>;

/// curl flag → 请求字段（决策表驱动，语料锁测试）
fn parse_tokens(tokens: &[String]) -> Result<CurlCommand, CurlParseError> {
    // -X/--request → method；-H/--header → headers（重复累积）
    // -d/--data/--data-raw → body（隐式 POST）；-F → 暂不支持报错
    // --url / 位置参数 → url；-b/--cookie → Cookie 头
}

/// 发射复用（DRY 关键）：合成 HistoryEntry → 复用 HttpGenerator 发射
pub fn import_curl(input: &str, output: Option<&Path>) -> Result<()> {
    let cmd = parse_tokens(&tokenize(input)?)?;
    let entry = synth_history_entry(cmd);            // 复用 history::model
    let text = HttpGenerator::generate(&[entry])?;   // 复用 generator（现成发射器）
    create_exclusive_or_stdout(output, &text)?;      // 复用 path_guard 防覆盖
}
```

**范围裁决（MVP）**：支持 `-X/-H/-d/--data-raw/--url/-b/--compressed`；`-F`/`--data-urlencode`/`$'..'` 显式报错；20 条真实 devtools 语料决策表锁行为。

---

### F7 `# @ai-assert` + `ai resolve`（P0，就地形态）

**用法**：

```http
### login
POST {{base_url}}/auth/login
# @ai-assert 登录成功且返回管理员
```

```bash
rupost ai resolve login.http        # 原地展开（需 AI 配置，走 B4 链）
git diff login.http                 # 逐行审查展开结果
```

**实现骨架**（`src/ai/resolve.rs`）：

```rust
pub struct ResolveReport { pub expanded: usize, pub skipped: usize, pub failed: Vec<(usize, String)> }

pub async fn resolve_file(path: &Path, no_mask: bool) -> Result<ResolveReport> {
    let original = std::fs::read_to_string(path)?;
    let resolved = crate::ai::config::resolve().await?;          // 复用 B4 链
    let mut out = String::new();
    let mut report = ResolveReport::default();

    for line in original.lines() {
        match parse_intent(line) {                                // "# @ai-assert " / "// @ai-assert "
            Some(intent) => match translated_lines(&intent, &resolved).await {
                Ok(lines) => { report.expanded += 1; out.push_str(&lines); }   // 缩进对齐原行
                Err(e) => { report.failed.push(..);
                            out.push_str(&format!("{line}  # ⚠ 展开失败: {e}")); }  // 保留原指令
            },
            None => out.push_str(line),
        }
        out.push('\n');
    }
    atomic_write(path, &out)?;                                    // 复用 path_guard 原子写
    Ok(report)
}
```

**要点**：展开失败的指令**保留原文 + 行内警告**（绝不静默丢弃）；`--dry-run` 输出 diff 不落盘。

---

### F9 MCP Server MVP（P1，供给侧）

**用法**（Claude Desktop / Claude Code 注册）：

```json
{ "mcpServers": { "rupost": { "command": "rupost", "args": ["mcp", "serve", "--root", "."] } } }
```

**三个 Tool 的 JSON Schema（本节即冻结格式，补齐此前空白）**：

```jsonc
// Tool 1: run_suite —— 执行测试文件/DAG，返回结构化结果
{ "name": "run_suite",
  "inputSchema": { "type": "object",
    "properties": {
      "paths":  { "type": "array", "items": {"type": "string"}, "description": "测试文件/目录（相对 --root）" },
      "var":    { "type": "array", "items": {"type": "string"}, "description": "key=value 覆盖" },
      "fail_fast": {"type": "boolean", "default": false} },
    "required": ["paths"] },
  "output": "{ total, passed, failed, results: [{file, request_number, name, status, passed, error}] }" }

// Tool 2: get_history —— 查询历史交换（脱敏后返回）
{ "name": "get_history",
  "inputSchema": { "type": "object",
    "properties": { "last": {"type": "integer", "default": 5},
                    "url_contains": {"type": "string"} } } }

// Tool 3: start_mock —— 从文档/快照起确定性 mock，返回 base_url
{ "name": "start_mock",
  "inputSchema": { "type": "object",
    "properties": { "file": {"type": "string"}, "port": {"type": "integer", "default": 0} },
    "required": ["file"] } }
```

**实现骨架**（`src/mcp/server.rs`，通用模块——依赖方向是"agent 调 rupost"）：

```rust
pub async fn serve(root: PathBuf, mock_only: bool) -> Result<()> {
    // stdio JSON-RPC loop：initialize → tools/list → tools/call
    // 安全边界：路径 join 后必须 within(root)；history 输出经 redactor；--mock-only 时禁 run_suite
    loop {
        let req = read_jsonrpc_line(stdin()).await?;
        let resp = match req.method.as_str() {
            "tools/list"  => tools_catalog(),
            "tools/call"  => dispatch_tool(&req, &root, mock_only).await,  // 内部复用 run_test_suite/mock
            _             => jsonrpc_error(MethodNotFound),
        };
        write_jsonrpc_line(stdout(), resp).await;
    }
}
```

---

### F8 `ai bootstrap`（P1，批量——骨架见提案 §3/§4，此处只列复用映射）

| 管线阶段 | 复用组件 | 新增量 |
| :-- | :-- | :-- |
| A 素材采集 | `HistoryStorage::tail` / `read_snapshot_suite` ✅ | `--from` 参数分发 |
| B 机械还原 | `HttpGenerator`（与 F6 同一发射器） | 断言注入点 |
| C AI 增强 | `AiClient` + `json_util` 宽容解析 ✅ | `TestSynthesizer`（桶建议 DTO） |
| D 引擎裁决 | `assertion::parser` + `MockCompiler` ✅ | lint 报告聚合 |
| E 回归资产 | 普通 `.http` ✅ | 零 |

---

## 3. 查漏补缺（本轮发现与处置）

| # | 缺口 | 处置 |
| :- | :-- | :-- |
| G-1 | `examples/ai_assert/README.md` 未覆盖 P0 新功能（`--last/--response/--no-mask`/B4） | ✅ 本轮已补三节（含真机输出） |
| G-2 | `progress_summary.md` 缺 P0 交付行 | ✅ 本轮已补 |
| G-3 | MCP Tool schema 未冻结（此前已知最大空白） | ✅ 本节 §2-F9 提供三 Tool schema 草案，MVP 立项时评审冻结 |
| G-4 | `ai.toml` 字段级 schema 未冻结（此前已知） | ✅ 已在 `examples/ai_assert/README.md` B4 节给出样例 + config.rs TC-C05 测试锁定 api_key 拒绝 |
| G-5 | R6 `@ai-assert` 指令语法未冻结 | ✅ §2-F7 定义：`# @ai-assert <nl>` / `// @ai-assert`；失败保留原文 + 行内警告 |
| G-6 | R4 curl 长尾范围未裁决 | ✅ §2-F6 MVP 范围裁决（支持/报错清单 + 20 语料门禁） |
| G-7 | E1/E4 diff 报告格式未定义 | 🔲 登记跟随确定性 compare 基建（维持原判） |
| G-8 | **progress_summary 交付物仍列具体文件路径**（审计 §4-3 指出"路径漂移最高频矛盾"，本轮自身也犯） | 🔲 制度执行：新行已开始写模块域（如 `src/ai/`），旧行逐批改造 |
| G-9 | `redactor` 词表 `tokenize` 子串误报（安全侧过度掩码） | 登记不修（安全侧偏移可接受，复核员结论） |

## 4. 执行顺序建议

```
F6 import curl（通用，~3 天）──→ F10 确定性导入（复用 importer）
F7 resolve（AI，~2 天）─────────┐
F8 bootstrap（AI，~3 天）───────┼── MVP 收口后 → F9 MCP MVP（~4 天）→ 二期
（F6 与 F7/F8 可并行：不同模块、无共享文件）
```

每项延续 TDD 门禁（fmt / clippy -D warnings / workspace 测试 / verify 冒烟）与 B 级兜底声明。
