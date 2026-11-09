# Rupost 开发规范（实战教训提炼版）

> **文档状态**: Active  
> **来源**: 2026-11-07/09 会话集群的实战教训提炼（每条规则锚定一次真实事故，标注 ⚓）  
> **定位**: [研发流程规范](development_workflow.md) 的执行层细则——流程规定"做什么"，本文件规定"怎么做才不踩坑"  
> **适用**: 全体智能体与人类开发者；与 AGENTS.md §5（CLAP 位置契约）同等级强制力

---

## 1. 提交纪律（jj 工作副本语义）

### 1.1 工作副本快照吸收防范 ⚓
***事故**：一个 `test(cli)` 提交静默吞掉了同工作副本内 1013 行未分段的重构变更（jj 会对整个工作副本做快照），事后需 `jj split` 拆三次才恢复原子性。*

- **规则**：开始任何新工作前，确认 `jj st` 显示工作副本为空或仅含本次工作内容；**多批次并行推进时，每批完成立即 `jj commit`，绝不跨批累积**。
- **规则**：`jj commit` 后用 `jj diff -r @- --stat` 复核提交内容与预期一致——jj 的快照语义会把工作副本内**一切**未提交变更（含其他会话残留）吸入提交。
- **规则**：路径拆分用 `jj split <filesets> -m "..."`，但要知道它对**当前** `@` 的 diff 生效——变更已被早期提交吸收时，需 `jj split -r <旧提交> <filesets>` 在历史内拆。

### 1.2 提交哈希引用的血统校验 ⚓
***事故**：checkpoint 把 Phase 4 交付记为前置契约测试提交的哈希（jj 重写产生同内容副本，diff 为空但非血统祖先）。*

- **规则**：文档引用提交哈希前，用 `git merge-base --is-ancestor <hash> HEAD` 校验血统；jj 重写历史后，同内容副本哈希**不是**有效引用。
- **规则**：`jj describe` 修订旧提交信息后，检查受影响的后代哈希是否已全部更新到文档。

### 1.3 提交信息与事实一致 ⚓
***事故**：提交信息声称"旧实现直接报错拒绝导入"，经临时工程实证 `http` crate 的 `from_str` ≡ `from_bytes`（该层改动是 no-op）；声称"两条断言"实为一条。*

- **规则**：提交信息中的**行为变更声明**必须有实证支撑——对第三方 crate 行为的断言（如 `HeaderValue::from_str` 是否拒绝某输入），用 /tmp 临时工程实测后再写。
- **规则**：数量词（"X 处"、"X 条"）写进信息前先 `grep -c` 实测；修完一处后 grep 全仓同类，确认没有第二处（大小写残留事故：修了 :238 漏了 :140）。

---

## 2. 重构与迁移纪律

### 2.1 编译器当迁移检查器 ⚓
***事故（正面）**：json.rs / commands/ / actions/ 三次大迁移零遗漏，靠的是"同提交改完全部引用、不加兼容 shim"——编译错误即迁移清单。*

- **规则**：类型/函数迁移**同提交改完全部引用点**（`grep -rn "旧路径" src/ tests/` 列清单后再动手），不加 `pub use` 兼容层；bin crate 内确需路径稳定但未被消费的重导出，显式 `#[allow(unused_imports)]` 并注明理由。
- **规则**：迁移提交信息中列明引用点清单（迁移后 grep 复核为零残留）。

### 2.2 glob 导入的隐性依赖显式化 ⚓
***事故**：`stream.rs` 顶部 `use super::*;` 依赖父模块私有 `use std::collections::HashMap`——父模块清理 import 时子模块立即断链。*

- **规则**：子模块经 `use super::*` 隐式获得的**非本模块声明**的名字（父模块的私有 use 绑定），在父模块整理 imports 时必须先审计：`grep -nE "\b(名字)\b" 子模块` 逐个确认来源。
- **规则**：新写的叶子子模块自带完整显式 imports，不依赖父模块的 use 泄漏。

### 2.3 机械 diff 验证等价性 ⚓
***事故（正面）**：发射层整块 dedent 后，"去缩进逐行机械 diff"证实零语义偏差；对照的目测审查曾漏判。*

- **规则**：整块搬移/dedent 后，用 `diff <(sed 's/^    //' 旧块) 新块` 或等效机械手段验证逐行等价；"看起来一样"不算验证。
- **规则**：dedent/整块替换后必跑 `cargo fmt`——机械操作会引入格式漂移（本会话三次 fmt 差异全部源于此）。

### 2.4 计划数字先实测 ⚓
***事故**：解耦提案 v1 的数字约半数失实（字段 46 实为 69、行数 537/552 实为 509/579、"TestArgs 结构体"实为内联字段、5-Tab 清单与真实枚举错位）——双子代理审查被迫全量校准。*

- **规则**：方案/提案中的每个可量化声明（行数、字段数、函数清单、测试数、枚举变体名）写之前用 `wc -l` / grep 实测；**概念性清单**（如"X 个 Tab"）必须对照真实枚举定义。
- **规则**：引用历史文档的表述（如 checkpoint 旧条目）不能直接沿用——旧条目可能已过时，以当前代码为准并勘误旧条目。

---

## 3. 测试纪律

### 3.1 守护测试映射真实调用路径 ⚓
***事故**：非 ASCII header 修复的守护测试走了 `CurlImporter::parse`，但 CLI 实际路径是 `run_curl_import → parse_curl_to_entry` 直连——测试红了也护不住生产路径；且同一 bug 在发射层（第三层）与序列化层（第四层）各有一份，只修/只测一层不闭环。*

- **规则**：写守护测试前先画调用链（生产入口 → 缺陷点），测试必须挂在**生产真实经过**的路径上；同一缺陷的多层副本（parse/读回/发射/序列化）逐层修、逐层测。
- **规则**：修复"静默丢值"类缺陷时，grep 全仓同类模式（`to_str().unwrap_or("")`、`if let Ok(to_str)`）一次列全，逐层处置或显式登记 backlog——"修一层"的提交信息不得声称"全链路"。

### 3.2 测试文件惯例先行 ⚓
***事故**：往子进程风格的测试文件里写了库 API 风格测试（`MultiFormatImporter::import`），编译失败返工。*

- **规则**：向既有测试文件追加用例前，先看文件头部 import 与既有用例形态（子进程 `Command::new(env!("CARGO_BIN_EXE_rupost"))` vs 库 API 直调），保持同构。
- **规则**：`crate::` 与 `rupost::` 的区分——src/ 内用 `crate::`，tests/ 集成测试用 `rupost::`（bin crate 的模块如 `cli` 只能从 bin 测试或子进程触达，`rupost::cli` 不存在）。

### 3.3 可能挂死的测试必须确定性失败 ⚓
***事故（审查发现）**：非回环拒绝测试在强制被移除时会 bind 成功并永驻 serve——以挂死形式变红，靠 CI 超时兜底。*

- **规则**：会 await 长驻操作（serve/循环轮询）的测试用 `tokio::time::timeout` 包裹，失败必须确定性、有时限。
- **规则**：轮询等待（poll 模式）必须有 deadline + 超时断言消息，不得裸循环。

### 3.4 红绿判定先推演 ⚓
***事故**：非回环守卫测试初版把"非法 Header 值"归因于非 ASCII（实为换行符）；覆盖分支的写入测试被审查判为假守护（删修复不红）。*

- **规则**：写测试前回答："删掉被守护的修复，这条测试会红吗？以什么机制红？"——不能回答的测试是假守护。
- **规则**：错误归因用排除法实证——报错文案指向 A 原因时，先构造排除 A 的最小用例再下结论。

### 3.5 大小写与格式口径一次修全 ⚓
***事故**：统一 `--report` 大小写口径时改了 3 处漏 1 处（app.rs:140 单请求提示门），`--report JSON --only 3` 的 stdout 仍被污染——CHANGELOG 声明先于事实。*

- **规则**："统一口径"类修复动手前：`grep -rn '比较模式' src/` 列出**全部**比较点建清单，逐个修、逐个锁测试；CHANGELOG 声明在清单清零后才成立。
- **规则**：格式/旗标语义用表驱动决策测试锁定（先例：`test_global_args_position_contract_table`、`test_determine_report_format_decision_table`）。

### 3.6 覆盖目标按 crate 区分 ⚓
***事故（审查发现）**：验证纪律只列 `cargo test --lib`，而 cli 属 bin crate——12 个单测（含位置契约表）完全裸奔。*

- **规则**：lib 侧改动验证 `cargo test --lib <filter>`；bin 侧（cli/main）改动必须 `cargo test --bin rupost`；涉及子进程集成测试时 `cargo test --workspace` 全量。

---

## 4. 字符串与编码纪律

### 4.1 `to_str()` 静默丢值点全景处置 ⚓
***事故**：非 ASCII header 值在 parse/读回/发射/序列化四层各有丢值形态（报错拒绝/空串/跳过渲染/空串），分三个批次才修全。*

- **规则**：`HeaderValue::to_str()` 只对可见 ASCII 成功——遇非 ASCII 值的**正确默认是 `String::from_utf8_lossy(v.as_bytes())`**（无损或显式可见），`unwrap_or("")` 与 `if let Ok` 跳过均为静默丢值，禁止新增。
- **规则**：新增/触碰含 `to_str()` 的代码时，确认该点已在 future_backlog 的丢值点盘点中处置（修复或显式登记）。
- **规则**：`from_bytes` 允许 obs-text（0x80+）但拒绝控制字符（<0x20、0x7F）——header 注入防线依赖后者，改动构造方式时注入测试必须同行提交。

---

## 5. 行为变更与文档同步

### 5.1 行为变更三件套 ⚓
***事故**：空扫描路径下报告落盘语义漂移（短暂改为写空报告），无 CHANGELOG、无测试，被审查捕获后才恢复。*

- **规则**：任何用户可感知的行为变更（含"顺手"的隐式变更）必须同时具备：CHANGELOG 条目 + 锁定测试 + 提交信息声明。三者缺一即未完成。
- **规则**：CHANGELOG 声明的强度不超过事实——"全仓清零"类措辞先 grep 反例（"清零"被一条子进程豁免推翻过）。

### 5.2 注释与实现同步 ⚓
***事故**：catch 分支注释写"回落默认暗色"而实际回落 'light'；MockCompiler 注释"过滤非测试块"在语义变更后失真两轮。*

- **规则**：改行为必改注释——注释描述的是**当前**行为，不是编写时的行为。
- **规则**：删除代码时同步删除/更新其注释（MockCompiler 死代码的注释在删除后一段才修正）。

---

## 6. 安全姿态纪律

### 6.1 新暴露面 fail-closed ⚓
***事故（正面）**：--dev CORS 三层姿态（非回环 fail-fast 于 bind 前 + 横幅 + Host 守卫叠加）经七种 host 形态独立程序实测确认 fail-closed 无绕过。*

- **规则**：任何放宽类开关（CORS/写路径/白名单）必须：默认关闭；强制校验早于资源绑定；校验失败信息含可执行指引；威胁描述点名真实威胁（"浏览器中任意网页可跨域读写"而非模糊的"请勿用于共享环境"）。
- **规则**：白名单/匹配函数的边界输入（变体写法、编码形态）用独立程序实测——matches! 文本命中 ≠ bind 行为符合预期。

### 6.2 横幅与校验的时序 ⚓
***事故（审查发现）**：--dev 横幅在校验前打印，非回环拒绝时输出"已启用→报错"的自相矛盾序列。*

- **规则**：状态宣告类输出放在**全部校验通过之后**；拒绝路径的输出只有错误本身。

---

## 7. 审查协作纪律（子代理协议）

***经验**：本会话五轮子代理审查，两轮高效（机械 diff + 实证）、一轮超时、一轮发现审查者自身的假守护。*

- **规则**：审查任务书必须给出**可机械执行的验证任务**（"逐字段对比"、"去缩进 diff"、"临时工程实证 crate 行为"），而非"看看有没有问题"。
- **规则**：审查代理任务书**禁止跑全量测试套件**（编译超时会挂死代理）——门禁由主会话跑，代理专注语义与 diff。
- **规则**：审查发现按 P0/P1/P2 分级落地，P1 以上当批修复；对审查者发现的"该修未修"（如本会话 :140 残留），修复提交本身也要过一遍同样的清单思维。
- **规则**：审查范围用显式 revset 区间（`git log <已审点>..HEAD`）界定，避免重复审或漏审。

---

## 8. 门禁清单（每批次收尾）

```bash
cargo test --workspace          # 全量（含 bin 与子进程集成）
cargo clippy --workspace --all-targets -- -D warnings   # 零警告
cargo fmt --all -- --check      # 格式（机械编辑后必跑）
```

补充项（按改动面）：
* 触及 bin crate（cli/main）→ `cargo test --bin rupost`
* 触及 web 前端资产 → 手工冒烟 + 确认 `cargo build` 嵌入成功
* 触及 TLS/依赖版本 → `examples/run_all.sh`（唯一 TLS 探针）
* 机械批量编辑（perl/python）→ `cargo fmt` 后再 `--check`
* 文档哈希引用 → 血统校验（§1.2）

---

## 9. 跨平台兼容与路径安全（POSIX 路径纪律）

### 9.1 暴露路径与断言严格 POSIX 归一化 ⚓
***事故**：Windows CI 跑 `web::store::tests::test_local_fs_document_store_basic_and_sandbox` 和 `tests/batch_testing_test.rs:test_batch_report_json` 报断言失败（`api\\spec.md != api/spec.md` 与 `\\?\C:\... != //?/C:/...`）。原因是局部手工 `.replace('\\', "/")` 存在疏漏；而沙箱 `PathSandbox::resolve_safe_path` 防逃逸严格拒绝 `\` 输入，全库暴露给外部（Web、API、TUI、JSON Report、Snapshot）的路径必须统一为 POSIX 正斜杠。*

- **规则**：禁止在业务层手写 `.replace('\\', "/")` 或 `to_str()` 拼接路径。所有对外暴露（Web API、TUI 视图、JSON Report、Snapshot、导出器）和跨平台比对的路径必须统一调用 `crate::utils::path_guard::normalize_posix_path` 单一事实来源（SSOT）。
- **规则**：集成测试与单测断言跨平台路径时，禁止裸拿 `path.to_str().unwrap()` 直接比较，必须先经 `normalize_posix_path` 规整。
- **规则**：实时文档判定必须统一由 `crate::utils::path_guard::is_live_doc` 与 `LIVE_DOC_EXTENSIONS` 判定（收敛 `.http`、`.md`、`.markdown`），消解散落的扩展名分支匹配。

---

## 10. 协议抽象与领域模型内聚（SSOT 与防 Panic 纪律）

### 10.1 字符串安全截断（防 Panic 契约） ⚓
***事故**：`history/printer.rs` 对短 ID 裸切片 `&entry.id[..8]`，在非标准短 ID 下直接越界 panic；此外多字节 UTF-8 字符（如中文）按 byte 切片会在 char boundary 内部触发 panic。*

- **规则**：字符串按字符数截断统一使用 `crate::utils::text::truncate_chars(text, max_chars, suffix)`；按字节安全截断统一使用 `crate::utils::text::truncate_utf8_bytes(text, max_bytes, suffix)`。
- **规则**：**绝对禁止使用裸切片 `&s[..N]`**，除非先经过显式 `s.is_char_boundary(N)` 或 `s.char_indices()` 保护。

### 10.2 HTTP 谓词与常量集合的 SSOT 与零分配比较 ⚓
***事故**：`cli/parser.rs` 与 `importer/httpie.rs` 分别手写 `let methods = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];`，并通过 `to_uppercase()` 频繁申请堆内存。*

- **规则**：HTTP 方法判定与列表由领域层 `crate::http::Method` 统一持有（`Method::STANDARD_METHODS` 与 `Method::is_standard`），禁止在外部模块散落硬编码数组。
- **规则**：字符串大小写不敏感判断统一采用 `eq_ignore_ascii_case`，禁止先调用 `.to_uppercase()` 或 `.to_ascii_lowercase()` 制造额外的堆内存分配。

### 10.3 URL Scheme 识别的 RFC 3986 规范性 ⚓
***事故**：全库散落手写粗糙的 `.starts_with("http://") || .starts_with("https://")`，漏判大写 `HTTP://`、IPv6 格式，且无法区分通用 Scheme 与仅 HTTP 协议，并在相对路径含 query 时（如 `/oauth/cb?redirect=https://x`）误判。*

- **规则**：判定合法 Scheme 统一使用 `crate::http::url::has_scheme_prefix`（严格符合 RFC 3986 语法）；判定 HTTP/HTTPS 协议前缀统一使用 `crate::http::url::has_http_scheme`，禁止在业务代码中手工写 `.starts_with("http")`。
- **规则**：环回地址解析统一使用 `crate::http::url::is_loopback_host`，支持 FQDN 尾随点（如 `localhost.`）、方括号 IPv6 与裸 IPv6（如 `::1`）。

### 10.4 领域模型充血内聚（Rich Domain Model 优先于裸闭包） ⚓
***事故**：`ParsedRequest` 缺少语义查询方法，导致导出器、HTTP 运行时、Mock 模块各自手写 4-5 行的遍历闭包解析 `Accept: text/event-stream` 与 `Content-Type`。*

- **规则**：核心实体上的常见查询属性必须在实体上实现领域方法（如 `parsed.is_sse_stream()`, `parsed.content_type()`, `parsed.has_header()`），避免在外部散落嵌套遍历闭包。
- **规则**：流式事件日志与有界落盘统一由 `crate::runner::utils::file_sync::write_log_with_limit` 托管，包含父目录自动探测、上限探测、截断标记与 flush 保护。禁止在协议 Runner 内部复制底层文件 I/O 逻辑。

---

## 11. 离散逻辑治理与 DRY 规范 (Single Source of Truth)

### 11.1 业务知识的单一性（DRY 的第一性原理） ⚓
***事故**：实时文档后缀（`.http`/`.md`/`.markdown`）、HTTP 标准方法名、SSE 协议头嗅探在全库多处存在平行的局部判定，当新增需求或修复边界缺陷时，漏改率高达 60%，破坏开闭原则。*

- **规则**：**DRY 的核心是“系统知识的单一性”，而非仅仅消除重复的代码行**。任何业务规则、合法值集合、格式规范在代码库中必须有且仅有一个**权威定义点（Single Source of Truth, SSOT）**。当规则发生变更时，只需修改该权威点，全库自动同步。
- **规则（坏味道排查清单）**：
  1. **字面量数组重复**：2 处以上出现相同的字面量数组（如 `["GET", "POST", ...]`、`["http", "md", ...]`）→ 必须收敛为关联常量或枚举。
  2. **手写字符串前缀/替换**：散落的 `.replace('\\', "/")` 或 `.starts_with("http")` → 必须提升为纯函数并锁定表驱动单测。
  3. **同构私有 I/O 函数**：在多个不同模块中实现结构相似、仅提示文案微调的私有文件写入/解析代码（如 `write_ws_log_with_limit` 与 `write_sse_log_with_limit`）→ 必须下沉到底层工具层统一复用。
  4. **穿透实体字段做遍历计算**：外部调用方频繁解构 `entity.headers.iter().find(...)` → 必须为实体提供富有表达力的领域方法（Rich Domain Model）。

---

## 12. TDD 驱动重构与行为契约锁定规范 (Test-Driven Refactoring)

### 12.1 契约先行锁定（Red-Green-Refactor 纪律） ⚓
***事故**：在未编写完备表驱动测试的情况下直接修改 `is_loopback_host` 与 `has_scheme_prefix`，导致 `::1` 裸 IPv6 与 FQDN 尾随点漏判，在 CI 阶段才暴露。*

- **规则**：**在对任何既有离散逻辑进行提取、收敛或重构之前，必须先编写测试用例锁定当前行为边界与预期契约**。
- **执行协议 (三阶段闭环)**：
  1. **阶段 1 (Red 先行锁定)**：编写表驱动测试（Table-driven tests），显式包含正向合法值、逆向非法值、边界极值（空串、纯符号、截断点、多字节 Unicode、跨平台路径分隔符、大小写变体）。
  2. **阶段 2 (Green 纯粹实现)**：在唯一的 SSOT 模块中完成极简、无堆内存浪费的高性能实现，使新测试 100% 变绿。
  3. **阶段 3 (Refactor 批量替换与孤儿清理)**：将散落在全库的所有离散调用点逐一替换为调用 SSOT。每替换一处，运行相应模块测试；替换完成后，**彻底删除被取代的私有辅助代码、未使用的导入与旧有死代码（Zero Orphan Code）**。
- **规则（反假守护校验）**：编写守护测试后必须自问：“若删掉/反转核心修复逻辑，本测试能否确定性失败？”无法明确回答或删改实现后仍全绿的测试均属**假守护**，禁止提交。

---

## 附：本文件维护纪律

* 每条规则必须锚定至少一次真实事故（⚓ 标记）；无事故锚点的通用建议放 future_backlog。
* 新事故提炼时增补对应章节；规则与 AGENTS.md 冲突时以 AGENTS.md 为准并回改本文件。
