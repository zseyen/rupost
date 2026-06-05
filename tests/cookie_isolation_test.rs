//! Integration tests for multi-environment cookie isolation.

use rupost::middleware::resolve_cookie_path;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn test_cookie_filename_resolution_by_env() {
    let base = PathBuf::from("/path/to/cookies.json");

    // 1. 无环境名称，保持原样
    let path_no_env = resolve_cookie_path(base.clone(), None);
    assert_eq!(path_no_env, base);

    // 2. dev 环境，拼入环境后缀
    let path_dev = resolve_cookie_path(base.clone(), Some("dev"));
    assert_eq!(path_dev, PathBuf::from("/path/to/cookies_dev.json"));

    // 3. prod 环境，拼入环境后缀
    let path_prod = resolve_cookie_path(base.clone(), Some("prod"));
    assert_eq!(path_prod, PathBuf::from("/path/to/cookies_prod.json"));
}

#[tokio::test]
async fn test_cookie_physical_isolation_on_disk() {
    use rupost::middleware::CookieMiddleware;
    use rupost::middleware::Middleware;

    let dir = tempdir().unwrap();
    let base_cookie_file = dir.path().join("cookies.json");

    // 模拟运行 dev 环境下的请求并保存
    let dev_resolved_path = resolve_cookie_path(base_cookie_file.clone(), Some("dev"));
    let dev_mw = CookieMiddleware::new_with_persistence(dev_resolved_path.clone()).unwrap();
    {
        let store = dev_mw.cookie_store();
        let mut lock = store.lock().unwrap();
        let url = reqwest::Url::parse("https://example.com").unwrap();
        let cookie = cookie::Cookie::build(("session", "dev-session-token"))
            .domain("example.com")
            .path("/")
            .max_age(cookie::time::Duration::hours(1))
            .build();
        lock.insert_raw(&cookie, &url).unwrap();
    }

    // 触发保存
    let dummy_resp = rupost::http::Response::error("".to_string());
    dev_mw.after_response(&dummy_resp).await.unwrap();

    // 模拟运行 prod 环境下的请求并保存
    let prod_resolved_path = resolve_cookie_path(base_cookie_file.clone(), Some("prod"));
    let prod_mw = CookieMiddleware::new_with_persistence(prod_resolved_path.clone()).unwrap();
    {
        let store = prod_mw.cookie_store();
        let mut lock = store.lock().unwrap();
        let url = reqwest::Url::parse("https://example.com").unwrap();
        let cookie = cookie::Cookie::build(("session", "prod-session-token"))
            .domain("example.com")
            .path("/")
            .max_age(cookie::time::Duration::hours(1))
            .build();
        lock.insert_raw(&cookie, &url).unwrap();
    }

    // 触发保存
    prod_mw.after_response(&dummy_resp).await.unwrap();

    // 验证物理盘上产生了两个被隔离的文件
    assert!(dev_resolved_path.exists());
    assert!(prod_resolved_path.exists());
    assert_ne!(dev_resolved_path, prod_resolved_path);

    // 验证 dev 文件里不包含 prod-session-token
    let dev_content = fs::read_to_string(&dev_resolved_path).unwrap();
    assert!(dev_content.contains("dev-session-token"));
    assert!(!dev_content.contains("prod-session-token"));

    // 验证 prod 文件里不包含 dev-session-token
    let prod_content = fs::read_to_string(&prod_resolved_path).unwrap();
    assert!(prod_content.contains("prod-session-token"));
    assert!(!prod_content.contains("dev-session-token"));
}
