use crate::Result;
use crate::RupostError;
use crate::http::url::is_loopback_host;
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
    /// 开发者跨域模式（--dev）：挂载宽松 CORS 供独立前端联调；仅允许回环绑定
    pub dev: bool,
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
            dev: false,
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
    /// R-4（免重填）+ WP-1（P0 隔离）：按浏览器会话隔离的执行态资产
    /// （cookie 桥 + 捕获变量；进程内存储，`Cookie: rupost_session=<opaque>`
    /// 划分边界，跨客户端零共享；--dev 宽松 CORS 不构成共享身份的理由）
    pub sessions: Arc<crate::web::session::WebSessionStore>,
    /// WP-R2：Web 复现包输出根（服务端管辖路径，客户端零路径输入）
    pub incident_root: std::path::PathBuf,
}

/// 检查 Host 标头是否属于安全白名单（本地基础名单 + 用户自定义名单，防御 DNS Rebinding）
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

    // 遵循 RFC 9110 §7.2: Host 标头大小写无关 (Case-Insensitive)；0.0.0.0 与本地回环放行
    if host_only == "0.0.0.0" || is_loopback_host(host_only) {
        return true;
    }

    custom_allowed
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(host_only))
}

/// 防御 DNS Rebinding 的 Host 校验守卫中间件（RFC 9112 Default-Deny）：
/// 缺失或非法编码的 Host 标头一律 400，白名单外 Host 一律 403
async fn host_guard_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> std::result::Result<Response, (StatusCode, &'static str)> {
    // 1. RFC 9112 契约：HTTP/1.1 请求必须携带 Host 标头，缺失即拒绝（杜绝 HTTP/1.0 绕过）
    let Some(host_val) = req.headers().get(HOST) else {
        return Err((StatusCode::BAD_REQUEST, "Missing Host header"));
    };

    // 2. 非法编码的 Host 标头同样拒绝
    let Ok(host_str) = host_val.to_str() else {
        return Err((StatusCode::BAD_REQUEST, "Malformed Host header"));
    };

    // 3. 白名单校验（本地基础名单 + 用户 --allowed-hosts 配置联动）
    if !is_allowed_host(host_str, &state.allowed_hosts) {
        return Err((StatusCode::FORBIDDEN, "Host not allowed"));
    }

    Ok(next.run(req).await)
}

/// 构造统一的 Web Router
pub fn create_web_router(config: WebServerConfig) -> Router {
    let cors = config.dev.then(tower_http::cors::CorsLayer::permissive);
    let state = AppState {
        store: config.store,
        assets: config.assets,
        readonly: config.readonly,
        default_env: config.default_env,
        allowed_hosts: config.allowed_hosts,
        sessions: Arc::new(crate::web::session::WebSessionStore::default()),
        incident_root: config.working_dir.join(".rupost").join("incidents"),
    };

    let api_router = Router::new()
        .route("/files", get(handlers::files::list_files_handler))
        .route("/file", get(handlers::files::get_file_handler))
        .route("/meta", get(handlers::files::meta_handler))
        .route("/save", post(handlers::save::save_file_handler))
        .route("/run/block", post(handlers::run::run_block_handler))
        .route("/run/suite", post(handlers::run::run_suite_handler))
        .route(
            "/mock/simulate",
            post(handlers::mock::simulate_mock_handler),
        )
        .route("/import", post(handlers::import::import_handler))
        .route("/import/curl", post(handlers::import::import_curl_handler));

    let router = Router::new()
        .nest("/api/v1", api_router)
        .route("/", get(serve_index))
        .route("/{*path}", get(serve_asset))
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            host_guard_middleware,
        ))
        .with_state(state);

    // --dev CORS：挂在最外层（Host 守卫与路由之后），OPTIONS 预检由 CorsLayer 直接应答
    if let Some(cors) = cors {
        router.layer(cors)
    } else {
        router
    }
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
    // --dev 安全姿态：宽松 CORS 仅限本地回环绑定，非回环 --host 直接拒绝（fail-fast）
    if config.dev && !is_loopback_host(host) {
        return Err(RupostError::Other(format!(
            "--dev 仅允许绑定本地回环地址（127.0.0.1/localhost/::1），当前 host: {host}。如需局域网访问请去掉 --dev 并改用 --allowed-hosts 白名单。"
        )));
    }
    if config.dev {
        // 校验通过后才打印横幅（避免非回环拒绝时"先宣称已启用后报错"的误导）
        eprintln!(
            "[!] --dev：宽松 CORS 已启用——浏览器中任意网页可跨域读写本地工作台文档，仅限临时本地调试，勿在联网环境长时间运行"
        );
    }
    let app = create_web_router(config);
    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("\n  🚀 rupost 活文档工作台启动成功");
    println!("  ➜  地址: http://{}", addr);
    println!("  ➜  按 Ctrl+C 停止服务\n");

    axum::serve(listener, app).await?;
    Ok(())
}
