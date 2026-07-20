# RuPost 质量加固与 Cookie 加固执行方案

> **创建日期**: 2026-10-18（v2，经三路子代理独立审查后修订）
> **来源**: 三轮审查（代码质量 → 对抗性测试 → Cookie 评估）+ 子代理审查（可行性核查 / 完整性核查 / 风险评审）
> **状态**: 待执行
> **验证基线**: 237 单元测试 + 81 集成测试全绿（clippy 完整命令：`SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo clippy --all-targets`，aws-lc-sys 与 macOS 27 SDK 不兼容，属上游问题）

## v2 修订摘要（子代理审查成果）

- **修正 1 个会导致编译失败的错误前提**：Phase 0.2 的 fs2 移除——std 1.89 的 API 名是 `lock()` 而非 `lock_exclusive()`，3 处调用点必须先改名
- **补齐 2 条遗漏**：WS session 的 `Result<_,String>` 错误类型、cookie 默认不持久化的用户提示
- **修复 4 个方案漏洞**：沙箱 LCA 退化、`mock/server.rs` 同款 cwd 沙箱 bug 被漏、BufWriter flush 缺口、3.4 CLI 参数面低估
- **独立扫描新发现 6 个 bug**（4 高危）：WS EXPECT 运算符缺失、同源运算符误切第二实现点、URL `://` 子串误判、mock Trie `**` 中段死路由、f64::EPSILON 误用、SSE 断言路由误伤
- **调整执行顺序**：SseRunner 去重（原 6.2）前置到 SSE 修复之前，避免 60 行重复块复制三份
- **删除过度设计**：`@sse_legacy_events` 兼容开关（项目未发布，无兼容包袱）
- **补充关键豁免**：`@timeout` 必须对流式请求豁免，否则修复本身会制造"SSE 被误杀"新 bug

标注约定：**[实测]** = 已用二进制+本地服务器复现；**[代码]** = 代码级确认；**[新]** = 子代理独立扫描新发现。

---

## Phase 0 — 发布阻塞项（P0，约 0.5 天）

| # | 问题 | 位置 | 修复 | 验证 |
|---|------|------|------|------|
| 0.1 | MSRV 声明 1.85，实际需 1.89（let-chains 约 24 处 + File 锁 API） | `Cargo.toml` | `rust-version = "1.89"` | clippy 无 `incompatible_msrv` |
| 0.2 | fs2 迁移不彻底：`lock_shared`/`unlock` 已被 std 接管，但 **`lock_exclusive()` 在 std 中名为 `lock()`**，cookie.rs:129、storage.rs:89、storage.rs:171 三处需先改名才能删 fs2 | `cookie.rs:11,129`、`storage.rs:5,89,171` | ① 3 处 `lock_exclusive()` → `lock()`；② 删两处 `use fs2::FileExt`；③ Cargo.toml 移除 fs2；④ 修正 storage.rs:58/132 注释 | `cargo build` + `cargo tree \| grep fs2` 为空 |
| 0.3 | 格式偏差（app.rs:233、handlers.rs:149、handlers.rs:158） | `src/tui/` | `cargo fmt` | `cargo fmt --check` 通过 |
| 0.4 | 错误提示教用户 `-v base_url=...`，`-v` 实为 verbose [实测] | `src/error.rs:50` | 文案改 `--var base_url=...`。**独立成 commit**（用户可见修复 ≠ 构建卫生） | 触发 BaseUrlNotConfigured 验证提示可执行 |
| 0.5 | 新建最小 CHANGELOG.md（后续 4.2/3.2 的行为变更登记处；风险登记引用它但文件不存在） | 仓库根 | 建立 + 预登记 4.2（SSE 派发语义，直接 breaking，无兼容开关——项目未发布无存量用户） | 文件存在 |

**提交**: `fix(build): correct MSRV to 1.89, complete fs2→std migration` + `docs: fix misleading -v suggestion` + `chore: add CHANGELOG`（3 个原子提交）

---

## Phase 1 — 崩溃修复（P0，约 1 天）

> 原则：用户输入（状态码/断言/header）永远产生失败的测试结果，绝不 panic。

### 1.1 非标准状态码（600–999）panic [实测]
- **根因**: `src/http/types.rs:160` 校验 `100..600`，http crate 允许 100–999
- **修复**: 放宽为 `100..=999`。已核查全部消费方无副作用：`is_success` 等为区间判断（700 自然判失败）；断言 extractor 取 `code as f64`（`@assert status == 700` 可行）；reporter/replayer 仅消费 u16
- **回归**: wiremock `ResponseTemplate::new(700)`（已证实支持，http crate `StatusCode::from_u16(700)` 合法）→ 失败结果而非 panic

### 1.2 断言值单引号字符切片 panic [实测]
- **根因**: `assertion/parser.rs:171` 无 `len() >= 2` 守卫（capture.rs:73 有正确写法）
- **注意**: 该 panic 是 2.1 运算符误切的下游症状，守卫只消 panic 不消根因，与 2.1 的回归测试一起写
- **回归**: `contains "<"`、`contains "'"` 输出断言失败而非 panic

### 1.3 Header 名/值非法字符 panic [实测]
- **根因**: `src/http/request.rs:32-34` 的 `.expect(...)`
- **修复**: header 构建改返回 `Result`。**改动面（v2 修正，非零）**：除主路径 `Request::try_from` 外，`replayer.rs:118` 调用 `with_header` 需加 `?`；`with_auth_bearer`（request.rs:63，现无调用方）同走 insert_header 需一并处理
- **回归**: header 名含空格 → `RequestBuildFailed` 错误结果；obs-text 非 ASCII 值正常发送

**提交**: 每个 panic 一个原子提交（3 个），修复与回归测试同一提交

---

## Phase 2 — 核心正确性（P1，约 2.5–3 天）

### 2.1 断言运算符误切（两处同源实现，提为共享函数）[实测] + [新·高危]
- **现象**: `contains "a==b"`、`contains "<b>x</b>"` 被切成错误表达式；`contains "<"` panic（1.2）
- **实现点 1**: `assertion/parser.rs:21-29`（断言路径）
- **实现点 2 [新]**: `ws/action_parser.rs:176-189` `precompile_jsonpath`——**同款列表顺序缺陷**，且运算符清单更短
- **[新·高危] WS EXPECT 不支持 `>= <= > <`**: `precompile_jsonpath` 只有 `["==","!=","contains"]`，`EXPECT $.price >= 100` 会把整串当 jsonpath key → 永远 PathNotFound → 静默超时假阴性；同块 `@assert body.price > 100` 反而支持 `>`，行为自相矛盾
- **修复**: 提取共享的最左运算符扫描函数（引号感知状态机 + 平局规则：**同位置时更长运算符优先**，即 `>=` 胜 `>`——load-bearing 细节，漏写必返工），两个调用点同时替换；WS 侧补全 7 运算符
- **回归**: `assertion_edges` 域增加 WS EXPECT 用例（`>=`、引号内运算符、超时假阴性消除）

### 2.2 `@timeout` 对 HTTP 请求不生效 [实测] —— **含流式豁免（关键）**
- **根因**: `metadata.timeout` 仅 ws_runner.rs:66（握手）消费；client.rs:27,45 硬编码 30s
- **修复**: `Request` 增加 `timeout: Option<Duration>` → `TryFrom<ParsedRequest>` 透传 → `execute_raw` 用 `RequestBuilder::timeout()` 覆盖
- **⚠️ 必须豁免流式**: reqwest 的 per-request timeout 是**总时长**（含 body 读取），SSE 走同一 client 路径，`@sse` + `@timeout 5s` 会杀死长连流——修复本身制造新 bug。规定：**metadata 含 `@sse` 或 `@websocket` 时忽略 `@timeout` 总时长语义**（SSE 有专属 `@sse_timeout`），并在 CHANGELOG 说明
- **回归**: 慢端点 + `@timeout 500ms` → 超时失败（wiremock `set_delay`）；**`@sse` + 短 `@timeout` → 流不被切断**（专项验收）

### 2.3 沙箱根错用 cwd（两处同病）[实测]
- **根因**: `app.rs:41` 与 **`mock/server.rs:71`（v2 新发现，方案 v1 漏）** 均以 `current_dir()` 为沙箱根；`resolve_and_parse` 共 3 个调用方
- **修复**: v1 的 LCA 方案废弃——不相交路径时 LCA 退化为 `/`，防穿越归零。改为**多根 allowlist**：`validate_sandbox(path, roots: &[PathBuf])`，落在任一初始扫描根下即通过；app.rs 与 mock/server.rs 同步修复。同时按风险评审结论，将代码注释与 `directory_structure.md` 的"安全沙箱"措辞降格为"依赖路径合理性校验"（该检查对网络零限制，本就不是安全边界，实际价值是依赖路径写错时快速失败）
- **回归**: cwd 外绝对路径可执行；`rupost mock /abs/config.http` 不再误拒；`@depends-on ../../etc/x.http` 仍被拒

### 2.4 capture 级联丢弃且静默 [实测] —— 与 2.7 共享 warnings 通道
- **根因**: `capture.rs:218-222` 循环内 `?`（all-or-nothing）+ `capture_normal` 吞错；`TestResult`（`runner/types.rs:23-65`）**没有 warnings 字段**
- **修复**: ① 先给 `TestResult` 加 `warnings: Vec<String>` 字段 + reporter 渲染（一次结构体扩展，2.4/2.7/4.5 三个消费方共用，避免改三次）；② capture 改逐条收集、失败进 warnings；③ SSE 路径（`capture_sse_frame`/`capture_sse_llm_content`）同步接线（v1 漏）
- **回归**: 一坏一好 → 好的生效、坏的可见警告

### 2.5 `@capture` 变量名含 "from" 解析错位 [实测]
- **位置**: `metadata.rs:145`（`find("from").unwrap()` 命中变量名子串）；`ws/action_parser.rs:100`（已有 `parts[1]=="from"` 守卫，但 source 提取仍找首个子串）
- **修复**: 按 token 字节偏移切分（`split_whitespace` 无偏移，用 `match_indices` 定位独立的 `from` token），保留源表达式原始空格（regex 源需要）
- **回归**: `from_id from body.id` 正确；`from_id from regex "from (\d+)"` 中 regex 内 from 不受影响

### 2.6 大写 `.MD` 走错解析器 [实测]
- **位置**: `resolver.rs:94` `== Some("md")`；全项目仅此一处路由（tui/state.rs:101 已 lowercase）
- **修复**: `eq_ignore_ascii_case("md")`；回归：`.MD`/`.Md` 走 markdown 解析器

### 2.7 未定义变量静默字面量发送 [实测] —— 复用 2.4 的 warnings 通道
- **修复**: 替换后扫描残留 `{{...}}` → 写入 `TestResult.warnings`（不走 stderr，与 2.4/4.5 统一通道）；body 中合法字面 `{{` 可能误报，可接受（警告级别）
- **回归**: 未定义变量 → 警告含变量名；已定义 → 无警告

### 2.8 [新·高危] URL 含 `://` 子串即误判为绝对地址
- **位置**: `src/http/url.rs:16`（`raw_url.contains("://")`）
- **触发**: 配置 base_url 后写 `GET /oauth/callback?redirect=https://api.example.com/done`（OAuth 回调参数极常见）→ 原样返回伪绝对 URL → reqwest 报 relative URL without base，请求失败且报错与根因无关
- **修复**: 改判 scheme 前缀（`^https?://` / `^wss?://`）而非子串
- **回归**: query 含 `://` 的相对路径正常拼接

### 2.9 [新·中高] 浮点相等误用 `f64::EPSILON`
- **位置**: `assertion/types.rs:177-178`（`(a-b).abs() < f64::EPSILON`）
- **后果**: EPSILON 是 1.0 附近的 ULP，非通用容差；`== 0` 对 1e-20 假相等、`!= 0` 假失败
- **修复**: 直接数值相等（serde_json Number 语义）或相对误差比较；回归：极小值断言

### 2.10 [新·中] SSE 断言按 `contains("stream.")` 子串路由误伤
- **位置**: `assertion/evaluator.rs:112,135,160`
- **触发**: SSE 文件中 `@assert headers.x-stream-id == "abc"` 或 `body.data.stream.x == 1` 被移出握手断言集、错配求值目标，可能假通过
- **修复**: 路由判断改为前缀匹配 `stream.llm.content`（精确路径前缀）而非任意子串
- **回归**: header 名含 `stream.` 的断言在 SSE 场景正确求值

**提交**: 每子项一个原子提交（10 个）；2.1 的共享函数与两处替换同一提交

---

## Phase 3 — Cookie 加固（P1-P2，约 1–1.5 天）

### 3.1 + 3.3 合并为一个提交：安全写盘序列重写（0600 权限 + TOCTOU + flush）[实测/代码]
- **现状三个缺陷**: ① `File::create` 0644 落盘 session cookie（实测 `-rw-r--r--`）；② 截断在拿锁之前（TOCTOU，并发读者见空文件→静默空仓库）；③ **[v2 新发现]** `BufWriter` 包装 `&file`，`save_incl_expired_and_nonpersistent` 不 flush，现状 unlock 后靠 Drop 隐式 flush 且忽略错误——本就存在"他进程拿锁后读到半截 JSON"窗口
- **修复**（单一 `save()` 重写）: `OpenOptions` 不截断打开 + `mode(0o600)`（`#[cfg(unix)]`）→ `lock_exclusive`（0.2 改名后为 `lock()`）→ `set_len(0)` + `seek(0)` → 写入 → **显式 flush → unlock**；已存在的旧文件写前 `set_permissions` 收紧
- **回归**: stat 0600（`#[cfg(unix)]`）；双进程并发写 N 轮终态校验 + 读者加载非空（"无空窗口"在 cargo test 中不可直接观测，验收降级为此）；现有 `test_cookie_persistence` 改写（它显式调 `after_response` 触发持久化，3.2 后该调用变 no-op）

### 3.2 保存粒度：移除每响应落盘 [代码] —— 非breaking，内部权衡
- **修复**: 删 `after_response` 的 `save()`；`CookieMiddleware::save` 改 pub(crate)，**调用点挂 `BatchExecutor` 的 per-file 边界**（TestExecutor 被全批次共享，Drop 在整批结束，必须在文件边界显式调）+ Drop 兜底
- **范围声明（v2 修正）**: 并行 worker 全为 `with_ephemeral_cookies()`（parallel.rs:275），`--cookie-file` + parallel **今天就没有落盘**，本项修复也只覆盖串行模式——CHANGELOG 与验收均限定串行
- **回归**: 跨运行持久化（现有测试改写后）

### 3.4 CliRunner 绕过环境隔离 —— 改动面为 CLI 扩展 [代码]
- **v2 修正**: 顶层 `Cli` 结构**没有 `--env` 参数**（cli.rs:11-40），`CliRunner::new` 不接收 env——修复前需先给默认请求路径补 `--env` 参数面；v1 的回归命令 `rupost g url ...` 有误（`g` 是 Generate 别名），更正为无子命令 curl 风格调用
- **修复**: ① 顶层 Cli 加 `--env`（global）；② `CliRunner::new` 传入并走 `resolve_cookie_path`
- **回归**: `rupost http://... --cookie-file ck.json -e dev` 读写 `ck_dev.json`

### 3.5 锁中毒 unwrap [代码] —— v2 修正引用
- **位置**: `cookie.rs:132,151,165`、`ws/session.rs` 11 处（**history/storage.rs 无 `lock().unwrap()`，v1 引用有误**，其文件锁均 map_err）
- **修复**: `lock().unwrap_or_else(|e| e.into_inner())`
- **同文件顺带**: `ws/session.rs` 的 `Result<_, String>` 错误类型（session.rs:66,101，完整性核查遗漏项）映射为 `RupostError`，并入本提交

### 3.6 钻石 DAG 多父 Cookie 警告 [代码]
- **修复**: `wait_for_deps` 检测多父 cookie_state 不同 → `warn!`（措辞修正：按**依赖列表顺序**的最后一个，非"最后到达者"）；jar 真合并另立 PRD
- **回归**: 钻石依赖确认警告输出

### 3.7 [补·遗漏项] 默认不持久化无提示
- **修复**: 默认模式（无 `--cookie-file`）时输出一次性提示（debug 级或 verbose 下）："会话 Cookie 为临时模式，加 --cookie-file 持久化"；README 补充说明
- **回归**: verbose 输出含提示

**提交**: 3.1+3.3 合并 1 个 + 3.2/3.4/3.5(含 session 错误类型)/3.6/3.7 各 1 个 = 6 个

---

## Phase 4 — 流式协议与文档一致性（P2，约 1–1.5 天）

> **执行顺序（v2 调整）**: 先做 4.0（原 Phase 6.2 前置），否则 4.1 会把 60 行重复块复制第三份。

### 4.0 SseRunner 去重（原 6.2 前置）
- 提取 `process_event()`，消除 `sse_runner.rs:151-215` 与 `221-281` 的逐行重复；4.1 因此变为 2 行

### 4.1 SSE 出错分支不 flush [代码]
- `Some(Err(e))` 分支 break 前调用 `flush()` 并走 `process_event()`

### 4.2 SSE 空数据帧派发（直接 breaking，无兼容开关）[代码]
- **修复**: 仅 `!current_data.is_empty()` 时派发；**WHATWG 细节（v2 修正）**: 空 data 时只重置 event type，**id（lastEventId）不重置**——v1 写"重置 event/id"过宽
- 已核查仓库零依赖旧行为（所有测试/示例的 SSE 帧均带 data 行；`data:` 空值仍会 push 空字符串进 Vec 照常派发，仅"零 data 行"行为变化）
- **回归**: 空 data 帧 → 不派发且 `sse_max_events` 计数不含它；`data:` 空值帧 → 照常派发

### 4.3 SSE 内存上限 [代码] —— v2 修正膨胀点
- **v1 错误**: `buffer` 只持有未换行残片，恶意流用百万行 `data: x` 不发空行时，膨胀的是 **`current_data` Vec**（stream.rs:17）
- **修复**: `current_data` 累计字节数与 buffer 双上限（各 1MB），超限 warn 并截断
- **回归**: 超限流不 OOM、有警告

### 4.4 markdown info string [实测]
- `markdown_file.rs:285` 改取第一个 token：`lang.split_whitespace().next()`；回归：```` ```http title="x" ```` 被识别

### 4.5 畸形 JSON 降级发送 [实测]
- `request_builder.rs:41` 的 `?` 改分支：JSON 解析失败 → `with_text` 原样发送 + 写入 2.4 的 warnings 通道（不新增语法；测试工具应能发恶意负载测 400）
- **回归**: `Content-Type: application/json` + 畸形 body → 原样到达服务器（wiremock 断言收到）

### 4.6 文档与死代码清理（合并一个 docs 提交）
- 删 `utils/security.rs`（`run_security_lint` 零调用方）；修正 `directory_structure.md`（path.rs 沙箱描述 → resolver.rs；routing.rs "密钥扫描"描述删除；"安全沙箱"措辞降格为"依赖路径合理性校验"）
- 动态变量状态更新：progress_summary / Pending_Features 改为已完成，**注意实现的变量名是 `$random_int`（下划线），文档旧写法 `{{$randomInt}}` 照抄会二次误导**
- README/guides 补全 7 个断言运算符清单（实际支持 `>= <= > < contains == != exists`）

**提交**: 4.0+4.1 同提交序列、4.2/4.3/4.4/4.5 各 1 个 + docs 1 个 = 6 个

---

## Phase 5 — 对抗性回归测试沉淀（贯穿，随各 Phase 提交）

**两层结构（符合 AGENTS.md）**: 纯函数修复的用例落各模块 `mod tests`（1.2 守卫、2.1 扫描、2.5 分词、2.6 路由、4.2/4.3 直接扩展 stream.rs 已有 mod tests）；需真实网络栈的 E2E 落 `tests/adversarial_regression_test.rs`（wiremock 已在 dev-dependencies，符合 Mock 隔离要求）。

八个测试域（验收按域绑定，非凑数）：

| 域 | 承载 | 关键用例 |
|---|---|---|
| status_code_edge | E2E | wiremock 700/999 → 失败结果；`@assert status == 700` 可通过 |
| assertion_edges | mod tests + E2E | `contains "<"/"a==b"/"<b>x</b>"`；**WS EXPECT `>=` 与引号内运算符** |
| header_validation | E2E | 空格名 → 错误结果；obs-text 值正常 |
| timeout_enforcement | E2E | wiremock `set_delay` 超时失败；**`@sse`+短`@timeout` 流不被切断** |
| sandbox_boundary | E2E | cwd 外路径可执行；mock 路径不误拒；穿越被拒 |
| capture_and_vars | mod tests + E2E | from_ 变量名、级联隔离、warnings 可见、`://` 相对路径 |
| cookie_hardening | E2E | 0600、并发终态、损坏恢复、环境隔离、依赖链传递 |
| streaming_protocol | E2E | Err 分支 flush、空 data 帧、内存上限、畸形 JSON 到达、info string |

每个测试注释内嵌**原始复现命令**（本次审查的实测步骤），保证可重放。

**提交**: 与所属修复同一原子提交（测试锁死修复），不设独立测试提交

---

## Phase 6 — 架构债（P3，独立排期）

1. `RunParams` 参数对象（4 处 `#[allow(too_many_arguments)]`；可选与 Phase 4 同期，弱推荐）
2. TUI 拆分：`write_log_with_limit`/`cleanup_old_logs_dir`/`VisualLineProcessor` 实际在 **tui/state.rs:803/838/848**（v1 误写 app.rs）→ 下沉 history/
3. rupost-ai 测试补强：v1"零测试"不实——translator.rs 已有 3 个测试，**真正无测试的是 client.rs**，改为补 client.rs 与 translator 覆盖
4. 杂项：anyhow 移除（cookie.rs 3 处 + formatter.rs + error.rs From impl）、`load_generator` 挪 examples/、`--env-file` 路径无效静默吞错（config.rs:96，随 2.7 的警告通道加提示）

---

## 执行顺序与里程碑

```
Phase 0 (0.5d) → Phase 1 (1d) → 【A: 可发布 crates.io】
                 Phase 2 (3d, 含3个新bug修复) → Phase 3 (1.5d) → 【B: 正确性与安全达标】
                 Phase 4 (1.5d, 4.0前置) → 【C: 全部问题闭环】
                 Phase 5 贯穿 | Phase 6 独立排期
```

**总计**: Phase 0–4 约 **7–8 个工作日、约 22 个原子提交**（v1 的 20+ 修正为：0.4 拆分 +1，3.1+3.3 合并 -1，新增 2.8–2.10 +3，3.7 +1）。

## 验收标准（v2 修订）

1. `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo clippy --all-targets` 零警告（写明完整命令，跨机器可复现）
2. `cargo fmt --check` 通过
3. Phase 5 八个测试域**每域至少 1 条且全部通过**（绑定域而非数量下限）
4. **10 个**实测 bug（Phase 1 三项 + Phase 2 七项 [实测] 标注）用测试注释中内嵌的复现命令重放，全部产生正确行为
5. cookie 文件 `stat -f "%Lp"` 为 600；`cargo tree | grep fs2` 为空；双进程并发终态校验通过
6. 文档与实现一致（README 运算符、directory_structure、progress_summary 含 `$random_int` 实名）
7. **Phase 4 行为验收**（v2 新增）：SSE Err 分支尾帧不丢；空 data 帧不派发且不计数；畸形 JSON 原样到达服务器；info string 正确路由
8. **流式豁免验收**（v2 新增）：`@sse` + 短 `@timeout` 场景流不被总时长切断
9. **战役归档**（v2 新增，AGENTS.md Stage 6）：完成后写入 progress_summary.md 与 checkpoint.md

## 风险与回滚

- **2.2 timeout + 流式豁免**是唯一可能引入新回归的修复 → 专项验收第 8 条 + 全量 `examples/` 回归
- **3.2 保存粒度**崩溃丢失窗口变化 → CHANGELOG 明示，并声明并行模式本就不落盘的范围事实
- **4.2 SSE 派发语义**直接 breaking → CHANGELOG 记录；无兼容开关（项目未发布，为不存在的用户维护死开关违反 YAGNI）
- **2.3 多根 allowlist** 放宽依赖可达范围至初始路径祖先树并集 → 措辞已降格为"合理性校验"，风险登记相应弱化
- 上游风险：aws-lc-sys 与 macOS 27 SDK 不兼容（本机需 SDKROOT 临时规避），建议发布前测试 reqwest 切 ring 后端
