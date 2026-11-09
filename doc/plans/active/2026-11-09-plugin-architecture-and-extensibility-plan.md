# RuPost 微内核插件架构与扩展生态实施方案 (Plugin SPI & Extensibility Architecture)

- **文档状态**: Approved / Ready for Execution
- **创建日期**: 2026-11-09
- **归档路径**: `doc/plans/active/2026-11-09-plugin-architecture-and-extensibility-plan.md`
- **目标执行者**: AI 研发智能体（含 Flash 级模型）/ 人类开发者
- **关联上级计划**: [`doc/plans/active/2026-11-09-full-scenario-architecture-review-and-roadmap.md`](file:///Users/zsyzzx/project/rust/rupost/doc/plans/active/2026-11-09-full-scenario-architecture-review-and-roadmap.md)
- **设计范式借鉴**: **DeepSeek Harness (`dsh`)**、Git Credential Helper、Docker CLI Plugins

---

## 0. 设计哲学与核心原则

1. **微内核纯粹性（Micro-Kernel Purity）**：
   - `rupost` 核心只负责**文档 AST 编译、异步并发 DAG 调度、微米级网络时序探测与基础断言**；
   - 所有的第三方云厂商加签（AWS SigV4, 阿里云/腾讯云签名）、特定网关封包（Lambda RIE, API Gateway Proxy Event）、以及纷繁复杂的外部导入格式（Postman, OpenAPI 3.x, HAR）**一律作为外置插件接入**；
   - 核心二进制体积增加严格控制在 0KB 级（无额外 C 依赖、无外部云厂商重型 SDK）。
2. **约定大于配置（Convention over Configuration）**：
   - 插件无需动态链接库（`.so`/`.dylib`），采用标准 Unix Stdio 进程管道进行 JSON 通信；
   - 开发者可使用任意语言（Python, Node.js, Go, Shell, Rust）在 10 分钟内编写出一个生产级插件。
3. **强类型中间表示（IR-Driven）**：
   - 所有的 Import 插件最终只产出唯一的 `ParsedRequest` / `ParsedFile` AST；
   - 所有的 Signer/Envelope 插件只处理标准化的 `PluginRequestPayload` 与 `PluginResponsePayload`。

---

## 1. 整体架构与分层设计 (Clean Architecture)

```
┌────────────────────────────────────────────────────────────────────────────────────────┐
│                        .http / .md 活文档 (测试用例)                                   │
│  GET https://api.my-cloud.com/v1/orders                                               │
│  @plugin aws-sigv4 region=us-east-1 service=lambda                                     │
│  @envelope aws-apigw-v2                                                                │
└──────────────────────────────────────────┬─────────────────────────────────────────────┘
                                           │
                                           ▼
┌────────────────────────────────────────────────────────────────────────────────────────┐
│                              rupost 核心调度引擎                                       │
│                                                                                        │
│   1. AST 编译 ──> 2. 变量解析 ──> 3. Plugin Dispatcher ──> 4. 发包与探测 ──> 5. 断言  │
│                                           │                                            │
└───────────────────────────────────────────┼────────────────────────────────────────────┘
                                            │
                        ┌───────────────────┴───────────────────┐
                        │ Stdio IPC (标准输入输出 JSON 管道)      │
                        └───────────────────┬───────────────────┘
                                            │
         ┌──────────────────────────────────┼──────────────────────────────────┐
         ▼                                  ▼                                  ▼
┌─────────────────────────┐     ┌─────────────────────────┐     ┌─────────────────────────┐
│   rupost-plugin-aws     │     │  rupost-plugin-apigw    │     │  rupost-import-postman  │
│    (Python / Boto3)     │     │      (Go / Node)        │     │     (TypeScript)        │
├─────────────────────────┤     ├─────────────────────────┤     ├─────────────────────────┤
│ • 计算 AWS SigV4 签名   │     │ • 组装 Lambda Event JSON │     │ • 解析 Postman JSON     │
│ • 注入 Authorization 头 │     │ • 解包 Lambda 响应体    │     │ • 转换输出 ParsedFile   │
└─────────────────────────┘     └─────────────────────────┘     └─────────────────────────┘
```

---

## 2. 插件发现机制与命名规约 (Discovery & Naming)

### 2.1 命名规约
* **运行时加签与封包插件**：可执行文件命名为 `rupost-plugin-<NAME>`
  * 示例：`rupost-plugin-aws-sigv4`、`rupost-plugin-wechat-pay`
* **格式导入转换插件**：可执行文件命名为 `rupost-import-<NAME>`
  * 示例：`rupost-import-postman`、`rupost-import-openapi`、`rupost-import-har`

### 2.2 搜索路径优先级 (Search Paths)
当在测试用例中声明 `@plugin <NAME>` 或执行 `rupost import <NAME>` 时，`PluginRegistry` 严格按以下次序搜索可执行文件：
1. **当前项目级私有目录**：`./.rupost/plugins/`
2. **用户全局配置目录**：`~/.rupost/plugins/`
3. **系统环境变量 PATH**：从 `$PATH` 中自动匹配可执行文件。

*跨平台保护：在 Windows 平台自动追加 `.exe`, `.cmd`, `.bat` 扩展名探测。*

---

## 3. 进程间通信协议 (Stdio IPC Protocol Specification)

所有插件遵循一套极简的 JSON 标准输入/输出契约（类似 Language Server Protocol / Git Credential Helper 机制）。

### 3.1 运行时加签/突变协议 (`Signer Hook`)

#### 输入 (写入插件的 `stdin`)
```json
{
  "protocol_version": "1.0",
  "action": "sign",
  "method": "POST",
  "url": "https://xyz.lambda-url.us-east-1.on.aws/orders",
  "headers": {
    "content-type": "application/json",
    "x-custom-trace": "trace-123"
  },
  "body": "{\"item_id\": 4821}",
  "plugin_args": {
    "region": "us-east-1",
    "service": "lambda"
  }
}
```

#### 输出 (插件向 `stdout` 输出，单行或格式化 JSON)
```json
{
  "success": true,
  "extra_headers": {
    "authorization": "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20261109/us-east-1/lambda/aws4_request...",
    "x-amz-date": "20261109T143000Z"
  },
  "mutated_body": null,
  "warnings": []
}
```
*注：若插件报错，输出 `{"success": false, "error": "AWS credentials not found in environment"}`，微内核将其作为请求级失败上报。*

### 3.2 网关封包与解包协议 (`Envelope Hook`)

#### Pack 阶段 (包装 HTTP 请求)
* **输入 Action**: `"pack"`
* **插件输出**:
  ```json
  {
    "success": true,
    "mutated_method": "POST",
    "mutated_url": "http://localhost:8080/2015-03-31/functions/function/invocations",
    "mutated_body": "{\"rawPath\":\"/orders\",\"headers\":{...},\"body\":\"{\\\"item_id\\\":4821}\"}"
  }
  ```

#### Unpack 阶段 (解包云函数/网关响应)
* **输入 Action**: `"unpack"`，携带真实响应：
  ```json
  {
    "protocol_version": "1.0",
    "action": "unpack",
    "status": 200,
    "headers": {"content-type": "application/json"},
    "body": "{\"statusCode\": 201, \"headers\": {\"x-resp\": \"ok\"}, \"body\": \"{\\\"order_id\\\": \\\"OD-99\\\"}\"}"
  }
  ```
* **插件输出**:
  ```json
  {
    "success": true,
    "unpacked_status": 201,
    "unpacked_headers": {"x-resp": "ok"},
    "unpacked_body": "{\"order_id\": \"OD-99\"}"
  }
  ```

### 3.3 导入器协议 (`Importer Hook`)
* **调用形态**：`rupost import postman ./collection.json`
* **执行命令**：`rupost-import-postman --file ./collection.json`
* **输出**：插件直接向 `stdout` 打印解析生成的标准 `.http` 语法字符串；`rupost` 负责落盘并保持代码格式规范。

---

## 4. 核心代码设计与文件归宿

### 4.1 新建 `src/plugin/types.rs`
```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 传递给插件的请求上下文
#[derive(Debug, Clone, Serialize)]
pub struct PluginSignRequest<'a> {
    pub protocol_version: &'static str,
    pub action: &'static str,
    pub method: &'a str,
    pub url: &'a str,
    pub headers: HashMap<String, String>,
    pub body: Option<&'a str>,
    pub plugin_args: HashMap<String, String>,
}

/// 插件加签返回结果
#[derive(Debug, Clone, Deserialize)]
pub struct PluginSignResponse {
    pub success: bool,
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
    #[serde(default)]
    pub mutated_body: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// 插件解包请求
#[derive(Debug, Clone, Serialize)]
pub struct PluginUnpackRequest<'a> {
    pub protocol_version: &'static str,
    pub action: &'static str,
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: &'a str,
    pub plugin_args: HashMap<String, String>,
}

/// 插件解包返回结果
#[derive(Debug, Clone, Deserialize)]
pub struct PluginUnpackResponse {
    pub success: bool,
    #[serde(default)]
    pub unpacked_status: Option<u16>,
    #[serde(default)]
    pub unpacked_headers: HashMap<String, String>,
    #[serde(default)]
    pub unpacked_body: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}
```

### 4.2 新建 `src/plugin/driver.rs`
负责子进程执行、Stdio 读写、超时熔断与安全沙箱防护：

```rust
use crate::{Result, RupostError};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

pub struct PluginDriver;

impl PluginDriver {
    /// 执行插件进程，写入 stdin 并捕获 stdout，设置严密超时熔断（默认 5s）
    pub async fn execute_pipe(
        executable_path: &PathBuf,
        input_json: &str,
        timeout: Duration,
    ) -> Result<String> {
        let mut child = Command::new(executable_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| RupostError::Other(format!("无法拉起插件进程 {}: {}", executable_path.display(), e)))?;

        // 写入 stdin
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input_json.as_bytes()).await
                .map_err(|e| RupostError::Other(format!("向插件写入数据失败: {}", e)))?;
        }

        // 异步等待并附带超时护栏
        let wait_result = tokio::time::timeout(timeout, child.wait_with_output()).await;

        match wait_result {
            Ok(Ok(output)) => {
                if !output.status.success() {
                    let err_msg = String::from_utf8_lossy(&output.stderr);
                    return Err(RupostError::Other(format!(
                        "插件执行退出异常 (code {:?}): {}",
                        output.status.code(),
                        err_msg.trim()
                    )));
                }
                let stdout_str = String::from_utf8_lossy(&output.stdout).to_string();
                Ok(stdout_str)
            }
            Ok(Err(e)) => Err(RupostError::Other(format!("等待插件进程失败: {}", e))),
            Err(_) => {
                // 超时强制终结进程
                let _ = child.kill().await;
                Err(RupostError::Other(format!(
                    "插件执行超时 (超过 {:?})，已被强制终止",
                    timeout
                )))
            }
        }
    }
}
```

### 4.3 新建 `src/plugin/registry.rs`
负责插件的就地探测与路径缓存：
```rust
use std::path::PathBuf;

pub struct PluginRegistry;

impl PluginRegistry {
    /// 寻找指定名称的插件可执行文件
    pub fn find_plugin(plugin_name: &str, prefix: &str) -> Option<PathBuf> {
        let bin_name = format!("{prefix}-{plugin_name}");
        
        // 1. 本地 ./.rupost/plugins/
        let local_path = PathBuf::from(".rupost/plugins").join(&bin_name);
        if local_path.is_file() {
            return Some(local_path);
        }

        // 2. 用户全局 ~/.rupost/plugins/
        if let Some(home) = dirs::home_dir() {
            let user_path = home.join(".rupost/plugins").join(&bin_name);
            if user_path.is_file() {
                return Some(user_path);
            }
        }

        // 3. 环境变量 PATH 搜索
        if let Ok(path_var) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path_var) {
                let candidate = dir.join(&bin_name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }

        None
    }
}
```

### 4.4 AST 指令与执行器生命周期挂载
1. **元数据指令解析 (`src/parser/metadata.rs`)**：
   增加 `@plugin <name> [key=value ...]`：
   ```rust
   // 挂载到 RequestMetadata
   pub struct PluginInvocation {
       pub name: String,
       pub args: HashMap<String, String>,
   }
   ```
2. **HTTP 执行器集成 (`src/runner/protocol/http_runner.rs`)**：
   在 `client.execute_raw(request)` 发包**紧前**：
   * 检查 `parsed.metadata.plugins`；
   * 调用 `PluginDriver::execute_pipe` 运行插件并拿到 `extra_headers`；
   * 将加签头合并到待发包的 `Request` 中。
3. **响应解包集成**：
   在收到响应后，若存在 `@envelope` 或解包插件，调用解包方法替换 `response.status`、`response.headers`、`response.body`。

---

## 5. 官方参考插件实现范例 (Reference Dogfooding)

在仓库提供参考实现，作为开发者模版与 E2E 验证桩：

### 范例 A: Python 版 AWS SigV4 签名插件 (`examples/plugins/rupost-plugin-aws-sigv4`)
```python
#!/usr/bin/env python3
"""
RuPost 官方参考插件: AWS SigV4 签名器
依赖: pip install boto3 botocore
"""
import sys, json, os

def main():
    try:
        raw_input = sys.stdin.read()
        if not raw_input.strip():
            return
        req = json.loads(raw_input)
    except Exception as e:
        sys.stderr.write(f"Invalid JSON input: {e}\n")
        sys.exit(1)

    import boto3
    from botocore.auth import SigV4Auth
    from botocore.awsrequest import AWSRequest

    plugin_args = req.get("plugin_args", {})
    region = plugin_args.get("region", os.environ.get("AWS_REGION", "us-east-1"))
    service = plugin_args.get("service", "lambda")

    session = boto3.Session()
    creds = session.get_credentials()
    if not creds:
        output = {"success": False, "error": "AWS credentials not found"}
        print(json.dumps(output))
        return

    aws_req = AWSRequest(
        method=req["method"],
        url=req["url"],
        data=req.get("body") or "",
        headers=req.get("headers", {})
    )
    SigV4Auth(creds, service, region).add_auth(aws_req)

    output = {
        "success": True,
        "extra_headers": dict(aws_req.headers),
        "warnings": []
    }
    print(json.dumps(output))

if __name__ == "__main__":
    main()
```

---

## 6. 分步落地排期与 TDD 实施指南

```
[ Phase 1: 核心 SPI 与 Stdio 通信引擎 ] (0.5 天)
  ├─ 新建 src/plugin/ 模块：types.rs, driver.rs, registry.rs
  ├─ 单元测试: 用模拟 echo/cat 脚本测试 Stdio 序列化、超时熔断与退出码捕获
  └─ JJ 原子提交: feat(plugin): implement stdio ipc driver and plugin registry

[ Phase 2: AST @plugin 解析与发包前加签拦截 ] (0.5 天)
  ├─ src/parser/metadata.rs 支持 @plugin 指令与键值对参数解析
  ├─ src/runner/protocol/http_runner.rs 在 before_request 挂载加签逻辑
  ├─ 集成测试: 挂载测试桩插件，验证 Header 成功被外部进程追加并发送
  └─ JJ 原子提交: feat(runner): integrate @plugin invocation into http lifecycle

[ Phase 3: 格式导入插件化 (rupost import 路由) ] (0.5 天)
  ├─ src/importer/mod.rs 改造: 未知格式时路由至 rupost-import-<name>
  ├─ 集成测试: 编写 dummy importer 脚本，验证通过 stdout 导入 .http AST
  └─ JJ 原子提交: feat(importer): allow external cli-based format importer plugins
```

---

## 7. 质量验收门禁

全套代码落地后，必须通过以下验收卡点：
1. **进程隔离安全**：插件崩溃或输出乱码不能导致 `rupost` 主进程 Panic；
2. **超时绝对熔断**：死循环脚本在达到 5s 阈值后必须被确定性强杀，返回友好超时提示；
3. **三层回归通过**：
   ```bash
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   bash tests/verify_features.sh
   bash examples/run_all.sh
   ```

本设计彻底解耦了云函数、云厂商生态与第三方工具导入格式，使 `rupost` 具备了与 `dsh` 等标杆框架相匹敌的微内核生态扩展力。
