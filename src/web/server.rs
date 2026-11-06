use crate::Result;
use crate::web::assets::{AssetProvider, EmbeddedAssetProvider};
use crate::web::handlers;
use crate::web::store::{DocumentStore, LocalFsDocumentStore};
use axum::Router;
use axum::extract::{DefaultBodyLimit, Path as AxumPath, Request, State};
use axum::http::StatusCode;
use axum::http::header::{CONTENT_TYPE, HOST, HeaderValue};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use std::path::PathBuf;
use std::sync::Arc;

/// Web 工作台配置
#[derive(Clone)]
pub struct WebServerConfig {
    pub store: Arc<dyn DocumentStore>,
    pub assets: Arc<dyn AssetProvider>,
    pub readonly: bool,
    pub working_dir: PathBuf,
    pub default_env: Option<String>,
    pub allowed_hosts: Vec<String>,
}

impl Default for WebServerConfig {
    fn default() -> Self {
        Self {
            store: Arc::new(LocalFsDocumentStore::new(".")),
            assets: Arc::new(EmbeddedAssetProvider),
            readonly: false,
            working_dir: PathBuf::from("."),
            default_env: None,
            allowed_hosts: Vec::new(),
        }
    }
}

/// 路由共享状态
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn DocumentStore>,
    pub assets: Arc<dyn AssetProvider>,
    pub readonly: bool,
    pub default_env: Option<String>,
    pub allowed_hosts: Vec<String>,
}

/// 检查 Host 标头是否属于安全白名单（防御 DNS Rebinding）
fn is_allowed_host(host_str: &str) -> bool {
    let host_only = if host_str.starts_with('[') {
        host_str
            .split(']')
            .next()
            .map(|s| s.trim_start_matches('['))
            .unwrap_or("")
    } else {
        host_str.split(':').next().unwrap_or("")
    };

    host_only == "localhost"
        || host_only == "127.0.0.1"
        || host_only == "::1"
        || host_only == "0.0.0.0"
        || host_only.ends_with(".localhost")
}

/// 防御 DNS Rebinding 的 Host 校验守卫中间件
async fn host_guard_middleware(
    req: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    if let Some(host_val) = req.headers().get(HOST)
        && let Ok(host_str) = host_val.to_str()
        && !is_allowed_host(host_str)
    {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(next.run(req).await)
}

/// 构造统一的 Web Router
pub fn create_web_router(config: WebServerConfig) -> Router {
    let state = AppState {
        store: config.store,
        assets: config.assets,
        readonly: config.readonly,
        default_env: config.default_env,
        allowed_hosts: config.allowed_hosts,
    };

    let api_router = Router::new()
        .route("/files", get(handlers::files::list_files_handler))
        .route("/file", get(handlers::files::get_file_handler))
        .route("/save", post(handlers::save::save_file_handler))
        .route("/run/block", post(handlers::run::run_block_handler))
        .route(
            "/mock/simulate",
            post(handlers::mock::simulate_mock_handler),
        )
        .route("/import", post(handlers::import::import_handler))
        .route("/import/curl", post(handlers::import::import_curl_handler));

    Router::new()
        .nest("/api/v1", api_router)
        .route("/", get(serve_index))
        .route("/{*path}", get(serve_asset))
        .with_state(state)
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(middleware::from_fn(host_guard_middleware))
}

/// 首页根路径直接投递 index.html
async fn serve_index(State(state): State<AppState>) -> Response {
    serve_asset_internal(&state, "index.html")
}

/// 资产静态服务 fallback
async fn serve_asset(State(state): State<AppState>, AxumPath(path): AxumPath<String>) -> Response {
    serve_asset_internal(&state, &path)
}

fn serve_asset_internal(state: &AppState, path: &str) -> Response {
    if let Some((bytes, mime)) = state.assets.get_asset(path) {
        let mut resp = bytes.into_response();
        if let Ok(mime_val) = HeaderValue::from_str(mime) {
            resp.headers_mut().insert(CONTENT_TYPE, mime_val);
        }
        resp
    } else {
        (StatusCode::NOT_FOUND, "Asset not found").into_response()
    }
}

/// 启动活文档工作台 Web 服务器
pub async fn run_web_server(config: WebServerConfig, host: &str, port: u16) -> Result<()> {
    let app = create_web_router(config);
    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("\n  🚀 rupost 活文档工作台启动成功");
    println!("  ➜  地址: http://{}", addr);
    println!("  ➜  按 Ctrl+C 停止服务\n");

    axum::serve(listener, app).await?;
    Ok(())
}
