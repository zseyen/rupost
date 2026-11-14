# WP-2026-11-14-01：TUI/Web 插件与门禁选项面透传（replay-reuse B3/B4 执行方案）

- **状态**: Ready for Implementation（设计定稿 + 代码示例 + 使用场景 + TDD 清单）
- **工作量预估**: 半天（M）
- **上游**: [2026-11-14-remaining-work-roi-analysis.md](./2026-11-14-remaining-work-roi-analysis.md) 包 2（B3/B4）
- **核心文件**: `src/variable/config.rs`、`src/runner/executor_factory.rs`、`src/variable/types.rs`、`src/web/handlers/run.rs`、`src/tui/handlers/actions/execution.rs`、`src/cli/commands/test.rs`（对照）

---

## 1. 目标

消除"TUI/Web 是插件二等公民"：交互入口（TUI / Web 工作台）的插件运行时选项
（降级/门禁/超时/env 放行/strict_vars）从硬编码默认值变为**可配置**，
优先级与既有级联一致：

```
Web 请求参数（extra，仅 Web） > rupost.toml [plugins] 段 > PluginRuntimeOptions::default()
```

**非目标**（保持裁决）：不新增 CLI 旗标（CLI 已有完整旗标面）；不改变 CLI 路径
`assemble_plugin_runtime` 的任何行为；不做 Web var-state 落盘（已裁决维持内存态）。

## 2. 设计

### 2.1 toml `[plugins]` 段 schema（serde 追加式演进）

`src/variable/types.rs` 的 `VariableConfig` 追加顶层可选段（旧 toml 无该段 → None，
零破坏；未知字段容忍原则不变）：

```rust
/// B3（免重填主线）：TUI/Web 交互入口的插件与门禁选项（rupost.toml [plugins] 段）。
/// 仅交互入口消费——CLI 套件路径的旗标面不变（assemble_plugin_runtime 不读此段）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PluginTomlConfig {
    /// 插件缺失/失败时降级为警告跳过（默认 false：阻断 + 处方）
    #[serde(default)]
    pub allow_missing_plugins: bool,
    /// CI 式严格门禁：发生任何降级 → 整体判失败（TUI run_results / Web 响应 success=false）
    #[serde(default)]
    pub strict_plugins: bool,
    /// 单次插件调用超时秒数（默认 5）
    #[serde(default)]
    pub plugin_timeout_secs: Option<u64>,
    /// 显式放行插件子进程环境变量（KEY 或 KEY=VALUE，语义同 --plugin-env）
    #[serde(default)]
    pub plugin_env: Vec<String>,
    /// 放行工作区 ./.rupost/plugins（默认隔离；env 通道 RUPOST_ALLOW_PROJECT_PLUGINS 仍生效）
    #[serde(default)]
    pub allow_project_plugins: bool,
    /// E-4：未解析 {{var}} 拒绝外发
    #[serde(default)]
    pub strict_vars: bool,
}

// VariableConfig 追加（追加式演进：旧 toml 无该段 → None）：
 pub struct VariableConfig {
     /// 所有环境配置
     #[serde(default)]
     pub environments: HashMap<String, Environment>,
+    /// B3：TUI/Web 交互入口的插件与门禁选项（CLI 套件路径不消费此段）
+    #[serde(default, skip_serializing_if = "Option::is_none")]
+    pub plugins: Option<PluginTomlConfig>,
 }
```

对应 `rupost.toml` 用法：

```toml
[environments.dev]
base_url = "http://127.0.0.1:9000"

[plugins]
allow_missing_plugins = true      # 交互调试：缺失降级 + 处方提示
plugin_timeout_secs = 15          # SigV4 STS 冷启动类慢插件
plugin_env = ["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"]  # 从父环境放行
# strict_plugins = true           # CI 式工作台：降级即失败
# strict_vars = true              # 未解析 {{var}} 拒绝执行
# allow_project_plugins = false   # 默认已隔离，显式声明仅为可读性
```

### 2.2 读取入口（ConfigLoader 单点，SSOT）

```rust
// src/variable/config.rs — ConfigLoader impl 追加
/// B3：读取 [plugins] 段（交互入口专用；缺失/解析失败静默回退默认 + 告警，
/// 与 find_and_load 的告警策略一致）。TUI/Web 均经此单点，禁止各自解析 toml。
pub fn find_and_load_plugin_config() -> (PluginTomlConfig, Vec<String>) {
    let mut warnings = Vec::new();
    let Some(path) = Self::find_config_path() else {
        return (PluginTomlConfig::default(), warnings);
    };
    match Self::load_from_path(&path) {
        Ok(config) => {
            if let Some(cfg) = config.plugins {
                return (cfg, warnings);
            }
            (PluginTomlConfig::default(), warnings)
        }
        Err(e) => {
            warnings.push(format!("rupost.toml [plugins] 段解析失败，已回退默认: {e}"));
            (PluginTomlConfig::default(), warnings)
        }
    }
}
```

### 2.3 工厂签名演进（executor_factory）

```rust
// src/runner/executor_factory.rs
/// 交互入口共享执行器（B3 演进）：toml [plugins] 段 → PluginRuntimeOptions，
/// TUI/Web 共用；DiscoverOptions 仍由调用方注入（测试/项目根隔离）。
pub fn build_interactive_executor_with_options(
    discover: &DiscoverOptions,
    plugin_cfg: &crate::variable::PluginTomlConfig,
    strict_vars: bool,
) -> TestExecutor {
    let plugin_runtime = PluginRuntimeOptions {
        allow_missing_plugins: plugin_cfg.allow_missing_plugins,
        strict_plugins: plugin_cfg.strict_plugins,
        plugin_timeout: std::time::Duration::from_secs(
            plugin_cfg.plugin_timeout_secs.unwrap_or(DEFAULT_PLUGIN_TIMEOUT_SECS),
        ),
        plugin_env: crate::plugin::pipeline::parse_plugin_env_specs(&plugin_cfg.plugin_env),
    };
    let registry = discover_plugin_registry(
        &DiscoverOptions {
            allow_project_plugins: plugin_cfg.allow_project_plugins
                || crate::security::fs::env_flag_enabled("RUPOST_ALLOW_PROJECT_PLUGINS"),
            project_root: None,
        },
        &crate::plugin::pipeline::extract_authorized_env_keys(&plugin_cfg.plugin_env),
    );
    TestExecutor::with_ephemeral_cookies()
        .with_strict_vars(strict_vars)
        .with_plugins(registry, plugin_runtime)
}

/// 既有无参入口保留（兼容既有调用点/测试），语义 = default 配置：
pub fn build_interactive_executor() -> TestExecutor {
    build_interactive_executor_with(
        &DiscoverOptions {
            allow_project_plugins: crate::security::fs::env_flag_enabled(
                "RUPOST_ALLOW_PROJECT_PLUGINS",
            ),
            project_root: None,
        },
    )
}
// 注意：with_plugins(registry, PluginRuntimeOptions::default()) 行为不变——
// 无 toml 段时 zero-diff（Default 与 hard-coded 默认完全一致）。
```

### 2.4 TUI 接线（`tui/handlers/actions/execution.rs:135`）

```rust
// B3：toml [plugins] 段 → 交互执行器（告警走状态栏——渲染权归 TUI 适配层）
let (plugin_cfg, plugin_warnings) = ConfigLoader::find_and_load_plugin_config();
for w in &plugin_warnings {
    state.push_status_line(format!("[!] {w}")); // 既有状态栏通道，无则 eprintln
}
let mut executor = crate::runner::executor_factory::build_interactive_executor_with_options(
    &DiscoverOptions { /* 同现状 */ },
    &plugin_cfg,
    plugin_cfg.strict_vars,
);
executor.debug_on_failure = true;
```

### 2.5 Web 接线（`web/handlers/run.rs` 两 handler）

优先级：**请求参数 > toml > 默认**。`RunBlockRequest`/`RunSuiteRequest` 追加可选覆盖：

```rust
// src/web/types.rs（追加，serde default 兼容既有前端）
#[serde(default)]
pub plugin_overrides: Option<PluginOverrides>,

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PluginOverrides {
    #[serde(default)]
    pub allow_missing_plugins: Option<bool>,
    #[serde(default)]
    pub plugin_timeout_secs: Option<u64>,
    #[serde(default)]
    pub plugin_env: Vec<String>,
    #[serde(default)]
    pub strict_vars: Option<bool>,
}

// handlers/run.rs（run_block 与 run_suite 同构）：
let (mut plugin_cfg, plugin_warnings) = ConfigLoader::find_and_load_plugin_config();
for w in &plugin_warnings {
    tracing::warn!("web run: {w}");
}
if let Some(ov) = &req.plugin_overrides {
    if let Some(v) = ov.allow_missing_plugins { plugin_cfg.allow_missing_plugins = v; }
    if let Some(v) = ov.plugin_timeout_secs { plugin_cfg.plugin_timeout_secs = Some(v); }
    if !ov.plugin_env.is_empty() { plugin_cfg.plugin_env = ov.plugin_env.clone(); }
    if let Some(v) = ov.strict_vars { plugin_cfg.strict_vars = v; }
}
let executor = crate::runner::executor_factory::build_interactive_executor_with_options(
    &DiscoverOptions {
        allow_project_plugins: plugin_cfg.allow_project_plugins
            || crate::security::fs::env_flag_enabled("RUPOST_ALLOW_PROJECT_PLUGINS"),
        project_root: None,
    },
    &plugin_cfg,
    plugin_cfg.strict_vars,
);
```

### 2.6 B4：strict_vars 交互入口生效

`with_strict_vars` 已存在（批 5）；本 WP 使 `plugin_cfg.strict_vars` 经 2.3/2.4/2.5
真正到达 TUI/Web 执行器——B4 随 B3 闭环，无独立改动。

## 3. 使用场景

### 场景 1：TUI 调试带 SigV4 慢插件（当前被 5s 熔断）
项目 toml 声明 `plugin_timeout_secs = 15` + `allow_missing_plugins = true`。
张晨在 TUI 反复触发 `@auth aws-sigv4` 请求：STS 冷启动 8s 不再熔断；插件装
失败的项目里同事拉取后自动降级 + 状态栏处方，而不是硬阻断。

### 场景 2：Web 工作台云凭据放行
李浩的 Web 工作台跑含 `@auth vault` 的块，插件 `requires_env: ["VAULT_TOKEN"]`：
toml `plugin_env = ["VAULT_TOKEN"]` 一次性放行（父环境已有值），免每次拼
`--plugin-env`；前端亦可在单次请求 `plugin_overrides.plugin_env` 临时追加。

### 场景 3：CI 式 Web 工作台（strict）
预发工作台 toml 声明 `strict_plugins = true` + `strict_vars = true`：任何降级/
未解析变量 → 该块响应 `success=false` + 可读原因——工作台与 CI 门禁同语义，
"看板绿但流水线红"的矛盾不复现。

### 场景 4：零配置零破坏（回归保证）
无 `[plugins]` 段的既有项目：TUI/Web 行为与今天逐字节一致（Default 相等），
由"zero-diff 守护测试"锁定。

## 4. TDD 测试计划（先红后绿）

| # | 层 | 用例 | 断言 |
|---|---|---|---|
| T1 | lib（config） | `[plugins]` 段 serde round-trip（全字段/空段/缺段） | 缺段 → None；全字段逐值相等；未知字段容忍 |
| T2 | lib（config） | 坏 `[plugins]`（类型错）→ `find_and_load_plugin_config` 回退默认 + 告警 | 决策表：合法/缺失/损坏三形态 |
| T3 | lib（factory） | `build_interactive_executor_with_options`：toml timeout=15 → `plugin_options.plugin_timeout == 15s`；allow_missing=true 透传 | 字段级断言 |
| T4 | lib（factory） | zero-diff：无 toml 段 → 与 `PluginRuntimeOptions::default()` 全等 | 防假守护 |
| T5 | lib（factory） | `plugin_env` 经 `extract_authorized_env_keys` 进入 missing-env 告警豁免 | 告警不误报 |
| T6 | e2e（web） | toml `[plugins] allow_missing_plugins=true` + 未安装 `?optional=false` 插件块 → 响应 success=true + 降级告警 | R-4 会话复用叠加 |
| T7 | e2e（web） | `plugin_overrides.plugin_timeout_secs=1` + sleep-3s 插件 → 超时失败（Timeout 分类可见） | 请求级覆盖生效 |
| T8 | e2e（web） | strict_vars=true + 未解析 var 块 → success=false 且消息含 `--strict-vars` | 门禁透传 |
| T9 | e2e（tui，离屏 TestBackend 或 state 级） | toml strict_plugins=true + 降级结果 → run_results 判失败 | TUI 门禁 |

## 5. 台账与文档

- 底账 `2026-11-12-replay-reuse-backlog.md`：B3/B4 翻 ✅（引用本 WP）；
- CHANGELOG `[Unreleased]` 条目；
- `doc/guides/user_guide.md`（或 init 模板注释）补 `[plugins]` 段示例——**同时兑现 C8
  （schema/配置挂接文档）的一半**；
- ROI 文档包 2 标记完成。

## 6. 门禁

fmt / clippy `-D warnings` 零漂移；`cargo test --workspace` 全绿；
`tests/verify_features.sh` exit 0（T1–T8 均为单测/e2e 层，冒烟脚本不必扩）；
`examples/run_all.sh` ALL PASS（无 toml [plugins] 段 → zero-diff 路径）。

---

## 附：A1 状态更正

ROI 文档"包 3（评审驱动）：A1 error-Detail 提交契约评审"**已过时**——A1 已于
2026-11-14 经用户裁决实施完成（`pnmkwzwr d6875a15`，B-6 翻 ✅，02 附录二登记）。
包 3 从推荐组合中移除；本文档即包 2 的执行内容。
