use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use rupost::web::server::{WebServerConfig, create_web_router};
use rupost::web::store::LocalFsDocumentStore;
use rupost::web::types::{
    ApiResponse, ImportFormat, ImportRequest, ImportResponse, MockSimulateRequest,
    MockSimulateResponse, RunBlockRequest, RunBlockResponse, SaveFileRequest, SaveFileResponse,
};
use std::sync::Arc;
use tempfile::tempdir;
use tower::ServiceExt;

#[tokio::test]
async fn test_web_static_assets_serving() {
    let config = WebServerConfig::default();
    let app = create_web_router(config);

    // 1. 请求根路径 / -> 返回 index.html
    let req = Request::builder()
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
        .uri("/api/v1/save")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&save_attempt).unwrap()))
        .unwrap();
    let res = readonly_app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
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
