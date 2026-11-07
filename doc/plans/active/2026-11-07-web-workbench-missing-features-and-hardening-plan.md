# Rupost 活文档工作台：全方位功能扩展、安全加固与体验落地综合执行方案 (权威整合版)

> **文档标识**：2026-11-07-web-workbench-missing-features-and-hardening-plan  
> **所属版本**：rupost v0.2.0 (role-alignment-scenario)  
> **状态**：Active / Ready for Implementation  
> **设计准则**：Clean Architecture / First Principles / Defense-in-Depth / RFC 9110 & 9112 / TDD & DRY  

---

## 一、 整体进度复盘与整合蓝图

经过多轮迭代与双子代理（Blue Team 架构专家 & Red Team 安全专家）深度对抗审查，项目已顺利完成**第一步（安全闭环与基线）**的 11 个原子提交（`rwzvszqr..wqqwnrpn`），通过了 534 项单元测试与 14 项端到端 API 集成测试。

本方案将最新审查发现的**微调项（Host 大小写不敏感、空文档 422 统一）**与**第二步（功能扩展与体验）**的核心任务有机整合，形成统一的终极执行文档。

```
┌─────────────────────────────────────────────────────────────────────────────────────────┐
│                           活文档工作台全景执行路线图 (两大阶段)                            │
├────────────────────────────────────────────┬────────────────────────────────────────────┤
│ 第一步：安全闭环与基线 (已达成，待收口微调) │ 第二步：功能扩展与体验 (下一步攻坚重点)     │
├────────────────────────────────────────────┼────────────────────────────────────────────┤
│ [✓] RFC 9112 HostGuard Default-Deny (400)  │ [ ] 领域下沉: 建立独立通用 `src/importer/`  │
│ [✓] allowed_hosts 动态白名单联动生效       │ [ ] HarImporter: 浏览器抓包归档真实解析器   │
│ [✓] PathSandbox 悬空软链接逃逸阻断         │ [ ] PostmanImporter: Collection v2.1 解析器 │
│ [✓] 只读模式 (--readonly) 阻断发包防 SSRF   │ [ ] CLI --dev 标志与 CORS 跨域支持 (自研UI) │
│ [✓] LocalDirAssetProvider 静态扩展名收敛   │ [ ] 前端一键整套测试 (Run Suite) 面板       │
│ [✓] parse_http_header 解除循环依赖         │ [ ] 前端导入向导多格式文件拖拽 (Drag & Drop)│
│ [✓] POST /api/v1/run/suite 批量执行端点    │                                            │
│ [!] 收口微调 1: Host 大小写不敏感 (RFC9110)│                                            │
│ [!] 收口微调 2: run_suite 空活文档统一 422 │                                            │
└────────────────────────────────────────────┴────────────────────────────────────────────┘
```

---

## 二、 第一步收口微调：安全与协议精细化打磨

在进入第二步之前，优先落实 Code Review 提出的两项协议级细节微调：

### 1. Host 校验大小写不敏感匹配 (RFC 9110 §7.2 契约)
* **现状与痛点**：当前 `is_allowed_host` 采用精确字符串比对，若客户端发送 `Host: Localhost:3000` 或大写域名，会被误判拦截为 `403 Forbidden`。
* **技术改进**：在提取 `host_only` 之后，统一进行 ASCII 小写归一化比对（`eq_ignore_ascii_case`），同时对 `custom_allowed` 也做大小写不敏感比对。

```rust
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

    // 严格遵循 RFC 9110 §7.2: Host 标头大小写无关 (Case-Insensitive)
    if host_only.eq_ignore_ascii_case("localhost")
        || host_only == "127.0.0.1"
        || host_only == "::1"
        || host_only == "0.0.0.0"
        || host_only.to_ascii_lowercase().ends_with(".localhost")
    {
        return true;
    }

    custom_allowed.iter().any(|allowed| allowed.eq_ignore_ascii_case(host_only))
}
```

### 2. `run_suite_handler` 空活文档返回语义对齐
* **现状与痛点**：空 `.http` 文档在解析时会报错并返回 `422 Unprocessable Entity`；而完全不含 HTTP 块的空 `.md` 文档当前会返回 `total: 0, items: []` (200 OK)，两端行为不对称。
* **技术改进**：在 `run_suite_handler` 中增加 `if parsed_file.requests.is_empty()` 判定，空文档统一返回 422 语法错误与友好提示：`"Document contains no executable HTTP request blocks"`。

---

## 三、 第二步核心战役：功能扩展与架构下沉

### 1. 通用 `src/importer/` 模块物理下沉 (Clean Architecture)
* **架构重构**：
  导入能力是与协议直接相关的通用领域能力，不仅供 Web 工作台使用，未来 CLI（`rupost import <file>`）亦需复用。
  将原本混杂在 `src/web/importer.rs` 中的转换逻辑物理下沉至独立的 `src/importer/` 根模块：
  ```
  src/importer/
  ├── mod.rs          // 导出 RequestImporter Trait 与 ImportFormat 枚举
  ├── curl.rs         // cURL 命令行解析
  ├── httpie.rs       // HTTPie 命令行解析
  ├── raw.rs          // RFC 7230 原始报文解析
  ├── har.rs          // HAR 1.2 浏览器抓包归档解析器
  └── postman.rs      // Postman Collection v2.1 递归展开解析器
  ```
  `src/web/` 仅保留轻量级的 HTTP API 路由与适配器（`handlers/import.rs`）。

### 2. `HarImporter`：浏览器抓包归档真实解析器
* **核心业务逻辑**：
  1. 采用局部强类型反序列化模型解析标准 HAR 1.2 JSON（避免超大归档引起 `serde_json::Value` 堆膨胀）；
  2. 提取 `log.entries[].request` 中的 Method、URL、Query、Headers 与 postData；
  3. **数据噪声清洗**：过滤 HTTP/2 伪头（`:authority`, `:path` 等）与 `content-length`；
  4. **Host 冲突规避**：若开启 `templatize_host`，强制过滤原抓包报文中的 `Host` 标头，防止请求发往旧的目标地址；
  5. 自动为每个请求生成基于序号的命名锚点 `# @name har_step_N`。

### 3. `PostmanImporter`：Collection v2.1 递归提取解析器
* **核心业务逻辑**：
  1. 递归遍历 `item[]` 目录树（内置 `max_depth = 32` 递归栈保护，防止恶意嵌套爆栈）；
  2. 兼容 Postman URL 为“纯字符串”与“结构化对象”的两种导出格式；
  3. 将环境变量引用 `{{variable}}` 保持为标准 Rupost 占位符；
  4. 提取测试脚本中的基本状态断言（`pm.response.to.have.status(200)` 转换为 `# @assert status == 200`）；
  5. 将文件夹路径转换为用例层级前缀（`# @name 认证模块_微信登录`）。

### 4. CLI `--dev` 模式与 CORS 跨域放行
* **核心业务逻辑**：
  1. 在 `src/cli.rs` 中为 `Commands::Web` 增加 `--dev` 标志（`#[arg(long, help = "开启开发跨域模式，放行前端热重载端口的跨源访问")]`）；
  2. 在 `create_web_router` 中，当 `config.dev_mode == true` 时挂载 `tower_http::cors::CorsLayer::very_permissive()`；
  3. 支持外部前端团队在 `localhost:5173`（Vite）或 `localhost:3000` 独立启动自研 React/Vue 前端，跨域访问 Rupost 核心引擎。

### 5. 前端工作台交互增强 (Run All 与文件拖拽)
* **核心业务逻辑**：
  1. **一键整套执行（Run Suite）面板**：
     - 在左侧文件树或顶栏增加“▶ 运行整篇文档”按钮；
     - 右侧面板支持切换“单块调试”与“套件聚合报告”双视图；
     - 实时渲染套件通过率进度条（Green/Red/Amber 区分 passed/failed/skipped）；
  2. **多格式文件拖拽（Drag & Drop）**：
     - 导入弹窗不仅支持文本粘贴，支持直接将 `.har`、`.json` 文件拖入弹窗；
     - 前端通过 `FileReader` 自动读入并填充文本框，自动根据文件扩展名切换下拉选项。

---

## 四、 核心落地示例代码蓝图

### 示例 1: `is_allowed_host` 大小写不敏感修复 (`src/web/server.rs`)

```rust
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

    // 遵循 RFC 9110: Host 标头大小写无关 (Case-Insensitive)
    if host_only.eq_ignore_ascii_case("localhost")
        || host_only == "127.0.0.1"
        || host_only == "::1"
        || host_only == "0.0.0.0"
        || host_only.to_ascii_lowercase().ends_with(".localhost")
    {
        return true;
    }

    custom_allowed.iter().any(|allowed| allowed.eq_ignore_ascii_case(host_only))
}
```

---

### 示例 2: `run_suite_handler` 空活文档 422 校验 (`src/web/handlers/run.rs`)

```rust
    // 2. 统一门面解析整篇活文档（.md / .http 按扩展名分发）
    let parsed_file = match parse_document_by_path(&req.path, &content) {
        Ok(f) => f,
        Err(e) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ApiResponse::err(format!("Failed to parse document: {}", e))),
            );
        }
    };

    // 统一空文档语义：若文档内无任何可执行请求，统一返回 422
    if parsed_file.requests.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiResponse::err("Document contains no executable HTTP request blocks")),
        );
    }
```

---

### 示例 3: `HarImporter` 强类型清洗与模板化 (`src/importer/har.rs`)

```rust
use crate::http::url::UrlTemplatizer;
use crate::parser::types::{ParsedRequest, RequestMetadata};
use crate::Result;
use serde::Deserialize;

#[derive(Deserialize)]
struct HarRoot {
    log: HarLog,
}

#[derive(Deserialize)]
struct HarLog {
    entries: Vec<HarEntry>,
}

#[derive(Deserialize)]
struct HarEntry {
    request: HarRequest,
}

#[derive(Deserialize)]
struct HarRequest {
    method: String,
    url: String,
    headers: Vec<HarHeader>,
    #[serde(rename = "postData")]
    post_data: Option<HarPostData>,
}

#[derive(Deserialize)]
struct HarHeader {
    name: String,
    value: String,
}

#[derive(Deserialize)]
struct HarPostData {
    text: Option<String>,
}

pub struct HarImporter;

impl HarImporter {
    pub fn parse(content: &str, templatize_host: bool) -> Result<Vec<ParsedRequest>> {
        let har: HarRoot = serde_json::from_str(content)
            .map_err(|e| crate::error::RupostError::ParseError(format!("Invalid HAR JSON: {}", e)))?;

        let mut requests = Vec::with_capacity(har.log.entries.len());

        for (idx, entry) in har.log.entries.into_iter().enumerate() {
            let req = entry.request;
            if req.url.is_empty() {
                continue;
            }

            let url = if templatize_host {
                UrlTemplatizer::templatize_origin(&req.url, "{{base_url}}")
            } else {
                req.url
            };

            let mut headers = Vec::new();
            for h in req.headers {
                // 过滤 HTTP/2 伪头与系统自动生成的 Content-Length
                if h.name.starts_with(':') || h.name.eq_ignore_ascii_case("content-length") {
                    continue;
                }
                // 若模板化 Host，必须剔除旧抓包报文中的 Host 头
                if templatize_host && h.name.eq_ignore_ascii_case("host") {
                    continue;
                }
                headers.push((h.name, h.value));
            }

            let body = req.post_data.and_then(|p| p.text);
            let mut meta = RequestMetadata::default();
            meta.name = Some(format!("har_step_{}", idx + 1));

            requests.push(ParsedRequest {
                method: Some(req.method),
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

## 五、 分阶段实施与验证计划

1. **Step 1: 落实微调项与 TDD 验证 (快速收尾第一步)**
   - 修复 `is_allowed_host` 大小写匹配（单测覆盖 `Host: Localhost:3000`）；
   - 在 `run_suite_handler` 中增加空文档 422 校验（单测覆盖空 Markdown 文档）；
   - 验证全量 538+ 项测试全绿并原子化提交。
2. **Step 2: 领域下沉与通用 `importer` 模块构建**
   - 新建 `src/importer/` 目录，将 cURL、HTTPie、RawHTTP 以及新增的 `HarImporter`、`PostmanImporter` 统一归入；
   - 补充对抗性单元测试（恶意的深度嵌套 Postman JSON、大文件 HAR、表单与 JSON 混合）。
3. **Step 3: CLI `--dev` (CORS) 与工作台前端体验升级**
   - CLI 接入 `--dev`，Router 挂载 `tower_http::cors`；
   - 前端增加“套件聚合报告（Run Suite）”控制条与结果卡片；
   - 导入弹窗增加文件拖拽与自动加载。
4. **Step 4: 全量三层质量验收回归**
   - `cargo test` 全量通过；
   - `tests/verify_features.sh` 冒烟测试全绿；
   - `examples/run_all.sh` 15 个示例全部通过；
   - `cargo clippy --all-targets --all-features -- -D warnings` 与 `cargo fmt --check` 保持双零。
