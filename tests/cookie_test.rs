//! Integration tests for cookie support.

use rupost::http::Client;
use rupost::http::request::Request;
use rupost::middleware::CookieMiddleware;
use std::sync::Arc;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_cookie_sending() {
    // Start mock server
    let mock_server = MockServer::start().await;

    // Set up mock that requires a cookie
    Mock::given(method("GET"))
        .and(path("/protected"))
        .and(header("cookie", "session=abc123"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Welcome!"))
        .mount(&mock_server)
        .await;

    // Create client with ephemeral cookies (simulating a previous Set-Cookie)
    let middleware = CookieMiddleware::new_ephemeral();
    let cookie_store = middleware.cookie_store();

    // Manually insert a cookie into the store
    {
        let mut store = cookie_store.lock().unwrap();
        let url = reqwest::Url::parse(&mock_server.uri()).unwrap();
        let cookie = cookie::Cookie::new("session", "abc123");
        store.insert_raw(&cookie, &url).unwrap();
    }

    let client = Client::with_cookie_store(cookie_store);

    // Make request
    let request = Request::new("GET", &format!("{}/protected", mock_server.uri())).unwrap();
    let response = client.execute(request).await.unwrap();

    assert_eq!(response.status.code(), 200);
    assert_eq!(response.body, "Welcome!");
}

#[tokio::test]
async fn test_set_cookie_receiving() {
    // Start mock server
    let mock_server = MockServer::start().await;

    // Set up mock that sets a cookie
    Mock::given(method("GET"))
        .and(path("/login"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", "token=xyz789; Path=/")
                .set_body_string("Logged in"),
        )
        .mount(&mock_server)
        .await;

    // Create client with ephemeral cookies
    let middleware = CookieMiddleware::new_ephemeral();
    let cookie_store = middleware.cookie_store();
    let client = Client::with_cookie_store(Arc::clone(&cookie_store));

    // Make login request
    let request = Request::new("GET", &format!("{}/login", mock_server.uri())).unwrap();
    let response = client.execute(request).await.unwrap();

    assert_eq!(response.status.code(), 200);

    // Verify cookie was stored
    {
        let store = cookie_store.lock().unwrap();
        let url = reqwest::Url::parse(&mock_server.uri()).unwrap();
        let cookies: Vec<_> = store.get_request_values(&url).collect();
        assert!(
            cookies
                .iter()
                .any(|(name, val)| *name == "token" && *val == "xyz789")
        );
    }
}

#[tokio::test]
async fn test_cookie_persistence() {
    use std::fs;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let cookie_file = dir.path().join("cookies.json");

    // Start mock server
    let mock_server = MockServer::start().await;
    let server_uri = mock_server.uri();

    // Set up mock that sets a persistent cookie (must have Max-Age or Expires to be serialized)
    Mock::given(method("GET"))
        .and(path("/set"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", "persist=value123; Path=/; Max-Age=86400")
                .set_body_string("Cookie set"),
        )
        .mount(&mock_server)
        .await;

    // First run: receive and persist cookie
    {
        let middleware = CookieMiddleware::new_with_persistence(cookie_file.clone()).unwrap();
        let cookie_store = middleware.cookie_store();
        let client = Client::with_cookie_store(cookie_store);

        let request = Request::new("GET", &format!("{}/set", server_uri)).unwrap();
        let response = client.execute(request).await.unwrap();
        assert_eq!(response.status.code(), 200);

        // Trigger persistence by calling after_response
        use rupost::middleware::Middleware;
        middleware.after_response(&response).await.unwrap();
    }

    // Verify file was created and contains the cookie
    assert!(
        cookie_file.exists(),
        "Cookie file should exist after persistence"
    );

    let content = fs::read_to_string(&cookie_file).unwrap();
    assert!(
        content.contains("persist") && content.contains("value123"),
        "Cookie file should contain the persisted cookie, got: {}",
        content
    );

    // Second run: verify cookie can be loaded
    {
        let middleware = CookieMiddleware::new_with_persistence(cookie_file.clone()).unwrap();
        let cookie_store = middleware.cookie_store();

        // Just verify the store is not empty (cookie was loaded)
        let store = cookie_store.lock().unwrap();
        let all_cookies: Vec<_> = store.iter_any().collect();
        assert!(
            !all_cookies.is_empty(),
            "Cookie store should have loaded cookies from file"
        );
    }
}
