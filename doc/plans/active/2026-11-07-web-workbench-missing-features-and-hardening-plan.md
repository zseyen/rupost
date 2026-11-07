# Rupost 活文档工作台：缺失功能查漏补缺与安全架构加固综合执行方案

> **文档标识**：2026-11-07-web-workbench-missing-features-and-hardening-plan  
> **所属版本**：rupost v0.2.0 (role-alignment-scenario)  
> **状态**：Active / Ready for Implementation  
> **设计准则**：Clean Architecture / First Principles / Defense-in-Depth / TDD & DRY  

---

## 一、 方案背景与查漏补缺全景

在完成《活文档工作台 (Web Phase 1) 与全方位定制化架构综合方案》的基础骨干开发与双子代理对抗审查后，我们对照方案承诺与“开发、调试、发布”三大生命周期场景进行了全维度的**查漏补缺**。

现存待攻坚的内容明确划分为两大平行战役：
1. **战役 A（产品核心功能完备）**：补齐规划中承诺的套件批量执行端点、真实多格式向导装入器（HAR、Postman、OpenAPI）、CLI `--dev` 跨域 CORS 支持与前端套件运行交互；
2. **战役 B（系统安全与架构加固）**：落实 Code Review 审查捕获的 Default-Deny Host 门禁闭环、`PathSandbox` 悬空软链接逃逸阻断、只读模式防 SSRF、静态资产扩展名收敛以及 Parser 内部解耦。

```
┌────────────────────────────────────────────────────────────────────────┐
│                        Rupost 活文档工作台查漏补缺全景图                  │
├───────────────────────────────────┬────────────────────────────────────┤
│ 战役 A: 产品核心能力闭环 (Functional)│ 战役 B: 边界防御与架构加固 (Security) │
├───────────────────────────────────┼────────────────────────────────────┤
│ 1. POST /api/v1/run/suite (整套执行)│ 1. HostGuard Default-Deny 闭环 (400) │
│ 2. 多格式装入: HAR 浏览器抓包归档   │ 2. allowed_hosts 动态白名单联动生效  │
│ 3. 多格式装入: Postman Collection │ 3. PathSandbox 悬空软链接逃逸阻断  │
│ 4. 多格式装入: OpenAPI 3.0 / Swagger│ 4. 只读模式 (--readonly) 发包阻断   │
│ 5. CLI --dev 与 CORS 跨域支持     │ 5. 外部静态目录 (LocalDir) 类型收敛│
│ 6. 前端一键整套测试 (Run All) 面板│ 6. 解除 Parser 内部反向循环引用    │
│ 7. 前端多格式文件拖拽上传 (Drag&Drop)│ 7. 领域错误强类型化 (SecurityError) │
└───────────────────────────────────┴────────────────────────────────────┘
```

---

## 二、 战役 A：产品核心能力补齐与实现方案

### 1. 整篇文档套件批量执行端点 (`POST /api/v1/run/suite`)
* **业务场景**：开发者不仅需要单块原位调试，更需要一键跑通整篇活文档包含的所有接口与链路断言，获取批量汇总指标。
* **接口规范**：
  - **输入**：`RunSuiteRequest { path: String, env: Option<String> }`
  - **输出**：`RunSuiteResponse { total: usize, passed: usize, failed: usize, skipped: usize, duration_ms: u64, items: Vec<SuiteItemResult> }`
* **架构接入**：
  - 读取指定路径的文档内容（复用 `DocumentStore::read_document`）；
  - 使用 `parse_document_by_path` 提取全部请求；
  - 复用 `TestExecutor::with_ephemeral_cookies()` 执行链，输出聚合报告。

### 2. 真实多格式向导装入器 (Importer)
* **业务场景**：满足开发者将现有接口资产（浏览器抓包、Postman 用例库、Swagger 规范）零门槛迁移到 Rupost 活文档。
* **扩展点设计**：
  - `HarImporter`：解析标准 HAR 1.2 JSON，提取 `log.entries[].request`，保留 Method、URL、Query、Headers、PostData MIME 及文本，自动生成带有序号或 URL 尾段的 `@name`；
  - `PostmanImporter`：解析 Postman Collection v2.1 JSON，递归遍历 `item[]` 树，提取请求参数、Headers、Body Raw JSON，将简单的 `pm.response.to.have.status(200)` 转换为 `# @assert status == 200`；
  - `OpenApiImporter`：解析 OpenAPI 3.0 JSON，遍历 `paths`，生成带有参数说明与示例请求体的 HTTP 契约块，并根据 `responses.200` 自动生成 `@mock-default` 结构。

### 3. CLI `--dev` 模式与 CORS 跨域支持
* **业务场景**：支持企业前端团队独立启动 Vite / Webpack 开发自研 SPA，直接跨域与 `rupost web` API 交互。
* **参数与中间件**：
  - CLI 新增 `--dev` 标志；
  - 启动时若检测到 `config.dev_mode`，挂载 `tower_http::cors::CorsLayer::very_permissive()`（允许携带任意请求头与跨域 Origin）；
  - 非 dev 模式下保持严格同源。

---

## 三、 战役 B：系统安全防线与架构加固方案

### 1. HostGuard 缺失 Host 标头拦截与 Default-Deny (SEC-01 & SEC-02)
* **RFC 9112 契约**：所有 HTTP/1.1 请求必须包含 `Host` 标头，缺失或非法直接返回 `400 Bad Request`。
* **动态白名单联动**：提取 `State(state)`，将请求 Host 与默认基准及 `state.allowed_hosts` 联合判定，彻底防御 DNS Rebinding 并保障内网访问。

### 2. `PathSandbox` 悬空软链接逃逸阻断 (SEC-03)
* **风险防范**：使用 `candidate.symlink_metadata().is_ok()` 替代 `candidate.exists()`，确保指向沙箱外不存在目标的悬空符号链接直接被识别并拒绝，彻底杜绝穿透外写。

### 3. 只读模式发包阻断与 SSRF 防护 (SEC-04)
* **权限保护**：在 `run_block_handler` 与 `run_suite_handler` 起始位置增加 `if state.readonly { return (StatusCode::FORBIDDEN, ...); }`，切断一切网络发包通道。

### 4. 外部静态目录挂载类型收敛 (SEC-05)
* **数据防泄漏**：为 `LocalDirAssetProvider` 挂载 `with_extensions(&["html", "css", "js", "mjs", "json", "svg", "png", "jpg", "ico", "woff", "woff2"])` 白名单。

### 5. 解除 Parser 内部反向依赖 (ARCH-01)
* **模块单向流动**：将 `HttpFileParser::parse_header` 提取为独立公共函数 `parse_http_header`，消除 `mock_variant` 对 `http_file` 的反向引用。

---

## 四、 核心落地示例代码 (Implementation Blueprints)

### 示例 1: `run/suite` 整文件批量执行处理器 (`src/web/handlers/run.rs`)

```rust
use crate::parser::parse_document_by_path;
use crate::runner::TestExecutor;
use crate::variable::ConfigLoader;
use crate::web::server::AppState;
use crate::web::types::{ApiResponse, RunSuiteRequest, RunSuiteResponse, SuiteItemResult};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use std::time::Instant;

pub async fn run_suite_handler(
    State(state): State<AppState>,
    Json(req): Json<RunSuiteRequest>,
) -> impl IntoResponse {
    // 1. 只读模式网络发包阻断 (防 SSRF)
    if state.readonly {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::err("Suite execution is disabled in read-only mode")),
        );
    }

    // 2. 读取整篇活文档内容
    let content = match state.store.read_document(&req.path).await {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::err(format!("File not found: {}", e))),
            );
        }
    };

    // 3. 统一使用门面解析整篇活文档
    let parsed_file = match parse_document_by_path(&req.path, &content) {
        Ok(f) => f,
        Err(e) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ApiResponse::err(format!("Failed to parse document: {}", e))),
            );
        }
    };

    // 4. 准备变量上下文
    let config = ConfigLoader::find_and_load().unwrap_or_default();
    let target_env = req.env.as_deref().or(state.default_env.as_deref());
    let mut var_context = ConfigLoader::build_context(&config, target_env, &[], Some(".env"));
    var_context.insert("__default_scheme", "http");

    // 5. 采用内存隔离会话执行整套测试
    let start_time = Instant::now();
    let executor = TestExecutor::with_ephemeral_cookies();
    let results = match executor.execute_all(parsed_file, &mut var_context).await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::err(format!("Execution failed: {}", e))),
            );
        }
    };

    let total = results.len();
    let mut passed = 0;
    let mut failed = 0;
    let mut items = Vec::new();

    for r in results {
        if r.success {
            passed += 1;
        } else {
            failed += 1;
        }
        items.push(SuiteItemResult {
            name: r.name.unwrap_or_else(|| "unnamed".to_string()),
            method: r.method,
            url: r.url,
            status: r.status,
            duration_ms: r.duration.as_millis() as u64,
            success: r.success,
            error: r.error,
        });
    }

    let response = RunSuiteResponse {
        total,
        passed,
        failed,
        skipped: 0,
        duration_ms: start_time.elapsed().as_millis() as u64,
        items,
    };

    (StatusCode::OK, Json(ApiResponse::ok(response)))
}
```

---

### 示例 2: HAR 格式导入解析器 (`src/web/importer.rs`)

```rust
use serde_json::Value;

pub struct HarImporter;

impl HarImporter {
    pub fn parse_har(content: &str, templatize_host: bool) -> Result<Vec<ParsedRequest>> {
        let root: Value = serde_json::from_str(content)
            .map_err(|e| RupostError::ParseError(format!("Invalid HAR JSON: {}", e)))?;

        let entries = root["log"]["entries"]
            .as_array()
            .ok_or_else(|| RupostError::ParseError("Missing log.entries array in HAR".to_string()))?;

        let mut requests = Vec::new();

        for (idx, entry) in entries.iter().enumerate() {
            let req_obj = &entry["request"];
            let method = req_obj["method"].as_str().unwrap_or("GET").to_string();
            let raw_url = req_obj["url"].as_str().unwrap_or("").to_string();

            if raw_url.is_empty() {
                continue;
            }

            let url = if templatize_host {
                UrlTemplatizer::templatize_origin(&raw_url, "{{base_url}}")
            } else {
                raw_url
            };

            let mut headers = Vec::new();
            if let Some(h_arr) = req_obj["headers"].as_array() {
                for h in h_arr {
                    if let (Some(k), Some(v)) = (h["name"].as_str(), h["value"].as_str()) {
                        let k_lower = k.to_lowercase();
                        // 过滤浏览器自动添加的伪头和无用标头
                        if !k_lower.starts_with(':') && k_lower != "content-length" {
                            headers.push((k.to_string(), v.to_string()));
                        }
                    }
                }
            }

            let body = req_obj["postData"]["text"].as_str().map(|s| s.to_string());

            let mut meta = RequestMetadata::default();
            meta.name = Some(format!("har_request_{}", idx + 1));

            requests.push(ParsedRequest {
                method: Some(method),
                url,
                headers,
                body,
                metadata: meta,
                line_number: 1,
                base_path: None,
            });
        }

        Ok(requests)
    }
}
```

---

### 示例 3: Postman Collection v2.1 导入解析器 (`src/web/importer.rs`)

```rust
pub struct PostmanImporter;

impl PostmanImporter {
    pub fn parse_collection(content: &str, templatize_host: bool) -> Result<Vec<ParsedRequest>> {
        let root: Value = serde_json::from_str(content)
            .map_err(|e| RupostError::ParseError(format!("Invalid Postman JSON: {}", e)))?;

        let mut requests = Vec::new();
        Self::extract_items(&root["item"], templatize_host, &mut requests)?;
        Ok(requests)
    }

    fn extract_items(
        items: &Value,
        templatize_host: bool,
        results: &mut Vec<ParsedRequest>,
    ) -> Result<()> {
        if let Some(arr) = items.as_array() {
            for item in arr {
                if let Some(sub_items) = item.get("item") {
                    // 递归处理子目录
                    Self::extract_items(sub_items, templatize_host, results)?;
                } else if let Some(req_obj) = item.get("request") {
                    let name = item["name"].as_str().unwrap_or("Unnamed").to_string();
                    let method = req_obj["method"].as_str().unwrap_or("GET").to_string();

                    // 提取 URL
                    let raw_url = if let Some(url_str) = req_obj["url"].as_str() {
                        url_str.to_string()
                    } else if let Some(raw) = req_obj["url"]["raw"].as_str() {
                        raw.to_string()
                    } else {
                        continue;
                    };

                    let url = if templatize_host {
                        UrlTemplatizer::templatize_origin(&raw_url, "{{base_url}}")
                    } else {
                        raw_url
                    };

                    // 提取 Headers
                    let mut headers = Vec::new();
                    if let Some(h_arr) = req_obj["header"].as_array() {
                        for h in h_arr {
                            if let (Some(k), Some(v)) = (h["key"].as_str(), h["value"].as_str()) {
                                if !h["disabled"].as_bool().unwrap_or(false) {
                                    headers.push((k.to_string(), v.to_string()));
                                }
                            }
                        }
                    }

                    // 提取 Body
                    let body = req_obj["body"]["raw"].as_str().map(|s| s.to_string());

                    let mut meta = RequestMetadata::default();
                    meta.name = Some(name);

                    results.push(ParsedRequest {
                        method: Some(method),
                        url,
                        headers,
                        body,
                        metadata: meta,
                        line_number: 1,
                        base_path: None,
                    });
                }
            }
        }
        Ok(())
    }
}
```

---

### 示例 4: Default-Deny Host 门禁与动态白名单 (`src/web/server.rs`)

```rust
async fn host_guard_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    // 1. 严格践行 RFC 9112: 缺失 Host 标头必须返回 400 Bad Request
    let Some(host_val) = req.headers().get(HOST) else {
        return Err(StatusCode::BAD_REQUEST);
    };

    let Ok(host_str) = host_val.to_str() else {
        return Err(StatusCode::BAD_REQUEST);
    };

    // 2. 校验白名单（本地基础名单 + 用户配置 allowed_hosts）
    if !is_allowed_host(host_str, &state.allowed_hosts) {
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(next.run(req).await)
}

fn is_allowed_host(host_str: &str, custom_allowed: &[String]) -> bool {
    let host_only = if host_str.starts_with('[') {
        host_str
            .split(']')
            .next()
            .map(|s| s.trim_start_matches('['))
            .unwrap_or("")
    } else {
        host_str.split(':').next().unwrap_or("")
    };

    if host_only == "localhost"
        || host_only == "127.0.0.1"
        || host_only == "::1"
        || host_only == "0.0.0.0"
        || host_only.ends_with(".localhost")
    {
        return true;
    }

    custom_allowed.iter().any(|allowed| allowed == host_only)
}
```

---

### 示例 5: `PathSandbox` 悬空软链接逃逸阻断 (`src/security/path.rs`)

```rust
// 使用 symlink_metadata 准确捕获存在但指向外部不存在文件的悬空软链接
if candidate.symlink_metadata().is_ok() {
    let canon = candidate.canonicalize().map_err(RupostError::IoError)?;
    if canon.starts_with(&self.root) {
        Ok(canon)
    } else {
        Err(RupostError::Other(format!(
            "Path traversal attempt rejected via symlink: {}",
            clean.display()
        )))
    }
} else {
    // 逐级追溯已存在祖先目录...
}
```

---

## 五、 分阶段实施与验证计划 (Implementation Roadmap)

1. **Step 1: 安全基线加固先行 (TDD 驱动)**
   - 修复 `host_guard_middleware`（400 拦截缺失 Host，打通 `state.allowed_hosts`）；
   - 修复 `PathSandbox`（`symlink_metadata` 阻断软链接逃逸）；
   - 运行单元测试验证安全门禁无缝闭环。
2. **Step 2: 核心功能落地 (`run/suite` 与多格式装入)**
   - 实现 `run_suite_handler` 并挂载路由 `/api/v1/run/suite`；
   - 在 `importer.rs` 实现 `HarImporter` 与 `PostmanImporter`；
   - 编写多格式装入与批量运行的集成测试用例。
3. **Step 3: CLI 与前端工作台体验强化**
   - CLI 开放 `--dev`（挂载 CORS）与 `--allowed-hosts` 参数；
   - 前端工作台新增“一键运行整套测试”面板与结果统计卡片；
   - 导入弹窗支持文件拖拽解析。
4. **Step 4: 全量三层质量验收回归**
   - `cargo test` 全量通过；
   - `bash tests/verify_features.sh` 冒烟通过；
   - `bash examples/run_all.sh` 15 示例通过；
   - `cargo clippy --all-targets --all-features -- -D warnings` 与 `cargo fmt --check` 保持双零。
