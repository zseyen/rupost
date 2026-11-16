# RuPost 统一基础服务、配置体系与跨平台闭环执行方案

> **文档状态**: 已完成架构与代码实况审查（定稿）
> **创建日期**: 2026-11-16
> **适用架构**: Clean Architecture + DRY SSOT + TDD (Red-Green-Refactor)

---

## 一、 核心架构准则与红线（不可妥协约束）

1. **领域层零感知（依赖倒置）**
   - `HistoryStorage`、`PluginRegistry`、`WebSessionStore`、TUI 扫描器只接收组合根解析注入的强类型 `PathBuf` 或配置结构体，内部禁止再直接调用 `std::env::var*` 或硬编码 `".rupost"`。
   - 相对路径统一锚定调用方显式传入的 `project_root: &Path`（禁止在配置加载器或存储层内部隐式依赖进程全局 `std::env::current_dir()`），保障 `rupost web --dir <dir>` 与 `rupost tui` 长驻服务的多工作区隔离。
2. **向后兼容（`#[serde(default)]`）**
   - `rupost.toml` 所有新增配置段（`[paths]`、`[plugin]`、`[redaction]`、`[ai]`）全部标注 `#[serde(default)]`。无配置文件或字段缺省时，行为 100% 等价于当前默认约定，旧项目零破坏。
3. **三端同源（CLI / TUI / Web）**
   - 通过纯数据配置加载入口 `WorkspaceConfig::load(project_root, sink)` 提供统一配置快照，各入口按需切片消费，彻底消除“CLI 支持某参数或环境变量，TUI/Web 却不识别”的割裂。
4. **安全与隐私边界**
   - **项目级路径沙箱**：`./rupost.toml` 的 `[paths]` 自定义路径必须囚禁在 `project_root` 沙箱之内（禁止绝对路径、`..` 父级回溯与工作区外 symlink），防止恶意仓库引发任意文件覆盖；工作区外路径仅允许通过 CLI 参数或用户主目录配置指定。
   - **AI 凭据分离**：项目级 `./rupost.toml` 与 `~/.rupost/ai.toml` 依然**严禁明文 `api_key`**（维持 `TC-C05` 拒绝策略），仅允许通过 `[ai].api_key_env` 指定环境变量名；明文凭据仅允许存放在用户本机 `$HOME/.rupost/credentials.toml` 且受 `0o600` + 本人属主门禁保护。
   - **脱敏闭环**：动态解析到的自定义 `api_key`（无论来自 `api_key_env` 还是 `credentials.toml`）自动注册进 `RedactionPolicy` 动态脱敏词表，确保不泄露到日志、快照及 `--incident-out` 故障复现包。
5. **Simplicity First（奥卡姆剃刀）**
   - 拒绝提前抽象或引入非必要重依赖：不建上帝对象 `AppRuntimeContext`、不建单用文件 `HttpClientFactory`、不额外引入 `tokio-util`（如需优雅退出广播直接使用 `tokio::sync::watch`）。

---

## 二、 代码库基线盘点（已落地 vs 待收口）

| 能力域 | 现状核对 | 待收口缺口 |
| :--- | :--- | :--- |
| **进程启动与环境裁剪** | `src/plugin/driver.rs` 已抽出 `spawn_plugin_process` 与 `apply_plugin_env`（含 Windows Shebang 桥接与 `ETXTBSY` 重试），且 `src/cli/plugin_exec.rs` 已复用。 | `src/runner/on_failure.rs` 仍独立编写 `Command::new(shell())` 与 Unix `process_group(0)` / `kill` 逻辑，需抽离共享进程组原语。 |
| **诊断告警通道** | 全库存在 30+ 处裸 `eprintln!`（`executor_factory.rs`、`suite_setup.rs`、`variable/config.rs`、`security/redact.rs`、`executor_plugin.rs` 等）。 | TUI（Raw Mode 撕裂）与 Web（前端不可见）丢失装配/运行时告警，需引入 `WarningSink`。 |
| **配置与路径解析** | `src/variable/config.rs` 存在分散的 `load_from_path`、`load_plugin_config_from_path`、`load_redaction_policy_from_path`；`src/platform.rs` 仍直接读取 `RUPOST_HISTORY_DIR` 并硬编码 `.rupost`；`src/history/storage.rs` 存在全局单例 `get_storage()`。 | 缺少统一聚合的 `WorkspaceConfig` / `WorkspacePathsConfig` 注入，需移除 `get_storage()` 旁路及散落硬编码。 |
| **HTTP 客户端构建** | `reqwest::Client::builder()` 仅存在于 `src/http/client.rs` 与 `src/ai/config.rs` 两处。 | 在 `src/http/client.rs` 内提供轻量共享构建函数即可复用，无需独立新建 Factory 模块。 |
| **AI 密钥配置** | `src/ai/config.rs` 仅支持 `RUPOST_AI_API_KEY` > `OPENAI_API_KEY` > `~/.rupost/ai.toml`（禁 `api_key`）> Ollama 本地探测。 | 缺少用户自定义 `api_key_env`、多 `[ai.profiles.<name>]` 以及可选的本机 `~/.rupost/credentials.toml`（`0o600` 门禁）支持。 |
| **CI 编译卫生** | `Cargo.toml` 中 `semver = "1.0"` 零引用；`storage.rs` (`sample_entry`)、`tests/on_failure_test.rs`、`tests/snapshot_contract_test.rs` 存在 Windows `#[cfg(not(unix))]` 下的未使用警告风险。 | 需在 Wave 0 优先清理，确保三端 `clippy -- -D warnings` 零警告通过。 |

---

## 三、 分阶段执行路线图（MVP 渐进交付）

### Wave 0：编译卫生清理与文档收口（立即可执行）

1. **更新 `README.md`**
   - 在「核心特性」第 26 行补充插件跨平台 `#!` Shebang 透明桥接、零子进程运行时预检（支持 Windows `.exe`/`.cmd`/`.bat` 与 `python3` 回退探测）及 `RuntimeMissing` 三选一自愈降级说明。
   - 在「常用命令大纲」增加 `rupost plugin list` / `validate` / `schema` 及 `--allow-project-plugins` / `--allow-missing-plugins` 示例与跨平台提示。
2. **修复 Windows 条件编译警告与闲置依赖**
   - [Cargo.toml](file:///Users/zsyzzx/project/rust/rupost/Cargo.toml#L66)：移除未使用的 `semver = "1.0"`（后续若正式接入版本硬门禁再按需引入）。
   - [src/history/storage.rs](file:///Users/zsyzzx/project/rust/rupost/src/history/storage.rs#L297)：为仅用于 Unix 权限测试的 `sample_entry()` 补齐 `#[cfg(unix)]`。
   - [tests/on_failure_test.rs](file:///Users/zsyzzx/project/rust/rupost/tests/on_failure_test.rs)：为仅被 `#[cfg(unix)]` 测试调用的辅助函数与导入补齐 `#[cfg(unix)]` 门控。
   - [tests/snapshot_contract_test.rs](file:///Users/zsyzzx/project/rust/rupost/tests/snapshot_contract_test.rs)：整理非 Unix 条件下未使用的导入门控。

---

### Wave 1（P0）：极简基础设施原语

#### 1.1 `WarningSink` 统一诊断告警总线
- **位置**：新增 `src/diagnostics/warning_sink.rs`（约 40 行，无重型状态机）。
- **结构设计**：
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum WarningSeverity { Info, Warn, Security }

  pub trait WarningSink: Send + Sync {
      fn emit(&self, severity: WarningSeverity, source: &'static str, message: &str);
  }
  ```
- **内置实现（仅 2 个）**：
  - `StderrWarningSink`：CLI 端格式化输出到 `stderr`。
  - `BufferedWarningSink`：写入线程安全 `Vec<String>`；供 Web（直接序列化进接口响应 `warnings`）、TUI（同步装配完成后取出写入状态栏/弹窗队列）与单元测试断言使用，无需额外异步 `mpsc` 通道。
- **首批收敛点**：
  - `src/runner/executor_factory.rs`
  - `src/runner/suite_setup.rs`
  - `src/variable/config.rs`
  - `src/security/redact.rs`
  - `src/runner/executor_plugin.rs`

#### 1.2 `ProcessGroupGuard` 进程组强剿原语 + 取消信号支持
- **位置**：`src/platform/process.rs`。
- **内容**：收敛 `src/plugin/driver.rs` 与 `src/runner/on_failure.rs` 中重复的 `#[cfg(unix)] process_group(0)`、组级 `SIGKILL` 以及 `Drop` 守卫；同时预留 `tokio::sync::watch::Receiver<bool>` 参数入口以支持后续 `Ctrl+C` 优雅停机取消传播。

#### 1.3 HTTP 客户端构建收口
- **位置**：`src/http/client.rs`。
- **内容**：暴露轻量基础构建函数（统一 TLS Rustls 策略与连接/请求超时基础配置），供 `src/ai/config.rs` 的 `probe_local` 直接复用。

- **TDD 验收**：
  - `BufferedWarningSink` 确定性捕获插件发现失败与 `--var-state` 损坏告警且零 `stderr` 泄露；
  - `plugin_driver_test`、`on_failure_test` 回归全绿。

---

### Wave 2（P1）：统一配置、路径沙箱与 AI 凭据安全体系

#### 2.1 `WorkspaceConfig` + `WorkspacePathsConfig` 统一加载与依赖注入
- **位置**：`src/variable/types.rs`、`src/variable/config.rs`。
- **职责**：
  - 定义 `WorkspacePathsConfig`（`plugins_dir`、`history_file`、`logs_dir`、`cookie_dir`、`reports_dir`、`snapshots_dir`）及聚合配置 `WorkspaceConfig`（含 `#[serde(default)]` 的 `[paths]` / `[plugin]` / `[redaction]` / `[ai]`）。
  - 提供单一无状态函数 `WorkspaceConfig::load(project_root: &Path, sink: &dyn WarningSink)`：
    1. 所有相对路径严格锚定传入的 `project_root`；
    2. 对项目级 `[paths]` 执行沙箱防逃逸校验（拒绝绝对路径、`..` 与指向工作区外的 symlink）；
    3. 统一仲裁优先级：`CLI 参数 > 环境变量 > rupost.toml > 默认值`。
- **去除硬编码与单例旁路**：
  - 移除 `src/history/storage.rs` 中的全局静态单例 `get_storage()`，改为通过 `HistoryStorage::new_with_path(&paths.history_file)` 注入；
  - CLI、TUI、Web 与 `executor_factory.rs` 统一消费解析后的路径，不再直接拼接 `".rupost"` 或零散读取 `std::env::var`。

#### 2.2 AI 自定义 API Key 与多 Profile 方案（方案 1 + 方案 2 + 方案 4 融合）
- **位置**：`src/ai/config.rs`（复用 `src/security/fs.rs` 的 `is_owner_only_regular_file`、`ensure_private_dir`、`write_private_file_atomic`）。
- **配置分层与 5 层安全门禁**：
  1. **项目级 `./rupost.toml` & `~/.rupost/ai.toml`**：支持 `[ai].api_key_env`（环境变量名指针）与多 `[ai.profiles.<name>]`；**继续保持禁止明文 `api_key` 的硬红线**（维持 `TC-C05` 拒载与指引文案）。
  2. **用户本机私有凭据 `$HOME/.rupost/credentials.toml`**：
     - **作用域锁定**：仅从用户主目录加载，绝不扫描工作区 `./.rupost/`；
     - **普通文件与防符号链接校验**：调用 `is_owner_only_regular_file` 与 Unix `O_NOFOLLOW`；
     - **属主与 `0o600` 权限硬门禁**：Unix 下强制校验 `uid == geteuid()` 且 `mode & 0o077 == 0`（权限过宽如 `0o644` 时拒绝读取，并通过 `WarningSink` 提示执行 `chmod 600`）；
     - **内存防泄露**：结构体内实现脱敏 `Debug`（输出 `[REDACTED]`）且禁用 `Serialize`，并将加载的密钥值注入动态 `RedactionPolicy`。
- **完整解析优先级**：
  `RUPOST_AI_API_KEY` > `[ai].api_key_env` 指向的环境变量 > `OPENAI_API_KEY` > `~/.rupost/credentials.toml` 匹配 Key > 本地回环端点 `"local"` 占位（Ollama）。

#### 2.3 `AtomicFileWriter` 安全落盘收口
- **位置**：`src/security/fs.rs`。
- **内容**：收敛 `plugin/registry_cache.rs`、TOFU 缓存（`cli/plugin_exec.rs`）中重复的私有目录创建、Unix `0o600` / `O_NOFOLLOW` 校验与原子替换逻辑。

- **TDD 验收**：
  - 表驱动单测锁定 `WorkspaceConfig::load` 默认兼容性、相对路径工作区沙箱逃逸拦截；
  - 单测验证 `credentials.toml` 在 `0o644` 拒载、`0o600` 放行、且密钥值被动态脱敏掩码。

---

### Wave 3（P2）：交互会话同构与处方收口

1. **`InteractiveSession` 领域下沉**：从 `src/web/session.rs` 与 `src/tui/state/` 抽取共享核心交互执行会话（绑定 `TestExecutor` + `PluginRegistry` + 变量捕获表 + `CookieStore`），使 Web 与 TUI 核心执行语义 100% 同构。
2. **`PrescriptionFormatter` 处方文案收口**：合并 `src/plugin/pipeline.rs` 与 `src/cli/plugin_route.rs` 中重复的三选一处方格式化逻辑。

---

## 四、 每轮质量门禁与提交规范

每个 Wave 完成后必须通过以下全部校验方可 `jj` 原子化提交：
1. `cargo fmt --check`
2. `cargo clippy --all-targets --workspace -- -D warnings`
3. 对应新增表驱动单元测试 + 受影响集成测试 + `cargo test` 全量回归
4. 同步更新 [progress_summary.md](file:///Users/zsyzzx/project/rust/rupost/doc/plans/progress_summary.md) 记录里程碑进度。
