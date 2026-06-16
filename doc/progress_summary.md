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
| **Clean Architecture 架构重构** | 彻底解耦 `src/parser` 与基础设施层（`src/http`、`src/mock`）的物理依赖，建立由外向内的单向依赖关系 | 已完成 | `src/http/request_builder.rs`, `src/mock/compiler.rs` |
| **启发式自适应请求拆分** | 智能自适应识别 HTTP / Markdown 代码块中的用例边界，解决因漏写 `###` 分隔符而导致的解析错乱问题 | 已完成 | `src/parser/http_file.rs` |
| **统一 JSONPath 评估引擎** | 统一断言端与 Mock 端的 JSONPath 提取与分词解析逻辑，支持点号、中括号数组定位与根数组解析 | 已完成 | `src/utils/jsonpath.rs` |
| **Sprint 3: 高级特性与脚本引擎** | @loop 循环, @skip-if 条件运行, 前后置 Javascript/Rust 脚本支持 | 未开始 | - |
| **Sprint 4: HTML 报告与高级表现层** | 导出可视化 HTML 报告与模板表现层 | 未开始 | - |

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
    *   **顺序模式**：按拓扑序串行运行，变量及 Cookie 自然传递。
    *   **并行模式**：利用 `tokio::task::JoinSet` 及并发限制器进行并发处理。
    *   **状态克隆传递 (State Cloning)**：在拓扑依赖链上，实现从前置节点向子节点单向克隆并合并 Context 变量增量与序列化还原的会话 Cookie，彻底消除并行开发的数据孤岛，而非依赖分支依然维持安全隔离。
    *   **Fail-Fast 控制**：支持随时在串行或并行模式下检测到错误请求时中断其余用例。
5.  **结构化 JSON 报告与批汇总**：支持 `--report json` 格式的控制台或文件输出；支持终端高颜值批测试摘要输出。
6.  **JSON Path 数组数值下标提取支持**：支持以点号跟数字的形式直接从 JSON 数组中按数值索引定位元素，解决 `body.headers.Cookie.0` 类路径取值失败的问题。
7.  **批量并发复杂测试场景验证（DAG）**：构建并跑通了包含 Fork 与 Join 等多维依赖拓扑的批量并发集成用例集（`examples/batch_complex`），针对 Cookie 字段顺序随机抖动进行 `contains` 断言优化，成功实现高并发下无感级联。

4.  **结构化 JSON 报告与批汇总**：支持 `--report json` 格式的控制台或文件输出；支持终端高颜值批测试摘要输出。

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

