mod common;

use axum::body::{Body, to_bytes};
use axum::http::{HeaderValue, Request, StatusCode};
use rupost::web::server::{WebServerConfig, create_web_router};
use rupost::web::store::LocalFsDocumentStore;
use rupost::web::types::{
    ApiResponse, ImportFormat, ImportRequest, ImportResponse, MockSimulateRequest,
    MockSimulateResponse, RunBlockRequest, RunBlockResponse, RunSuiteRequest, RunSuiteResponse,
    SaveFileRequest, SaveFileResponse, WorkbenchMeta,
};
use std::sync::Arc;
use tempfile::tempdir;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_web_static_assets_serving() {
    let config = WebServerConfig::default();
    let app = create_web_router(config);

    // 1. 请求根路径 / -> 返回 index.html
    let req = Request::builder()
        .header("host", "127.0.0.1")
        .uri("/")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get("content-type").unwrap(),
        "text/html; charset=utf-8"
    );
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("Rupost Living Workbench"));

    // 2. 请求不存在的静态资产 -> 404
    let req_not_found = Request::builder()
        .header("host", "127.0.0.1")
        .uri("/not_exist_file.xyz")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let res_not_found = app.oneshot(req_not_found).await.unwrap();
    assert_eq!(res_not_found.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_web_document_store_and_save_guards() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().to_path_buf();

    // 写入一个初始活文档
    let initial_file = root.join("api_spec.md");
    std::fs::write(
        &initial_file,
        "# API Spec\n\n```http\nGET http://example.com/api\n```\n",
    )
    .unwrap();

    let store = Arc::new(LocalFsDocumentStore::new(&root));
    let config = WebServerConfig {
        store,
        readonly: false,
        ..Default::default()
    };

    let app = create_web_router(config.clone());

    // 1. GET /api/v1/files -> 列出文件列表
    let req = Request::builder()
        .header("host", "127.0.0.1")
        .uri("/api/v1/files")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let api_res: ApiResponse<Vec<rupost::web::FileEntry>> = serde_json::from_slice(&body).unwrap();
    assert!(api_res.success);
    let files = api_res.data.unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "api_spec.md");

    // 2. GET /api/v1/file?path=api_spec.md -> 获取文件内容
    let req = Request::builder()
        .header("host", "127.0.0.1")
        .uri("/api/v1/file?path=api_spec.md")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let api_res: ApiResponse<String> = serde_json::from_slice(&body).unwrap();
    assert!(api_res.success);
    assert!(api_res.data.unwrap().contains("# API Spec"));

    // 3. POST /api/v1/save -> 正常保存新内容
    let save_req_payload = SaveFileRequest {
        path: "api_spec.md".to_string(),
        content: "# Updated Spec\n\n```http\nPOST http://example.com/create\nContent-Type: application/json\n\n{\"msg\":\"hi\"}\n```\n".to_string(),
    };
    let req = Request::builder()
        .header("host", "127.0.0.1")
        .uri("/api/v1/save")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&save_req_payload).unwrap(),
        ))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let save_res: ApiResponse<SaveFileResponse> = serde_json::from_slice(&body).unwrap();
    assert!(save_res.success);
    assert_eq!(save_res.data.unwrap().verified_blocks, 1);

    // 验证物理文件内容已被更新
    let disk_content = std::fs::read_to_string(&initial_file).unwrap();
    assert!(disk_content.contains("# Updated Spec"));

    // 4. POST /api/v1/save -> 预写入语法校验防线 (Syntax Guard)
    // 故意写入损坏或不合法的 HTTP 请求行（如未定义的方法），验证拦截
    let invalid_syntax_payload = SaveFileRequest {
        path: "api_spec.md".to_string(),
        content: "# Broken Spec\n\n```http\nINVALID_METHOD http://example.com/bad\n```\n"
            .to_string(),
    };
    let req = Request::builder()
        .header("host", "127.0.0.1")
        .uri("/api/v1/save")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&invalid_syntax_payload).unwrap(),
        ))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    // 语法错误应被防线拦截，返回 422 Unprocessable Entity
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // 5. POST /api/v1/save -> 只读模式保护防线 (Read-only Guard)
    let mut readonly_config = config.clone();
    readonly_config.readonly = true;
    let readonly_app = create_web_router(readonly_config);

    let save_attempt = SaveFileRequest {
        path: "api_spec.md".to_string(),
        content: "# New Content".to_string(),
    };
    let req = Request::builder()
        .header("host", "127.0.0.1")
        .uri("/api/v1/save")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&save_attempt).unwrap()))
        .unwrap();
    let res = readonly_app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_web_meta_endpoint_reports_readonly_state() {
    // 1. 默认模式：readonly = false
    let app = create_web_router(WebServerConfig::default());
    let req = Request::builder()
        .uri("/api/v1/meta")
        .method("GET")
        .header("host", "127.0.0.1")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let meta: ApiResponse<WorkbenchMeta> = serde_json::from_slice(&body).unwrap();
    assert!(meta.success);
    assert!(!meta.data.unwrap().readonly);

    // 2. 只读模式：readonly = true（前端据此渲染徽章并置灰写路径按钮）
    let config = WebServerConfig {
        readonly: true,
        ..Default::default()
    };
    let app = create_web_router(config);
    let req = Request::builder()
        .uri("/api/v1/meta")
        .method("GET")
        .header("host", "127.0.0.1")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let meta: ApiResponse<WorkbenchMeta> = serde_json::from_slice(&body).unwrap();
    assert!(meta.data.unwrap().readonly);
}

#[tokio::test]
async fn test_web_readonly_mode_blocks_request_execution() {
    let tmp = tempdir().unwrap();
    let store = Arc::new(LocalFsDocumentStore::new(tmp.path()));
    let config = WebServerConfig {
        store,
        readonly: true,
        ..Default::default()
    };
    let app = create_web_router(config);

    let payload = RunBlockRequest {
        file_path: "spec.md".to_string(),
        raw_block: "GET http://127.0.0.1:59999/internal-probe\n".to_string(),
        env: None,
        extra_variables: None,
    };
    let req = Request::builder()
        .uri("/api/v1/run/block")
        .method("POST")
        .header("host", "127.0.0.1")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "read-only mode must block request execution (SSRF probe defense)"
    );
}

#[tokio::test]
async fn test_web_run_block_native_http_file_parse() {
    // .http 文件单块执行走原生解析路径（ARCH-02）：裸 HTTP 块不再经 Markdown 栅栏包裹
    let config = WebServerConfig::default();
    let app = create_web_router(config);

    let payload = RunBlockRequest {
        file_path: "case.http".to_string(),
        raw_block: "# @name health-check\nGET http://127.0.0.1:59999/health\n".to_string(),
        env: None,
        extra_variables: None,
    };
    let req = Request::builder()
        .uri("/api/v1/run/block")
        .method("POST")
        .header("host", "127.0.0.1")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let run_res: ApiResponse<RunBlockResponse> = serde_json::from_slice(&body).unwrap();
    assert!(run_res.success, "execution envelope must succeed");
    let data = run_res.data.unwrap();
    // 不可达端口 -> 执行引擎结构化返回错误（而非 HTTP 层报错）
    assert!(!data.success);
    assert!(data.error.is_some());
}

#[tokio::test]
async fn test_web_run_block_and_mock_simulate() {
    let config = WebServerConfig::default();
    let app = create_web_router(config);

    // 1. 原位单块调试执行 (/api/v1/run/block)
    // 我们使用本地无法连接的不可达端口，验证执行引擎能正常捕获错误并在 RunBlockResponse 中结构化返回
    let run_block_payload = RunBlockRequest {
        file_path: "spec.md".to_string(),
        raw_block: "GET http://127.0.0.1:59999/test-health\n\n?? status == 200".to_string(),
        env: None,
        extra_variables: None,
    };
    let req = Request::builder()
        .header("host", "127.0.0.1")
        .uri("/api/v1/run/block")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&run_block_payload).unwrap(),
        ))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let run_res: ApiResponse<RunBlockResponse> = serde_json::from_slice(&body).unwrap();
    assert!(run_res.success);
    let data = run_res.data.unwrap();
    assert!(!data.success);
    assert!(data.error.is_some());

    // 2. Mock 仿真规则探针 (/api/v1/mock/simulate)
    let mock_block = r#"```http
GET /api/v1/profile
Accept: application/json

@mock-when header.X-Role == admin
HTTP/1.1 200 OK
Content-Type: application/json

{"role": "admin", "permissions": ["all"]}

@mock-default
HTTP/1.1 403 Forbidden
Content-Type: application/json

{"error": "Forbidden"}
```"#;

    // 2.1 模拟管理员身份探针请求
    let mut headers = std::collections::HashMap::new();
    headers.insert("X-Role".to_string(), "admin".to_string());
    let simulate_req_admin = MockSimulateRequest {
        method: "GET".to_string(),
        path: "/api/v1/profile".to_string(),
        headers,
        query: Default::default(),
        body: None,
        raw_block: mock_block.to_string(),
    };
    let req = Request::builder()
        .header("host", "127.0.0.1")
        .uri("/api/v1/mock/simulate")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&simulate_req_admin).unwrap(),
        ))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let sim_res: ApiResponse<MockSimulateResponse> = serde_json::from_slice(&body).unwrap();
    assert!(sim_res.success);
    let sim_data = sim_res.data.unwrap();
    assert_eq!(sim_data.expected_status, 200);
    assert_eq!(sim_data.matched_variant_name, "variant_1");
    assert!(!sim_data.evaluations.is_empty());
    assert!(sim_data.evaluations[0].passed);

    // 2.2 模拟普通访客身份探针请求 -> 命中 @mock-default
    let simulate_req_guest = MockSimulateRequest {
        method: "GET".to_string(),
        path: "/api/v1/profile".to_string(),
        headers: Default::default(),
        query: Default::default(),
        body: None,
        raw_block: mock_block.to_string(),
    };
    let req = Request::builder()
        .header("host", "127.0.0.1")
        .uri("/api/v1/mock/simulate")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&simulate_req_guest).unwrap(),
        ))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let sim_res: ApiResponse<MockSimulateResponse> = serde_json::from_slice(&body).unwrap();
    assert!(sim_res.success);
    let sim_data = sim_res.data.unwrap();
    assert_eq!(sim_data.expected_status, 403);
    assert_eq!(sim_data.matched_variant_name, "default");
}

#[tokio::test]
async fn test_web_multi_format_import_api() {
    let config = WebServerConfig::default();
    let app = create_web_router(config);

    // 1. POST /api/v1/import (Auto / cURL)
    let curl_import_req = ImportRequest {
        format: ImportFormat::Auto,
        content: "curl -X POST https://api.myservice.io/v1/charge -H 'Authorization: Bearer sec_123' -d '{\"amount\": 9900}'".to_string(),
        templatize_host: true,
        generate_mock: true,
    };
    let req = Request::builder()
        .header("host", "127.0.0.1")
        .uri("/api/v1/import")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&curl_import_req).unwrap()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let import_res: ApiResponse<ImportResponse> = serde_json::from_slice(&body).unwrap();
    assert!(import_res.success);
    let data = import_res.data.unwrap();
    assert_eq!(data.detected_format, ImportFormat::Curl);
    assert_eq!(data.total_imported, 1);
    assert!(data.markdown_blocks[0].contains("POST {{base_url}}/v1/charge"));
    assert!(data.markdown_blocks[0].contains("@mock-default"));

    // 2. POST /api/v1/import/curl (明确指向 curl 别名路由)
    let req = Request::builder()
        .header("host", "127.0.0.1")
        .uri("/api/v1/import/curl")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&curl_import_req).unwrap()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let import_res: ApiResponse<ImportResponse> = serde_json::from_slice(&body).unwrap();
    assert!(import_res.success);
    assert_eq!(import_res.data.unwrap().detected_format, ImportFormat::Curl);
}

#[tokio::test]
async fn test_web_security_host_guard() {
    let config = WebServerConfig::default();
    let app = create_web_router(config);

    // 1. 合法 Host (localhost, 127.0.0.1, [::1], sub.localhost) -> 允许通过
    let valid_hosts = vec![
        "localhost:3000",
        "127.0.0.1:8080",
        "[::1]:8080",
        "api.localhost",
    ];
    for host in valid_hosts {
        let req = Request::builder()
            .uri("/")
            .method("GET")
            .header("host", host)
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_ne!(
            res.status(),
            StatusCode::FORBIDDEN,
            "Host {} should be allowed",
            host
        );
    }

    // 2. 恶意外部域名 (DNS Rebinding 攻击企图) -> 拦截返回 403 FORBIDDEN
    let malicious_hosts = vec![
        "evil.attacker.com",
        "attacker.internal:8080",
        "rebind.127.0.0.1.nip.io",
    ];
    for evil_host in malicious_hosts {
        let req = Request::builder()
            .uri("/api/v1/files")
            .method("GET")
            .header("host", evil_host)
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::FORBIDDEN,
            "Malicious host {} must be rejected with 403 Forbidden",
            evil_host
        );
    }
}

#[tokio::test]
async fn test_web_security_host_guard_default_deny_and_allowed_hosts() {
    // 1. 缺失 Host 标头 -> 400 Bad Request（RFC 9112 Default-Deny，杜绝 HTTP/1.0 绕过）
    let app = create_web_router(WebServerConfig::default());
    let req = Request::builder()
        .uri("/")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::BAD_REQUEST,
        "Request without Host header must be rejected with 400"
    );

    // 2. 非法编码的 Host 标头 -> 400 Bad Request
    let app = create_web_router(WebServerConfig::default());
    let malformed_host = HeaderValue::from_bytes(&[0xff, 0xfe, 0xfd]).unwrap();
    let req = Request::builder()
        .uri("/")
        .method("GET")
        .header("host", malformed_host)
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::BAD_REQUEST,
        "Non-UTF-8 Host header must be rejected with 400"
    );

    // 3. 自定义 --allowed-hosts 白名单联动生效：配置内 Host 放行（首页正常 200）
    let config = WebServerConfig {
        allowed_hosts: vec!["dev.internal.example".to_string()],
        ..Default::default()
    };
    let app = create_web_router(config);
    let req = Request::builder()
        .uri("/")
        .method("GET")
        .header("host", "dev.internal.example")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "Custom allowed host must be served normally"
    );

    // 4. 自定义白名单之外的 Host 仍被 403 拦截
    let req = Request::builder()
        .uri("/api/v1/files")
        .method("GET")
        .header("host", "other.example.com")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Host outside custom allowlist must still be rejected"
    );

    // 5. 大小写无关测试 (RFC 9110 §7.2: Case-Insensitive)
    for case_host in ["Localhost:3000", "LOCALHOST", "127.0.0.1:8080"] {
        let req = Request::builder()
            .uri("/")
            .method("GET")
            .header("host", case_host)
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::OK,
            "Host {case_host} must match case-insensitively"
        );
    }
}

#[tokio::test]
async fn test_web_security_path_sandbox_guards() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().to_path_buf();

    // 在根目录下写入一个隐藏敏感文件 .env 和一个正常 md 文件
    std::fs::write(root.join(".env"), "DATABASE_URL=postgres://secret").unwrap();
    std::fs::write(root.join("sample.md"), "# Hello\n").unwrap();

    let store = Arc::new(LocalFsDocumentStore::new(&root));
    let config = WebServerConfig {
        store,
        ..Default::default()
    };
    let app = create_web_router(config);

    // 1. 尝试读取隐藏敏感文件 .env -> 拦截返回 404 NOT_FOUND（防嗅探）
    let req = Request::builder()
        .header("host", "127.0.0.1")
        .uri("/api/v1/file?path=.env")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // 2. 尝试路径穿越跳出工作区 -> 拦截返回 404 NOT_FOUND（防嗅探）
    let req = Request::builder()
        .header("host", "127.0.0.1")
        .uri("/api/v1/file?path=../../etc/passwd")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // 3. 尝试越权保存敏感路径或非文档类型（例如隐藏目录中的合法语法文件）
    //    -> 被 PathSandbox 拦截，SecurityViolation 强类型精准映射为 403 Forbidden
    let evil_save_payload = SaveFileRequest {
        path: ".git/hooks/pre-commit.http".to_string(),
        content: "GET http://localhost/test\n".to_string(),
    };
    let req = Request::builder()
        .header("host", "127.0.0.1")
        .uri("/api/v1/save")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&evil_save_payload).unwrap(),
        ))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "sandbox violation must map to 403 via SecurityViolation"
    );
}

#[tokio::test]
async fn test_web_http_file_syntax_guard() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().to_path_buf();

    let store = Arc::new(LocalFsDocumentStore::new(&root));
    let config = WebServerConfig {
        store,
        ..Default::default()
    };
    let app = create_web_router(config);

    // 1. 保存格式正确的纯 .http 文件（包含变量定义与 Mock 变体）-> 200 OK
    let valid_http_content = r#"
@base_url = https://api.example.com

# @name getUser
GET {{base_url}}/users/1
Accept: application/json

@mock-default
HTTP/1.1 200 OK
Content-Type: application/json

{"id": 1, "name": "Alice"}
"#;
    let save_payload = SaveFileRequest {
        path: "suite.http".to_string(),
        content: valid_http_content.to_string(),
    };
    let req = Request::builder()
        .header("host", "127.0.0.1")
        .uri("/api/v1/save")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&save_payload).unwrap()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // 2. 保存语法损坏的 .http 文件 -> 422 UNPROCESSABLE_ENTITY
    let invalid_http_content = r#"
INVALID_HTTP_METHOD https://api.example.com
"#;
    let bad_save_payload = SaveFileRequest {
        path: "suite.http".to_string(),
        content: invalid_http_content.to_string(),
    };
    let req = Request::builder()
        .header("host", "127.0.0.1")
        .uri("/api/v1/save")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&bad_save_payload).unwrap(),
        ))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_web_import_idn_chinese_domain() {
    let config = WebServerConfig::default();
    let app = create_web_router(config);

    // 针对中文国际化域名 (IDN) 的导入测试，验证不会 Panic 且正确模板化
    let idn_curl = "curl -X GET https://测试.中国/v1/users";
    let req_payload = ImportRequest {
        format: ImportFormat::Curl,
        content: idn_curl.to_string(),
        templatize_host: true,
        generate_mock: true,
    };
    let req = Request::builder()
        .header("host", "127.0.0.1")
        .uri("/api/v1/import")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&req_payload).unwrap()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let resp: ApiResponse<ImportResponse> = serde_json::from_slice(&body).unwrap();
    assert!(resp.success);
    let data = resp.data.unwrap();
    assert!(data.markdown_blocks[0].contains("GET {{base_url}}/v1/users"));
}

#[tokio::test]
async fn test_web_run_suite_batch_execution() {
    // 1. wiremock 提供真实端点：/ok -> 200，/boom -> 500
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200).set_body_string("fine"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/boom"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    // 2. 工作区写入三块套件：通过 / 失败 / 跳过
    let tmp = tempdir().unwrap();
    let base = server.uri();
    let suite = format!(
        r#"
### 1
# @name ok-case
# @assert status == 200
GET {base}/ok

### 2
# @name boom-case
# @assert status == 200
GET {base}/boom

### 3
# @name skipped-case
# @skip
GET {base}/never
"#
    );
    std::fs::write(tmp.path().join("suite.http"), &suite).unwrap();

    let store = Arc::new(LocalFsDocumentStore::new(tmp.path()));
    let config = WebServerConfig {
        store,
        ..Default::default()
    };
    let app = create_web_router(config);

    let payload = RunSuiteRequest {
        path: "suite.http".to_string(),
        env: None,
    };
    let req = Request::builder()
        .uri("/api/v1/run/suite")
        .method("POST")
        .header("host", "127.0.0.1")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let suite_res: ApiResponse<RunSuiteResponse> = serde_json::from_slice(&body).unwrap();
    assert!(suite_res.success);
    let data = suite_res.data.unwrap();

    // 3. 聚合统计与 CLI 报告语义对齐：skipped 独立计数
    assert_eq!(data.total, 3);
    assert_eq!(data.passed, 1);
    assert_eq!(data.failed, 1);
    assert_eq!(data.skipped, 1);
    assert_eq!(data.items.len(), 3);
    assert!(data.items[0].success, "ok-case must pass");
    assert_eq!(data.items[0].name.as_deref(), Some("ok-case"));
    assert!(!data.items[1].success, "boom-case must fail on 500");
    assert!(data.items[1].error.is_none() || data.items[1].status == Some(500));
    assert!(data.items[2].skipped, "skipped case must be skipped");
}

#[tokio::test]
async fn test_web_run_suite_error_shapes() {
    let payload = RunSuiteRequest {
        path: "whatever.http".to_string(),
        env: None,
    };

    // 1. 只读模式 -> 403（SSRF 防线与 run/block 一致）
    let config = WebServerConfig {
        readonly: true,
        ..Default::default()
    };
    let app = create_web_router(config);
    let req = Request::builder()
        .uri("/api/v1/run/suite")
        .method("POST")
        .header("host", "127.0.0.1")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 2. 文档不存在 -> 404
    let tmp = tempdir().unwrap();
    let store = Arc::new(LocalFsDocumentStore::new(tmp.path()));
    let config = WebServerConfig {
        store,
        ..Default::default()
    };
    let app = create_web_router(config);
    let req = Request::builder()
        .uri("/api/v1/run/suite")
        .method("POST")
        .header("host", "127.0.0.1")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // 3. 无任何可执行请求的文档 -> 422（语法门禁：统一 .http 与 .md 空请求语义）
    std::fs::write(tmp.path().join("empty.http"), "# only comments\n").unwrap();
    let no_req_payload = RunSuiteRequest {
        path: "empty.http".to_string(),
        env: None,
    };
    let req = Request::builder()
        .uri("/api/v1/run/suite")
        .method("POST")
        .header("host", "127.0.0.1")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&no_req_payload).unwrap()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // 4. Markdown 中无任何 ```http 代码块 -> 422
    std::fs::write(
        tmp.path().join("empty.md"),
        "# Title\n\nOnly markdown prose, no executable http blocks.\n",
    )
    .unwrap();
    let empty_md_payload = RunSuiteRequest {
        path: "empty.md".to_string(),
        env: None,
    };
    let req = Request::builder()
        .uri("/api/v1/run/suite")
        .method("POST")
        .header("host", "127.0.0.1")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&empty_md_payload).unwrap(),
        ))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_dev_mode_cors_headers_and_preflight() {
    let config = WebServerConfig {
        dev: true,
        ..Default::default()
    };
    let app = create_web_router(config);

    // 1. 预检 OPTIONS：CorsLayer 直接应答，带 allow-origin/allow-methods
    let req = Request::builder()
        .uri("/api/v1/meta")
        .method("OPTIONS")
        .header("host", "127.0.0.1")
        .header("Origin", "http://localhost:5173")
        .header("Access-Control-Request-Method", "POST")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK, "preflight must succeed");
    assert!(
        res.headers().get("access-control-allow-origin").is_some(),
        "preflight must carry allow-origin"
    );

    // 1.5 层序性质锁定：预检先于 Host 守卫——白名单外 Host 的预检仍被 CorsLayer 放行（200）
    let req = Request::builder()
        .uri("/api/v1/meta")
        .method("OPTIONS")
        .header("host", "evil.example.com")
        .header("Origin", "http://localhost:5173")
        .header("Access-Control-Request-Method", "POST")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "preflight short-circuits host guard"
    );

    // 2. 实际跨域 GET：响应带 allow-origin（独立前端可读响应）
    let req = Request::builder()
        .uri("/api/v1/meta")
        .method("GET")
        .header("host", "127.0.0.1")
        .header("Origin", "http://localhost:5173")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.headers().get("access-control-allow-origin").is_some(),
        "actual cross-origin response must carry allow-origin"
    );
}

#[tokio::test]
async fn test_non_dev_mode_has_no_cors_headers() {
    let app = create_web_router(WebServerConfig::default());
    let req = Request::builder()
        .uri("/api/v1/meta")
        .method("GET")
        .header("host", "127.0.0.1")
        .header("Origin", "http://localhost:5173")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.headers().get("access-control-allow-origin").is_none(),
        "non-dev mode must not emit CORS headers (default-deny posture)"
    );
}

/// --dev 安全姿态：非回环 --host 直接拒绝（fail-fast 于 bind 之前）
#[tokio::test]
async fn test_dev_mode_rejects_non_loopback_host() {
    let dir = tempdir().unwrap();
    let store = Arc::new(LocalFsDocumentStore::new(dir.path()));
    let config = WebServerConfig {
        store,
        dev: true,
        ..Default::default()
    };
    // timeout 包裹：若强制被移除，bind 会成功并永驻 serve——失败必须确定性而非挂死
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        rupost::web::run_web_server(config, "0.0.0.0", 0),
    )
    .await
    .expect("must fail fast, not bind and serve forever");
    let err = result.expect_err("non-loopback host must be rejected in dev mode");
    assert!(
        err.to_string().contains("回环") || err.to_string().contains("127.0.0.1"),
        "error must explain loopback-only posture: {err}"
    );
}
