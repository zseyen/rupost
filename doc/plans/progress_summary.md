# RuPost 进度汇总 (Progress Summary)

本文件作为 RuPost 项目唯一的开发进度事实来源，记录已完成的阶段任务、未完成的计划以及未来的架构路线。

---

## 进度概览

| 阶段 / 功能模块 | 目标与核心特征 | 状态 | 交付物 |
| :--- | :--- | :--- | :--- |
| **Sprint 1: 核心重构与优化** | 引入 User-Agent 动态配置与 Request 级覆盖，收敛 Cookie 管理管道 | 已完成 | `src/http/client.rs`, `src/runner/executor.rs`, `tests/user_agent_test.rs` |
| **Sprint 2: 多文件与目录递归测试** | 递归扫描过滤、有向无环图 (DAG) 拓扑排序、顺序/并行双执行模式、并发 Cookie 隔离保护、Fail-Fast 流程中断、JSON 结构化报告、递归依赖补全与安全沙箱、并行状态克隆传递 | 已完成 | `src/runner/scanner.rs`, `src/runner/workflow.rs`, `src/runner/batch.rs`, `src/runner/resolver.rs`, `src/runner/parallel.rs`, `tests/batch_testing_test.rs` |
| **生产调试 (阶段 1)** | 本地局部环境变量级联覆盖与热重载，默认合并 `.env` 且支持 `--env-file` 传递 | 已完成 | `src/variable/env_file.rs`, `src/variable/config.rs` |
| **生产调试 (阶段 2)** | 网络连通性分析与高亮诊断工具 (`rupost diagnose <url>`)，精确测量 DNS/TCP/TLS/TTFB 时延，支持 X.509 证书解析与状态高亮输出 | 已完成 | `src/http/diagnose.rs`, `src/cli.rs`, `src/main.rs`, `tests/diagnose_integration_test.rs` |
| **生产调试 (阶段 7)** | 模糊路径匹配与条件变体独立 Mock 服务器，支持 Trie 树模糊匹配与多变体分支判断，可插拔的 Axum 网络适配器及高雅控制台高亮访问日志 | 已完成 | `src/mock/`, `src/cli.rs`, `src/main.rs`, `tests/mock_integration_test.rs` |
| **Clean Architecture 架构重构** | 彻底解耦 `src/parser` 与核心业务逻辑，下沉 HTTP 转换及 Mock 编译器，符合开闭原则 (OCP) | 已完成 | `src/parser/`, `src/http/request_builder.rs`, `src/mock/compiler.rs` |
| **Sprint 4: 大模型流式调试与初始化脚手架命令** | 自动规整 `stream.llm.content`、非阻塞物理文件增量同步（`@stream_to`）、内置 Mock 大模型服务（`MockLlmServer`）、路由改写与密钥扫描、一键项目与模板初始化命令（`rupost init`，支持别名 `template`/`i`），支持 `.http` 与 `.md` 后缀自适应与安全覆盖预警。 | 已完成 | `src/template/`, `src/http/llm_adapter.rs`, `src/runner/file_sync.rs`, `src/middleware/routing.rs`, `tests/template_test.rs`, `tests/llm_mvp_test.rs`, `templates/config/rupost.toml` |
| **元数据注释前缀兼容与运行脚本** | 兼容 `# @` 与 `// @` 风格元数据，编写一键测试 examples 的 run_all.sh 脚本 | 已完成 | `src/parser/http_file.rs`, `examples/run_all.sh` |
| **Sprint 5: WebSocket 协议测试与调试** | 支持在 `.http`/`.md` 中以 `@websocket` 指令声明长连接，识别 `SEND` / `EXPECT` 流式剧本；提供双层后台心跳保活 Worker 协程，支持 MsgPack 二进制解码断言与级联捕获。 | 已完成 | `src/ws/`, `src/runner/ws_runner.rs`, `tests/websocket_integration_test.rs`, `examples/websocket.http`, `examples/websocket.md` |
| **HTTP 快照录制与原样重放 (MVP)** | 增加测试时一键录制快照开关 `--save-snapshot`；新增顶层 `replay` 子命令与 `ReplayExecutor`，支持指定 `--target` 参数自动改写 Host/Port，对状态码进行比对高亮输出 | 已完成 | `src/cli.rs`, `src/main.rs`, `src/runner/replayer.rs`, `src/runner/types.rs`, `tests/snapshot_replay_test.rs` |
| **架构重构与组件化分层** | 将 `httpie` 和 `curl` 的解析逻辑从 `CliRunner` 中抽离为独立的 `src/cli/parser.rs` 子组件；将测试文件 URL 智能拼接与补全算法从 `TestExecutor` 中抽离为独立的 `src/runner/url.rs` 子组件，降低模块耦合。 | 已完成 | `src/cli/parser.rs`, `src/runner/url.rs` |
| **Sprint 7: TUI 引导、自适应与交互重构** | 实现底栏操作提示指示（lazygit 风格）、自适应窄屏三栏布局、鼠标点击捕获与面板切焦、以及同步预览与切焦编辑（Enter键脏确认拦截）。 | 已完成 | `src/tui/app.rs`, `src/tui/event.rs`, `src/tui/ui/mod.rs`, `tests/tui_smoke_test.rs` |
| **TUI 性能优化与交互升级** | 内存预折行 (Pre-wrapping) 视觉切片算法，解决中英文混合/Emoji 超长 Response 滚动错位问题；侧边栏双 Tab (Files / History) 状态解耦，提供滚动视口边界保持 (Viewport clamping)；历史记录项 Enter 反向还原为 `.http` 代码重新载入编辑器运行；组件化拆分及 Wide/Narrow/Stacked 自适应重绘冒烟测试。 | 已完成 | `src/tui/state.rs`, `src/tui/app.rs`, `src/tui/ui/sidebar.rs`, `src/tui/ui/editor.rs`, `src/tui/ui/response.rs`, `src/tui/ui/mod.rs` |
| **TUI v3.3 加固迭代 Phase 1：安全防线与写路径端口化** | 快速新建用例三防线（工作区围栏拒路径穿越、io 错误浮出状态栏、超长文件名截断+哈希后缀）；写路径 I/O 经 `FileRepository` 端口（Fs/InMemory 双实现，`create_exclusive` 原子创建消灭 TOCTOU）；URL 归一化统一到 `http::url::normalize_input_url`（CLI/TUI 同一语义，支持 `:3000` 缩写与端口命名）；quick input 换 ratatui-textarea + `InputMode` 互斥模态（真实光标替代伪 `█`）；`state.rs` 拆出 `state/{tree,layout}` 首片。 | 已完成 | `src/tui/ports.rs`, `src/tui/files.rs`, `src/utils/path_guard.rs`, `src/http/url.rs`, `src/tui/state/`, `tests/tui_smoke_test.rs` |
| **TUI 组件化与函数式拆分（handlers/ + state/ 全域）** | handlers.rs（931 行）拆为 8 组件 + `nav.rs` 纯函数层（面板/详情 Tab 轮转表、滚动数学、鼠标 `hit_test` 纯几何消灭 Wide/Narrow/Stacked 三份复制粘贴）；state 剩余域拆出 `stream/history/wrap/retention`（mod.rs 851→519 行）；"向心回弹"光标重定位改为 `relocate_selection` 纯函数（`std::iter::successors` 祖先链）；非测试通配导入清零；`relocate_selection` 表格测试 + `nav` 决策表测试补齐（45 单测/39 冒烟/457 全量绿）。文档对齐：keybindings.md 标注实现状态并修正 `n` 键、tui_iteration_v2 标注已交付、usage_and_testing 补快速输入章节与真机冒烟清单、CHANGELOG 登记 TUI 修复。 | 已完成 | `src/tui/handlers/`, `src/tui/state/`, `doc/tui/`, `CHANGELOG.md` |
| **TUI Phase 2/3 执行：合流 + retention 端口化 + `/` 过滤搜索** | rupost-tui 11 提交链 rebase 回主工作区（jj 自动合并，双回归 459 绿）；chmod 权限测试加 `#[cfg(unix)]` 门控（Windows CI 预修）；`write_log_with_limit` 端口化——`log_decision` 纯函数（Skip/AppendTruncated/Append 决策表）+ `FileRepository::file_len/append` 扩展（state/ 数据写债务就此清零，目录创建保留为 FS 准备步骤）；`/` 实时过滤——`InputMode::Filter` + `filter_matches` 纯谓词（Trie 插入前过滤，祖先目录自动保留）+ `set_filter` 重置选中，Esc 清除，侧栏 Files 标题显示活动过滤（复审修正：入口键在首版提交中被 fmt 吞掉，本片补接线）；459 测试/39 冒烟/双脚本全绿。P2 主题（93 处 Color:: 盘点）与 P3 设置持久化按"每特性可砍"规则留档。 | 已完成 | `src/tui/ports.rs`, `src/tui/state/retention.rs`, `src/tui/state/tree.rs`, `src/tui/handlers/`, `src/tui/ui/mod.rs` |
| **TUI P2 主题整理（93 处字面色槽位化）** | 新增 `tui/theme.rs`：9 语义槽位（accent/accent2/info/dim/text/ok/err/status_bg/status_fg）× 明暗双调色板，Dark 逐槽锁定主题化前原值（行为零变化），Light 全槽暗化保浅底可读；ui 四文件 93 处 `Color::` 字面色全部替换为槽位取色，字面色清零（仅存于 theme.rs）；`,` 全局键切换（模态内为输入字符不触发），帮助面板补条目；TestBackend smoke 矩阵加 theme 维度（2 主题 × 3 错误态 × 3 布局 × 3 Tab × 3 面板）。465 全量绿。 | 已完成 | `src/tui/theme.rs`, `src/tui/ui/`, `src/tui/handlers/` |
| **Sprint 6: HTML 报告与高级表现层** | 导出可视化 HTML 报告与模板表现层 | 未开始 | - |
| **质量加固战役 Phase 0-4**（2026-10） | 三轮审查（代码质量/对抗性测试/Cookie 评估）+ 子代理评审发现的 25 项问题全部闭环：3 个 panic 修复、断言运算符最左扫描（含 WS `EXPECT` 运算符补全）、`@timeout` 接线与流式豁免、沙箱多根 allowlist、capture 逐条化 + `TestResult.warnings`、未定义变量警告、URL `://` 误判、SSE 出错分支 flush/空 data 帧语义（WHATWG，breaking）/内存上限、markdown info string、畸形 JSON 降级发送、Cookie 全套加固（0600/落盘粒度/TOCTOU/内联 `--env` 隔离/锁中毒/DAG 告警）、`--env` 路径注入校验、JSON 报告 warnings、死代码清理（`security.rs`/`metadata_types.rs`） | 已完成 | `tests/adversarial_regression_test.rs`、`doc/plans/2026-10-18-quality-hardening-execution-plan.md`、`CHANGELOG.md` |
| **表现层与基础设施下沉 (方案二)** | 将表现层 `reporter.rs` 移入库层 `src/reporter.rs`，`url.rs` 下沉至 `src/http/url.rs`，`scanner.rs` 下沉至 `src/parser/scanner.rs`，`gc.rs` 下沉至 `src/history/gc.rs`，实现核心业务与基础设施、表现层的彻底解耦。 | 已完成 | `src/reporter.rs`, `src/http/url.rs`, `src/parser/scanner.rs`, `src/history/gc.rs` |
| **二级子域目录划分与收拢 (方案一)** | 将 `src/runner` 下平铺的批量测试、并行调度、工作流拓扑、依赖补全、快照重放等文件，整理进 `src/runner/engine/`、`src/runner/protocol/` 和 `src/runner/utils/` 二级子目录下，注册各级 `mod.rs` 并通过 `src/runner/mod.rs` 桥接重导出。 | 已完成 | `src/runner/engine/`, `src/runner/protocol/`, `src/runner/utils/`, `src/runner/mod.rs` |
| **关键逻辑单元测试与回归校验** | 为下沉至 `src/http/url.rs` 的 URL 解析逻辑新增 `test_resolve_final_url_edge_cases` 单元测试，覆盖 camelCase 优先级、根路径拼接和 WS/WSS 协议原样返回等场景，跑通全量 240+ 测试及端到端冒烟。 | 已完成 | `src/http/url.rs`, `tests/verify_features.sh` |
| **统一断言与协议执行链解耦重构** | 将断言求值从 Response 解耦（AssertTarget），支持 WsFrame 复用断言；剥离 executor.rs 里的 HTTP 执行流到 HttpRunner，使用静态分发 Runner 消除硬编码分流 | 已完成 | `src/assertion/types.rs`, `src/ws/matcher.rs`, `src/runner/http_runner.rs`, `src/runner/executor.rs` |

---

## 已交付细节

### Sprint 1: 核心重构与优化
1.  **动态 User-Agent 配置**：默认发送 `rupost/1.0.0` 作为 UA。支持从 `VariableContext` 读取全局 `user_agent` 变量并覆盖，以及在用例中显式使用 `User-Agent` 头部进行最高优先级覆盖。
2.  **Cookie 状态管理收敛**：剥离并封装 `CookieMiddleware` 交互，TestExecutor 不再硬编码 cookie store 的细节，完全符合 Clean Architecture。

### Sprint 2: 多测试文件与目录递归测试
1.  **DirectoryScanner**：支持递归遍历多个目录，自动过滤 `.http`/`.md`，主动剔除 `.git/`, `.rupost/`, `target/` 等隐藏或无关路径，保证按字典序升序。
2.  **WorkflowGraph (DAG & Kahn)**：解析用例文件头部的 `### @depends-on <filename>` 属性，使用 Kahn 拓扑排序算法计算执行顺序，并具备高强度的循环依赖环路自检。
3.  **DependencyResolver & 安全沙箱 (Sandbox Scope Jail)**：自动递归加载并补全缺失的依赖测试文件；强制校验依赖绝对路径必须位于沙箱目录范围内，并进行后缀合法性审查，切断路径穿越（Path Traversal）安全漏洞。
4.  **BatchExecutor**：
    *   **顺序模式**：按拓扑序串行运行，变量及 Cookie自然传递。
    *   **并行模式**：利用 `tokio::task::JoinSet` 及并发限制器进行并发处理。
    *   **状态克隆传递 (State Cloning)**：在拓扑依赖链上，实现从前置节点向子节点单向克隆并合并 Context 变量增量与序列化还原的会话 Cookie，彻底消除并行开发的数据孤岛，而非依赖分支依然维持安全隔离。
    *   **Fail-Fast 控制**：支持随时在串行或并行模式下检测到错误请求时中断其余用例。
5.  **结构化 JSON 报告与批汇总**：支持 `--report json` 格式的控制台或文件输出；支持终端高颜值批测试摘要输出。
6.  **JSON Path 数组数值下标提取支持**：支持以点号跟数字的形式直接从 JSON 数组中按数值索引定位元素，解决 `body.headers.Cookie.0` 类路径取值失败的问题。
7.  **批量并发复杂测试场景验证（DAG）**：构建并跑通了包含 Fork 与 Join 等多维依赖拓扑的批量并发集成用例集（`examples/batch_complex`），针对 Cookie 字段顺序随机抖动进行 `contains` 断言优化，成功实现高并发下无感级联。

### 生产调试：生产环境调试与排障模式 (阶段 1 & 2 & 7)
1. **本地局部变量覆盖 (阶段 1)**：
   * **解析器实现**：支持对本地被 git 忽略的 `.env` 环境变量文件进行高健壮度解析，自动剥离引号、忽略行内注释与前导/尾随空格。
   * **级联优先级控制**：在 `ConfigLoader` 内部建立了明确的覆盖层次顺序（`CLI变量覆盖 --var` > `系统环境变量` > `本地局部 .env 变量` > `共享 rupost.toml 环境配置`），避免本地调试变量泄露或覆盖他人共享配置。
2. **网络高亮诊断工具 (阶段 2)**：
   * **细粒度时延瀑布图**：基于第一性原理，利用 `lookup_host`、`TcpStream` 和 `tokio-rustls`（使用 `ring` 提供密码库）执行手动握手与连接，精准拆分 DNS 解析、TCP 握手、TLS 协商及 HTTP TTFB（Time To First Byte）的时延表现，在终端绘制占比条形图。
   * **X.509 证书深度分析**：通过 `x509-parser` 直接解构服务器证书的 Validity 期限，算出证书剩余有效天数，对于到期天数临期（<=30天）的场景显示黄色警告 `[WARNING]`，已过期的场景显示红色 `[EXPIRED]`，同时解析并显示 Issuer 与 Subject SAN 域名信息。
   * **独立命令行分发**：新增子命令 `rupost diagnose <url>`（别名 `rupost d <url>`），完美兼容已有的 `test`、`history`、`generate` 模块。
3. **模糊路径匹配与条件变体 Mock 服务器 (阶段 7)**：
   * **可插拔 Axum 适配器**：将底层网络通信（Axum 服务端）与匹配调度引擎核心剥离，网络框架成为可插拔插件，彻底遵循 Clean Architecture。
   * **Trie 树模糊匹配**：自主实现 Trie 匹配树，完全支持精确匹配、路径参数捕获（如 `:id` 自动提取至 Context）和通配符匹配（`*`, `**`），实现复杂路由映射。
   * **多路分支条件匹配 (Variant)**：基于同一路由支持多个 `MockVariant`，并支持通过 Header、Query、Body 内 JSONPath 的 Equals/Contains/Exists 条件自动计算路由变体分发，且支持变量占位符的动态渲染。
   * **CLI 集成与终端高亮访问日志**：新增 `rupost mock start <file> --port <port>` 命令行工具，智能解析反序列化格式（`untagged` 兼容自定义 JSON 配置与快照包），在控制台以高雅彩色打印实时流量访问与匹配命中信息。

### Clean Architecture 架构重构 (解耦 Parser)
1. **物理依赖完全清除**：彻底消除 `src/parser/` 对 `src/http/` 和 `src/mock/` 目录的任何反向引用 (`use crate::http::*` 或 `use crate::mock::*`)，使 Parser 只依赖核心 AST 实体与标准库。
2. **HTTP 转换逻辑下沉**：将 `TryFrom<ParsedRequest> for Request`、`add_body`、`is_json_like` 和 `to_request` 整体物理下沉至外层基础设施的 `src/http/request_builder.rs`，并在 `src/http/mod.rs` 中重新注册和导出。
3. **Mock 编译器下沉**：将 `MockCompiler`、条件表达式解析方法 `parse_condition_expression` 及对应单元测试整体物理迁移至外层基础设施的 `src/mock/compiler.rs`。
4. **全局调用与测试对齐**：修改 `src/main.rs` 和 `tests/mock_integration_test.rs` 中的 `MockCompiler` 调用和导入路径，对齐到 `rupost::mock::MockCompiler`，并通过了全部 163 个单元测试和集成测试校验。

### 启发式自适应请求拆分 (Heuristic Auto-Split)
1. **状态感知型请求拆分**：重构了 `split_by_separator` 模块，摒弃单纯依赖 `###` 分隔符的做法。通过在扫描时引入 `has_req_line`、`last_req_method` 以及空行感知逻辑，自动并正确切分漏写 `###` 的多用例。
2. **高强度的 Body 包含 HTTP 谓词容错**：在 POST/PUT 等带 Body 请求的方法中，针对 Body 内部顶格书写 `GET http://...` 等容易误切的边缘场景，利用状态机加以自动识别和规避，实现了极高的工业级容错和精准度。
3. **元数据关联修正**：将切分元数据的触发词收窄至 `@name` 和 `@test`，确保写在请求末尾的 `@assert`、`@capture` 等指令能够无感且正确关联到其宿主请求，不发生错误切割。

### 统一 JSONPath 评估引擎
1. **核心 `JsonPathResolver` 抽象**：下沉和抽象出独立的公共模块 `src/utils/jsonpath.rs`，实现统一的分词提取器 `parse_jsonpath_to_segments` 和求值器 `JsonPathResolver::resolve`，彻底消除了断言与 Mock 两端解析行为的不对齐。
2. **支持点号、中括号与根数组索引混合解析**：支持点号数字索引（`a.0.b`）、中括号索引（`a[0].b`）、单双引号转义剥离键名（`a['first name']`）以及根数组定位（`$[0]`），极大提升了用例表达的灵活性。
3. **全平台两端逻辑对齐**：通过重构 `src/assertion/parser.rs` 和 `src/mock/variant.rs`，两端底层均调取统一组件。断言端额外获得 `body.items[0]` 语法支持，Mock 端额外获得 `$.items.0` 和根数组匹配支持，双向赋能。

### 标准化 None (空值) 语义
1. **统一空值词汇转换**：在断言语法解析器 `parse_assert_value` 中，将未带引号的 `None`、`undefined`、`nil`、`NULL` 统统映射为新增的 `AssertValue::None`，并与原有 `AssertValue::Null` 做比较矩阵上的对齐（`null == None` 为真），同时区分开带双引号的 `"None"` 普通字符串。
2. **防崩溃路径缺失校验**：在断言求值器 `evaluate_assertion` 中，针对 `extract_value` 提取失败返回 `PathNotFound` 的场景，若断言期望值是 `None` 或 `null`，则主动将其拦截并纠正为 `AssertValue::None` 进行比较，从而使 `@assert body.password == null` 类型的字段缺失校验能优雅通过。
3. **Mock 变体匹配泛化**：引入 `is_none_value` 判定辅助函数并泛化至 `Header`/`Query`/`Body`。例如 `@mock-when $.headers.X-Auth exists None` 或 `@mock-when $.body.user exists null` 会在相应键不存在或为 `null` 时正确命中。

### Sprint 4: 大模型流式调试与脚手架命令
1.  **大模型格式抽象规整与通用流断言**：开发 `LlmStreamAdapter`，规整 OpenAI 与 Anthropic 流式事件帧至虚拟对象。支持对 `stream.llm.content` 的直接断言，非大模型流自动降级为 Generic 格式累加。
2.  **增量文件物理同步 (`@stream_to`)**：使用异步非阻塞 `FileSyncWriter` 增量追加/覆写流 Token 到物理文件，完美支持 IDE 分屏实时阅读。
3.  **内置 Mock 调试服务器**：内置 `MockLlmServer` 响应流测试，且提供本地零外部配置的测试闭环。
4.  **路由中间件与密钥泄露审计**：利用 `RoutingMiddleware` 安全重写外部大模型 host 为本地 mock 服务，且内置静态 API Key 泄露审计扫描，发现明文 Key 立即阻断。
5.  **脚手架初始化与模板命令 (`rupost init`)**：增加统一的 `rupost init` 命令并设置别名 `template`/`i`：
    -   **极简整合**：将全局配置 `rupost.toml`（即 `config` 类型）和用例模板（如 `sse` 等类型）的生成逻辑在底层用统一的 `TemplateStrategy` 策略模式解耦驱动。默认无参数初始化 `rupost.toml`。
    -   **格式自适应与路径推导**：自适应输出为 `.http`/`.md`，智能进行空路径输出解析（`config` 默认 `rupost.toml`，`sse` 默认 `sse_template.http`）。
    -   **安全预警分级机制**：检测输出文件是否重名冲突。在非 `--force` 模式下直接报错中断；在 `--force` 模式下仅在终端打印高亮警告（Warning）提醒，但不终止覆盖写。
    -   **自编译打包**：采用 `include_str!` 在编译期内置托管模板，单二进制文件开箱即用，无任何外部资产文件物理检索依赖。

### 元数据注释前缀兼容与运行脚本
1.  **元数据注释前缀兼容**：重构了 `src/parser/http_file.rs` 的行切分与元数据提取，支持在每行前自动剔除 `#` 和 `//` 等主流注释标记。现在 `# @skip`、`# @name` 以及 `// @assert` 可以完美并安全地生效，对齐 JetBrains HTTP Client / VS Code Rest Client 标准用法。
2.  **一键运行全量示例脚本 (`examples/run_all.sh`)**：编写了全自动验证脚本，集成构建、变量环境校验，通过两阶段（独立 Mock 启动、多依赖 DAG 有向图拓扑契约 Mock 启动）自动跑通 examples 目录下的全部测试用例，提供完整的 CI 闭环，对超时外网依赖收敛到更为稳定的 `httpbingo.org`。

### Sprint 5: WebSocket 协议测试与调试
1. **多路广播长连接底座 (`WsClient`)**：基于 Actor 模式实现统一的 WebSocket 长连接客户端，使用广播信道同步所有帧。配备后台双层 Worker 协程，支持发送 Ping/Pong 保活以及流式业务帧交换。
2. **动作提取与流式剧本解析 (`WsActionParser`)**：支持行级切分解析 `SEND`、`EXPECT`、`WAIT` 和 `CLOSE` 动作序列。支持在 `.http` 或 `.md` 中以 `@websocket` 声明长连接测试，支持指定 `@decoder` 解码器。
3. **自适应解码与级联断言匹配**：支持 MsgPack 二进制数据及 JSON 数据的自适应解码。对于匹配的帧支持级联的 `@assert` 断言和 `@capture` 变量提取，通过基于 JSONPath 的软匹配设计，即使在高频广播干扰下也能准确执行断言，无匹配帧雪崩。同时，支持在 EXPECT 匹配条件判定前将二进制帧通过转码器解码，从而能够进行子集与字段层面的精确逻辑判定。
4. **引擎无侵入路由分发 (`WsRunner`)**：为 WebSocket 设计了专用的执行引擎 `WsRunner`，在 `TestExecutor` 中利用 3 行微手术分流，完全契合开闭原则 (OCP)，不侵入已有的 HTTP 和 SSE 架构设计。
5. **重连自愈与逻辑会话代理 (`WsSession`)**：构建逻辑会话层 `WsSession` 作为统一接口，后台通过 `SessionManager` 进行物理断网的自动重连。当物理连接断开时，采用指数退避算法自动进行 5 次重连（总计约 30 秒）。在重连挂起阶段，支持限额 100 帧的发送缓冲队列 `pending_send_queue` 以自愈暂存。重连成功后进行强时序 Flush 补发，保障极高时序下的消息流连续性。
6. **滑动内存限制历史帧环形缓冲区 (`BoundedFrameBuffer`)**：实现 FIFO 滑动删除的环形缓冲区，对历史收发帧数据的存储设定 1000 帧内存上限，规避长连接大行情调试下由于消息高频推送带来的 OOM 内存爆仓隐患，且便于未来 TUI 分栏的历史回溯浏览。
7. **边界修复与单元测试强化**：修复了 `JsonPathMatcher` 匹配反转漏洞，完全支持 `!=` 和 `contains` 操作符；修正了发送失败入队重连时的数据时序方向（更新为 `push_front`）；前置热点变量替换，消除高频帧循环中的多次字符串切割开销；精确统计实际运行动作迭代数并丰富了各底层模块的配套单元/集成测试。

### 架构重构与组件化分层 (CLI & URL Resolver)
1. **CLI 语法解析器解耦 (`src/cli/parser.rs`)**：
   - 彻底将 httpie 和 curl 命令行的无状态解析逻辑从有状态的 `CliRunner` 中抽离，并单独提供了 `test_parse_httpie_strict_json_error` 和 `test_parse_curl_unsupported_with_value` 等高覆盖率的专属单元测试。
2. **URL Resolution 领域服务解耦 (`src/runner/url.rs`)**：
   - 将 URL 智能拼接、冒号本地快捷键替换以及分层相对路径计算算法，从 `TestExecutor` 执行器中彻底剥离，作为无状态的纯逻辑领域服务。大幅提升了代码的可读性，且测试无需依赖 Executor 或 CLI 即可独立运行。

### HTTP 快照录制与原样重放 (MVP)
1. **测试用例实时录制 (`--save-snapshot`)**：
   - 在 `rupost test` 命令中扩展了可选参数 `--save-snapshot`，能够无缝捕获测试过程中的完整 HTTP 交互流并打包为结构化 JSON 格式快照文件。
2. **非侵入式请求状态追踪**：
   - 为 `TestResult` 扩充了 `request: Option<RequestSnapshot>` 数据承载。在执行器执行请求和响应完毕后，非侵入式自动保存其请求快照以实现快照合并序列化。
3. **快照重放执行引擎 (`ReplayExecutor`)**：
   - 新增了重放引擎 `ReplayExecutor`，支持加载快照套件并发送真实流量，支持将原始请求的 Protocol/Host/Port 动态改写为指定的 `--target` 目标物理地址，保持 Path 和 Query 原样不变。
4. **状态码比对诊断**：
   - 自动比对新老响应状态码，并在终端高亮高颜值输出 Diff Diff 结果。

### Sprint 7: TUI 引导、自适应与交互重构
1. **底部常驻引导条 (lazygit 风格)**：
   - 采用垂直布局分理出终端屏幕最底下一行作为动态指引栏。根据当前激活的 Panel 以及侧边栏当前的子 Tab (Files/History)，高亮实时提示对应的操作快捷键（例如 Tab/Enter/Click/p/j/k）。
2. **窄屏自适应三栏布局**：
   - 彻底重构自适应布局下的 `LayoutMode::Narrow`，当终端宽度在中窄屏范围（80~119）时不再隐藏 Sidebar，而是调整为三栏布局 (20% sidebar, 40% editor, 40% response)，保障在任何合适的分辨率下侧边栏和历史列表都清晰可见。
3. **鼠标点击捕获与面板切焦**：
   - 全程开启 `EnableMouseCapture`。捕获鼠标点击事件并计算点击的具体 X/Y 轴坐标，结合当前的自适应 LayoutMode，实现了鼠标点击侧边栏子 Tab 进行切换、点击列表中的文件/历史项进行预览高亮，以及点击主窗口对应的 Panel 区域直接切换焦点的顺畅体验。
4. **同步预览与切焦编辑**：
   - 当在 Files 或 History 列表中使用 `j/k` 移动光标或者使用鼠标点击选中时，只要当前编辑器不是脏的 (`!is_dirty`)，主编辑区和响应区就会同步加载文件/历史请求的内容与响应，仅做展示。
   - 按下 `Enter` 键时，仅仅将焦点切入到 Editor 区域以开始编辑，从而实现了逻辑上的“只读预览”与“按回车才编辑”的高雅结合。
   - 如果编辑器内有未保存的脏数据，而用户在侧边栏中切换或回车打开其他内容时，会拉起强确认拦截弹窗保护临时打字修改。
5. **冒险性测试用例**：
   - 编写了空历史按回车、脏编辑器下切历史拦截、Narrow 临界尺寸（90）三栏自适应等边界校验用例，全方位巩固了 TUI 的稳定性。

### TUI 性能优化与交互升级
1. **内存预折行 (Pre-wrapping) 机制**：
   - 摒弃了 Ratatui Paragraph 自带的 `.wrap()` 机制，在文本加载和窗口大小发生变化时，利用 `unicode-width` 精确对中英文/Emoji 的视觉显示宽度进行剪切，并在内存中缓存折行结果。
   - 实现了物理行与视觉行 1:1 的精确映射与垂直滚动，保证滚动永远不超过内容边界且能精准触底，彻底消除了 WS/SSE 流式日志在大视口局部加载下由于折行计算不齐导致的滚动崩溃。
2. **侧边栏 Files & History 双 Tab 状态解耦**：
   - 将侧边栏横向切分为 `[F] Files`（测试文件）和 `[H] History`（请求历史）两个独立 Tab，可用左右方向键或 `h/l` 无缝切换。
   - 二者独立维护自身的 `selected_index` 和 `scroll_offset`，切换 Tab 时位置原样保留。引入滑动可视区边界保持算法，在通过 `Up/Down` 或 `j/k` 移动时，自动将选中项保持在屏幕可见区域内。
3. **文件名优先与路径简显切换**：
   - 文件列表默认仅高亮显示文件名本身，防止过长的父路径导致文件名被 `...` 截断。在侧边栏激活时，支持按 `p`/`P` 键一键切换展示完整相对路径。
4. **历史快照 Enter 一键反向还原**：
   - 历史面板精细化根据 HTTP 方法与状态码着色展示。在任意历史快照上按回车，自动调用 `format_request_snapshot_to_http` 算法将其逆向格式化为标准的 `.http` 源码并填充到编辑器，实现历史记录的零门槛重新编辑与发送。
5. **测试保障与模块解耦**：
   - 将单文件 `ui/mod.rs` 彻底重构组件化拆分为 `sidebar.rs`、`editor.rs` 和 `response.rs`。
   - 新增了中英文折行切分、列表可视区边界修正、历史序列化还原等核心逻辑的单元测试；并在 `TestBackend` 终端下编写了自适应 Wide/Narrow/Stacked 三种布局模式及多种 Modal 重叠状态的子组件渲染冒烟测试，通过 `cargo test` 全量通过回归。

### 统一断言与协议执行链解耦重构 (方案一 & 二)
1. **统一断言求值体系 (方案一)**：
   * **断言目标抽象**：设计 `AssertTarget` 统一数据提取接口，实现 HTTP Response 和 WebSocket WsFrame 的提取器对齐。
   * **逻辑复用与消除冗余**：使 `evaluate_assertion` 与 HTTP Response 解耦，WebSocket 模块中原先手写的匹配断言判定逻辑（如 `JsonPathMatcher::matches`）彻底删除，全部委托给系统级评估引擎。
2. **协议执行链解耦与静态分发 (方案二)**：
   * **执行流剥离**：将原本堆叠在 `executor.rs` 里的 HTTP 执行、SSE 监听、诊断以及快照历史等大段底座实现物理平移至 `src/runner/http_runner.rs` 中的 `HttpRunner`。
   * **静态分发路由**：定义 `Runner` Enum 作为分发策略，规避 `dyn trait` 带来的 Object Safety 异步 Future 限制，使 TestExecutor 主逻辑的物理行数缩减 80% 以上，职责划分清晰，完全符合 SRP 与 OCP。

### TUI v3.3 加固迭代 Phase 1：安全防线与写路径端口化
1. **安全三防线（红测试先行，旧代码实测会逃逸工作区并吞错）**：
   - 拒绝携带路径语义的文件名（`..`/反斜杠/绝对路径）；围栏判定复用从 DAG resolver 提取的 `utils::path_guard::is_within_roots`（两处共用一份实现）。
   - `exists()+write()` 组合的 TOCTOU 竞态改为 `OpenOptions::create_new` 原子创建；已存在不覆盖仍定位；其余 io 错误浮出到 `last_error` 状态栏红字呈现。
   - URL 推导文件名超 80 字符截断并追加 8 位短哈希，远离 255 字节上限。
2. **URL 归一化 DRY**：`http::url::normalize_input_url` + `NormalizedInput` 枚举成为 CLI 与 TUI 共用的自由输入判定谓词；决策表以表格驱动测试锁定。设计文档 v1.2/users→NotUrl 的行在实现期修正为 HostName（与 CLI `is_independent` 同规则优先）。
3. **InputMode 模态输入**：`show_quick_input`/`quick_input_value` 双字段删除，代之以 `InputMode::QuickCreate(Box<TextArea>)` 互斥枚举；textarea 带来多字节正确编辑与真实光标；死代码 `Action::UpdateQuickInput` 与 `quick_input` 字段移除。
4. **回归**：单测 30 + 冒烟 39 + 全量 443 全绿；`verify_features.sh` PASS、`examples/run_all.sh` 15/15（本机需 `SDKROOT=.../MacOSX26.5.sdk` 规避 CommandLineTools 27.0 SDK 的 .tbd 损坏——环境问题与本次改动无关）。
5. **遗留（下一片）**：`state/mod.rs` 剩余域（wrap/stream/history/reload）与 `handlers.rs` 的拆分；读路径 I/O 端口化（设计文档债务登记）。
