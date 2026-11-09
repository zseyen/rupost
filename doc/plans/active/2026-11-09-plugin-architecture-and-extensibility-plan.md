# RuPost 微内核插件架构与扩展生态实施方案 (Plugin SPI & Extensibility Architecture v2 - Hardened)

- **文档状态**: Approved / Hardened & Ready for Execution
- **更新日期**: 2026-11-10
- **归档路径**: `doc/plans/active/2026-11-09-plugin-architecture-and-extensibility-plan.md`
- **目标执行者**: AI 研发智能体（含 Flash 级模型）/ 人类开发者
- **前置依赖**: 必须先完成 [`2026-11-09-implementation-plan-milestones-1-to-3.md`](file:///Users/zsyzzx/project/rust/rupost/doc/plans/active/2026-11-09-implementation-plan-milestones-1-to-3.md) 中的 **Phase 0（JSON 请求体原文保真与 `Request.raw_body` 改造）**
- **设计范式借鉴**: **DeepSeek Harness (`dsh`)**、Git Credential Helper、Docker CLI Plugins

---

## 0. 设计哲学、安全红线与前置条件

### 0.1 核心微内核原则
1. **微内核纯粹性（Zero-Bloat Core）**：
   - `rupost` 核心只负责**文档 AST 编译、异步并发 DAG 调度、微米级网络时序探测与基础断言**；
   - 云厂商加签（AWS SigV4、阿里云/腾讯云网关签名）、Serverless 事件封包/解包（Lambda RIE / API Gateway Proxy Event）、以及重型外部格式导入（Postman、OpenAPI 3.x、HAR）**统一通过进程级 Stdio JSON 管道插件扩展**；
   - 核心仓库零新增第三方云 SDK 依赖。
2. **前置条件（Phase 0 已就绪）**：
   - `src/http/request_builder.rs` 对 JSON 请求体只做 `serde::de::IgnoredAny` 语法校验并**按原文一字不差发送**（不重排键序、不压缩空白），确保签名计算的字节与网卡发出的字节 100% 一致；
   - `src/http/request.rs` 的 `Request` 结构体持有 `pub raw_body: Option<String>`，使所有中间件执行完毕后，签名插件依然能读取最终要发出的请求体原文。

### 0.2 供应链安全与信任边界（Mandatory Security Guardrails）
1. **插件名称白名单校验**：
   - 插件名必须严格匹配正则 `^[a-z0-9][a-z0-9_-]{0,63}$`，**严禁包含 `/`, `\`, `..` 等路径字符**，从根源阻断 `@plugin ../../bin/sh` 路径穿越攻击。
2. **项目级插件默认隔离（防恶意仓库 RCE）**：
   - 默认仅信任用户主动安装到 **`~/.rupost/plugins/`（用户全局目录）** 与 **`$PATH`** 中的插件；
   - 工作区内的 `./.rupost/plugins/`（以 `ConfigLoader` 定位的配置根目录为基准）**默认不自动执行**，除非用户在 CLI 显式传入 `--allow-project-plugins` 或环境变量 `RUPOST_ALLOW_PROJECT_PLUGINS=1`；
   - `rupost web --readonly` 只读模式下**无条件禁用所有插件调用**。
3. **统一脱敏出口**：
   - 插件返回的 `extra_headers`（如 `Authorization`）、`warnings`、`error` 以及快照落盘必须统一经过 `crate::ai::redactor` 脱敏，绝不在报告或日志中明文泄露密钥。

---

## 1. 生命周期四阶段顺序与耗时隔离

```
[ ParsedRequest ]
       │
       ▼
  1. VariableResolver::resolve_parsed_request()
     (包括对 @plugin 参数值 key={{var}} 进行变量替换)
       │
       ▼
  2. [可选] Plugin Stage: "pack" (Serverless 网关封包，改写 method/url/headers/raw_body)
       │
       ▼
  3. 内部 Middlewares (CookieMiddleware, RoutingMiddleware 改写 host/port 与注入 Cookie)
       │
       ▼
  4. Plugin Stage: "sign" (基于最终将发往网卡的 URL/Headers/raw_body 计算签名并注入 Header)
       │
       ▼  ───► [ 独立计时器 start = Instant::now() 在插件执行完毕后才启动，隔离 plugin_ms ]
  5. client.execute_raw(request) (纯粹的网络发送与 TTFB 测量)
       │
       ▼
  6. [可选] Plugin Stage: "unpack" (将 Lambda RIE / 网关包裹 JSON 解包还原为真实 status/headers/body)
       │
       ▼
  7. 断言评估 (AssertionEvaluator) 与变量捕获 (@capture)
```

> [!IMPORTANT]
> **时延纯净度铁律**：外部插件进程启动（如 Python 解释器耗时 100~300ms）**严禁计入 HTTP 请求的 `duration` 与 `ttfb`**！网络计时器 `Instant::now()` 必须在 `sign` 插件执行完毕、紧邻 `client.execute_raw` 之前启动。

---

## 2. 进程间通信协议 (Stdio IPC Protocol v1.0)

* **流纪律**：插件进程**只能在 `stdout` 输出唯一一个合法的 JSON 对象**；任何调试信息、进度打印一律输出到 `stderr`。
* **Header 保序**：请求与响应中的 `headers` 一律使用二维数组 `Vec<(String, String)>`（JSON 形态为 `[["k", "v"], ...]`），保留原始顺序与同名 Header。

### 2.1 `sign` 阶段协议（MVP 核心）

#### 输入 (`stdin`)
```json
{
  "protocol_version": "1.0",
  "action": "sign",
  "method": "POST",
  "url": "https://xyz.lambda-url.us-east-1.on.aws/orders",
  "headers": [
    ["Content-Type", "application/json"],
    ["X-Trace-Id", "req-001"]
  ],
  "body": "{\n  \"item_id\": 4821\n}",
  "plugin_args": {
    "region": "us-east-1",
    "service": "lambda"
  }
}
```

#### 输出 (`stdout`)
```json
{
  "success": true,
  "extra_headers": [
    ["Authorization", "AWS4-HMAC-SHA256 Credential=..."],
    ["X-Amz-Date", "20261110T000000Z"]
  ],
  "mutated_url": null,
  "mutated_body": null,
  "warnings": [],
  "error": null
}
```

### 2.2 `unpack` 阶段协议（云函数网关响应解包）

#### 输入 (`stdin`)
```json
{
  "protocol_version": "1.0",
  "action": "unpack",
  "status": 200,
  "headers": [["content-type", "application/json"]],
  "body": "{\"statusCode\": 201, \"headers\": {\"x-custom\": \"ok\"}, \"body\": \"{\\\"id\\\": 1}\"}",
  "plugin_args": {}
}
```

#### 输出 (`stdout`)
```json
{
  "success": true,
  "unpacked_status": 201,
  "unpacked_headers": [["x-custom", "ok"]],
  "unpacked_body": "{\"id\": 1}",
  "error": null
}
```

### 2.3 `import` 外部格式转换协议 (`rupost-import-<format>`)

#### 输入 (`stdin`)
```json
{
  "protocol_version": "1.0",
  "action": "import",
  "format": "postman",
  "content": "{ ...原始文件内容... }",
  "templatize_host": true
}
```

#### 输出 (`stdout`)
插件输出结构化请求列表 JSON（直接映射为微内核 `ParsedRequest` IR，由 `rupost` 统一渲染为 `.http` 或 `.md`，防止插件拼出畸形文本）：
```json
{
  "success": true,
  "requests": [
    {
      "name": "Create Order",
      "method": "POST",
      "url": "{{base_url}}/api/orders",
      "headers": [["Content-Type", "application/json"]],
      "body": "{\"item\": 1}"
    }
  ],
  "warnings": [],
  "error": null
}
```

---

## 3. 核心模块代码规范与精确实现 (`src/plugin/`)

### 3.1 `src/plugin/types.rs`
```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const PLUGIN_PROTOCOL_VERSION: &str = "1.0";

/// AST 元数据中的单条插件调用声明：`@plugin <name> [key=val ...]`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginInvocation {
    pub name: String,
    pub args: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginSignRequest<'a> {
    pub protocol_version: &'static str,
    pub action: &'static str,
    pub method: &'a str,
    pub url: &'a str,
    pub headers: Vec<(String, String)>,
    pub body: Option<&'a str>,
    pub plugin_args: &'a HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginSignResponse {
    pub success: bool,
    #[serde(default)]
    pub extra_headers: Vec<(String, String)>,
    #[serde(default)]
    pub mutated_url: Option<String>,
    #[serde(default)]
    pub mutated_body: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginImportInput<'a> {
    pub protocol_version: &'static str,
    pub action: &'static str,
    pub format: &'a str,
    pub content: &'a str,
    pub templatize_host: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginImportedItem {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginImportOutput {
    pub success: bool,
    #[serde(default)]
    pub requests: Vec<PluginImportedItem>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
}
```

### 3.2 `src/plugin/registry.rs`（含白名单校验、信任边界与跨平台探测）
```rust
use crate::{Result, RupostError};
use std::path::{Path, PathBuf};

pub struct PluginRegistry;

impl PluginRegistry {
    /// 严格校验插件名，阻断路径穿越攻击
    pub fn validate_plugin_name(name: &str) -> Result<()> {
        let valid = !name.is_empty()
            && name.len() <= 64
            && name.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
        if !valid {
            return Err(RupostError::Other(format!(
                "非法插件名称 '{name}'：仅允许小写字母、数字、连字符(-)与下划线(_)"
            )));
        }
        Ok(())
    }

    /// 按信任策略与优先级定位插件可执行文件
    pub fn resolve_executable(
        prefix: &str,
        plugin_name: &str,
        allow_project_plugins: bool,
        project_root: Option<&Path>,
    ) -> Result<PathBuf> {
        Self::validate_plugin_name(plugin_name)?;
        let bin_base = format!("{prefix}-{plugin_name}");
        let mut searched = Vec::new();

        // 1. 项目级目录（仅在显式开启信任时搜索）
        if allow_project_plugins {
            let root = project_root
                .map(PathBuf::from)
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."));
            let dir = root.join(".rupost").join("plugins");
            if let Some(found) = Self::probe_in_dir(&dir, &bin_base) {
                return Ok(found);
            }
            searched.push(format!("{} (已启用项目插件)", dir.display()));
        } else {
            searched.push(".rupost/plugins (默认禁用，传 --allow-project-plugins 启用)".to_string());
        }

        // 2. 用户全局目录 ~/.rupost/plugins/
        if let Some(home) = dirs::home_dir() {
            let user_dir = home.join(".rupost").join("plugins");
            if let Some(found) = Self::probe_in_dir(&user_dir, &bin_base) {
                return Ok(found);
            }
            searched.push(user_dir.display().to_string());
        }

        // 3. 系统 PATH
        if let Ok(path_var) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path_var) {
                if let Some(found) = Self::probe_in_dir(&dir, &bin_base) {
                    return Ok(found);
                }
            }
            searched.push("$PATH".to_string());
        }

        Err(RupostError::Other(format!(
            "未找到插件可执行文件 '{bin_base}'（已搜索: {}）",
            searched.join(", ")
        )))
    }

    fn probe_in_dir(dir: &Path, bin_base: &str) -> Option<PathBuf> {
        #[cfg(windows)]
        let candidates = [
            format!("{bin_base}.exe"),
            format!("{bin_base}.cmd"),
            format!("{bin_base}.bat"),
            bin_base.to_string(),
        ];
        #[cfg(not(windows))]
        let candidates = [bin_base.to_string()];

        for name in candidates {
            let path = dir.join(name);
            if Self::is_executable(&path) {
                return Some(path);
            }
        }
        None
    }

    fn is_executable(path: &Path) -> bool {
        let Ok(meta) = std::fs::metadata(path) else {
            return false;
        };
        if !meta.is_file() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            meta.permissions().mode() & 0o111 != 0
        }
        #[cfg(not(unix))]
        {
            true
        }
    }
}
```

### 3.3 `src/plugin/driver.rs`（无死锁并发读写 + `kill_on_drop` 超时强杀）
```rust
use crate::ai::redactor::redact_free_text;
use crate::{Result, RupostError};
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

pub struct PluginDriver;

impl PluginDriver {
    /// 并发写入 stdin 并读取 stdout/stderr，避免 >64KB 管道缓冲区死锁；
    /// `kill_on_drop(true)` 保证超时丢弃 future 时内核立即终止子进程。
    pub async fn execute_json<In, Out>(
        executable_path: &Path,
        input: &In,
        timeout: Duration,
    ) -> Result<Out>
    where
        In: serde::Serialize + Sync,
        Out: serde::de::DeserializeOwned,
    {
        let input_bytes = serde_json::to_vec(input)
            .map_err(|e| RupostError::Other(format!("序列化插件输入失败: {e}")))?;

        let mut child = Command::new(executable_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                RupostError::Other(format!(
                    "无法启动插件进程 {}: {}",
                    executable_path.display(),
                    e
                ))
            })?;

        let mut stdin = child.stdin.take().expect("stdin piped");
        let mut stdout = child.stdout.take().expect("stdout piped");
        let mut stderr = child.stderr.take().expect("stderr piped");

        let io_task = async {
            let write_fut = async move {
                let res = stdin.write_all(&input_bytes).await;
                let _ = stdin.shutdown().await;
                drop(stdin); // 显式关闭发送 EOF
                res
            };

            let read_out_fut = async {
                let mut buf = Vec::new();
                stdout.read_to_end(&mut buf).await.map(|_| buf)
            };

            let read_err_fut = async {
                let mut buf = Vec::new();
                stderr.read_to_end(&mut buf).await.map(|_| buf)
            };

            let (write_res, out_res, err_res, status_res) =
                tokio::join!(write_fut, read_out_fut, read_err_fut, child.wait());
            (write_res, out_res, err_res, status_res)
        };

        let (_, out_res, err_res, status_res) = tokio::time::timeout(timeout, io_task)
            .await
            .map_err(|_| {
                RupostError::Other(format!(
                    "插件 {} 执行超时（超过 {:?}），已终止",
                    executable_path.display(),
                    timeout
                ))
            })?;

        let status = status_res
            .map_err(|e| RupostError::Other(format!("等待插件退出状态失败: {e}")))?;
        let stdout_bytes = out_res
            .map_err(|e| RupostError::Other(format!("读取插件 stdout 失败: {e}")))?;
        let stderr_bytes = err_res.unwrap_or_default();

        if !status.success() {
            let err_text = redact_free_text(&String::from_utf8_lossy(&stderr_bytes));
            return Err(RupostError::Other(format!(
                "插件 {} 非零退出 (code {:?}): {}",
                executable_path.display(),
                status.code(),
                err_text.trim()
            )));
        }

        serde_json::from_slice::<Out>(&stdout_bytes).map_err(|e| {
            let preview: String = String::from_utf8_lossy(&stdout_bytes)
                .chars()
                .take(200)
                .collect();
            RupostError::Other(format!(
                "插件 {} 输出的 stdout 不是合法 JSON ({e})，前 200 字符预览: {}",
                executable_path.display(),
                redact_free_text(&preview)
            ))
        })
    }
}
```

---

## 4. 分阶段实施路线图与 TDD 验收门禁

```
[ Phase 0: JSON 请求体原文保真与 Request.raw_body 基石 ] (0.2 天)
  ├─ 修改 src/http/request.rs 与 src/http/request_builder.rs
  ├─ 单测锁定：键顺序/空白/尾随零 100% 原样保留，Content-Type 子参数不丢失
  └─ JJ 提交: fix(http): preserve exact json body bytes and content-type parameters

[ Phase 1: 核心 SPI、信任白名单注册表与无死锁 Stdio 驱动 ] (0.5 天)
  ├─ 新建 src/plugin/{mod.rs, types.rs, registry.rs, driver.rs}
  ├─ 单测锁定：
  │   1. 恶意名称 `../../sh` 拦截；
  │   2. 未开启 allow_project_plugins 时拒绝加载本地 .rupost/plugins；
  │   3. >128KB 请求体并发读写不死锁；
  │   4. 超时子进程通过 kill_on_drop 确定性回收。
  └─ JJ 提交: feat(plugin): add hardened stdio plugin driver and trust-aware registry

[ Phase 2: @plugin 元数据解析与 HttpRunner 签名钩子接线 ] (0.5 天)
  ├─ src/parser/metadata.rs 支持 `@plugin <name> [k=v ...]`
  ├─ src/variable/resolver.rs 对 `parsed.metadata.plugins` 的参数值做变量替换
  ├─ src/runner/protocol/http_runner.rs 在中间件之后、启动网络计时器之前调用 sign 插件
  ├─ 单测与 Wiremock 集成测试：验证插件注入的 Authorization 头成功送达服务端，且插件耗时不污染响应 duration
  └─ JJ 提交: feat(runner): wire @plugin sign hook after middlewares with timing isolation

[ Phase 3: rupost import 外部插件扩展 ] (0.5 天)
  ├─ src/importer/mod.rs 在非内置格式时调用 `rupost-import-<format>` 转换 JSON IR -> ParsedRequest
  ├─ 单测验证：通过测试桩脚本将自定义 JSON 转换为标准 Markdown/HTTP 请求块
  └─ JJ 提交: feat(importer): support external format importer plugins via stdio json ir
```

---

## 5. 全量验收命令清单

每完成一个 Phase，执行：
```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
bash tests/verify_features.sh
bash examples/run_all.sh
```
