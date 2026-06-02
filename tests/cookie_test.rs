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

#[tokio::test]
async fn test_cookie_domain_and_path_matching() {
    let mock_server = MockServer::start().await;

    // Remove exact cookie header matchers from wiremock, just match path and method
    Mock::given(method("GET"))
        .and(path("/sub/page"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Submatched"))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/other"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Othermatched"))
        .mount(&mock_server)
        .await;

    let middleware = CookieMiddleware::new_ephemeral();
    let cookie_store = middleware.cookie_store();

    // Inject cookies
    {
        let mut store = cookie_store.lock().unwrap();
        let url = reqwest::Url::parse(&mock_server.uri()).unwrap();
        
        // Cookie for the domain
        let cookie_domain = cookie::Cookie::build(("domain_val", "localhost"))
            .domain(url.host_str().unwrap().to_string())
            .path("/")
            .build();
        store.insert_raw(&cookie_domain, &url).unwrap();

        // Cookie restricted to path "/sub"
        let cookie_path = cookie::Cookie::build(("path_val", "sub"))
            .domain(url.host_str().unwrap().to_string())
            .path("/sub")
            .build();
        store.insert_raw(&cookie_path, &url).unwrap();
    }

    let client = Client::with_cookie_store(cookie_store);

    // 1. Request to /other
    let request1 = Request::new("GET", &format!("{}/other", mock_server.uri())).unwrap();
    let resp1 = client.execute(request1).await.unwrap();
    assert_eq!(resp1.status.code(), 200);

    // 2. Request to /sub/page
    let request2 = Request::new("GET", &format!("{}/sub/page", mock_server.uri())).unwrap();
    let resp2 = client.execute(request2).await.unwrap();
    assert_eq!(resp2.status.code(), 200);

    // Inspect actual headers sent to server
    let received = mock_server.received_requests().await.expect("No received requests found");
    assert_eq!(received.len(), 2);

    // Request 1: to /other. Should have domain_val=localhost, but NOT path_val=sub
    let req1_headers = &received[0].headers;
    let cookie_header1 = req1_headers.get("cookie").expect("Request to /other should contain Cookie header").to_str().unwrap();
    assert!(cookie_header1.contains("domain_val=localhost"));
    assert!(!cookie_header1.contains("path_val=sub"));

    // Request 2: to /sub/page. Should contain BOTH cookies
    let req2_headers = &received[1].headers;
    let cookie_header2 = req2_headers.get("cookie").expect("Request to /sub/page should contain Cookie header").to_str().unwrap();
    assert!(cookie_header2.contains("domain_val=localhost"));
    assert!(cookie_header2.contains("path_val=sub"));
}

#[tokio::test]
async fn test_cookie_expiration() {
    let mock_server = MockServer::start().await;

    // Mock server should only receive the active cookie
    Mock::given(method("GET"))
        .and(path("/test"))
        .and(header("cookie", "active=1"))
        .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
        .mount(&mock_server)
        .await;

    let middleware = CookieMiddleware::new_ephemeral();
    let cookie_store = middleware.cookie_store();

    {
        let mut store = cookie_store.lock().unwrap();
        let url = reqwest::Url::parse(&mock_server.uri()).unwrap();

        // Active cookie (valid for 1 hour)
        let cookie_active = cookie::Cookie::build(("active", "1"))
            .domain(url.host_str().unwrap().to_string())
            .path("/")
            .max_age(cookie::time::Duration::hours(1))
            .build();
        store.insert_raw(&cookie_active, &url).unwrap();

        // Cookie with extremely short duration (valid for 1 second)
        let cookie_expired = cookie::Cookie::build(("expired", "2"))
            .domain(url.host_str().unwrap().to_string())
            .path("/")
            .max_age(cookie::time::Duration::seconds(1))
            .build();
        store.insert_raw(&cookie_expired, &url).unwrap();
    }

    // Wait for the cookie to expire
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    let client = Client::with_cookie_store(cookie_store);
    let request = Request::new("GET", &format!("{}/test", mock_server.uri())).unwrap();
    let response = client.execute(request).await.unwrap();

    assert_eq!(response.status.code(), 200);
    assert_eq!(response.body, "OK");
}

#[tokio::test]
async fn test_session_and_persistent_cookies() {
    use tempfile::tempdir;
    use rupost::middleware::Middleware;

    let dir = tempdir().unwrap();
    let cookie_file = dir.path().join("cookies.json");

    // 1. Create cookies (one session, one persistent) and save
    {
        let middleware = CookieMiddleware::new_with_persistence(cookie_file.clone()).unwrap();
        let cookie_store = middleware.cookie_store();

        {
            let mut store = cookie_store.lock().unwrap();
            let url = reqwest::Url::parse("https://example.com").unwrap();

            let session_cookie = cookie::Cookie::build(("session_cookie", "abc"))
                .domain("example.com")
                .path("/")
                .build();
            store.insert_raw(&session_cookie, &url).unwrap();

            let persistent_cookie = cookie::Cookie::build(("persistent_cookie", "123"))
                .domain("example.com")
                .path("/")
                .max_age(cookie::time::Duration::days(1))
                .build();
            store.insert_raw(&persistent_cookie, &url).unwrap();
        }

        // Trigger persistence via public middleware interface
        let dummy_resp = rupost::http::Response::error("".to_string());
        middleware.after_response(&dummy_resp).await.unwrap();
    }

    // 2. Load again and verify both cookies exist
    {
        let middleware = CookieMiddleware::new_with_persistence(cookie_file).unwrap();
        let cookie_store = middleware.cookie_store();
        let store = cookie_store.lock().unwrap();
        
        let cookies: Vec<_> = store.iter_any().collect();
        assert_eq!(cookies.len(), 2);

        let has_session = cookies.iter().any(|c| c.name() == "session_cookie" && c.value() == "abc");
        let has_persistent = cookies.iter().any(|c| c.name() == "persistent_cookie" && c.value() == "123");
        
        assert!(has_session, "Session cookie should be loaded");
        assert!(has_persistent, "Persistent cookie should be loaded");
    }
}

#[test]
fn test_cookie_file_corruption_recovery() {
    use std::fs;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let cookie_file = dir.path().join("cookies.json");

    // Write corrupt/invalid JSON content to cookie file
    fs::write(&cookie_file, "{invalid json: true").unwrap();

    // Load cookies.
    // Under our robust implementation, this should not fail, but fallback to empty store.
    // Let's assert it succeeds. (Note: Currently this will fail before implementation).
    let middleware_res = CookieMiddleware::new_with_persistence(cookie_file);
    
    // Assert we successfully recover and fallback
    assert!(middleware_res.is_ok(), "Should fallback to empty store on corruption");
    let middleware = middleware_res.unwrap();
    assert_eq!(middleware.cookie_store().lock().unwrap().iter_any().count(), 0);
}

#[tokio::test]
async fn test_concurrent_cookie_access() {
    use tempfile::tempdir;
    use std::sync::Arc;
    use rupost::middleware::Middleware;

    let dir = tempdir().unwrap();
    let cookie_file = Arc::new(dir.path().join("cookies.json"));

    let mut handles = vec![];

    for i in 0..10 {
        let file_path = Arc::clone(&cookie_file);
        let handle = tokio::spawn(async move {
            // Mimic workflow: load, insert cookie, save
            let middleware = CookieMiddleware::new_with_persistence((*file_path).clone()).unwrap();
            {
                let cookie_store = middleware.cookie_store();
                let mut store = cookie_store.lock().unwrap();
                let url = reqwest::Url::parse("https://example.com").unwrap();
                let cookie = cookie::Cookie::build((format!("cookie_{}", i), i.to_string()))
                    .domain("example.com")
                    .path("/")
                    .max_age(cookie::time::Duration::days(1))
                    .build();
                store.insert_raw(&cookie, &url).unwrap();
            }
            let dummy_resp = rupost::http::Response::error("".to_string());
            middleware.after_response(&dummy_resp).await.unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Final verification that the file is not corrupted and contains cookies
    let middleware = CookieMiddleware::new_with_persistence((*cookie_file).clone()).unwrap();
    let store = middleware.cookie_store();
    let count = store.lock().unwrap().iter_any().count();
    assert!(count > 0, "Cookie store should have at least one successfully written cookie");
}

#[tokio::test]
async fn test_cookie_disabled() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/set_cookie"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", "token=abc123; Path=/")
                .set_body_string("Cookie Set"),
        )
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/check_cookie"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Checked"))
        .mount(&mock_server)
        .await;

    // Create client without cookie support
    let client = Client::new();

    // 1. Request to set cookie
    let request1 = Request::new("GET", &format!("{}/set_cookie", mock_server.uri())).unwrap();
    client.execute(request1).await.unwrap();

    // 2. Request to check cookie
    let request2 = Request::new("GET", &format!("{}/check_cookie", mock_server.uri())).unwrap();
    client.execute(request2).await.unwrap();

    // Verify through received requests
    let received = mock_server.received_requests().await;
    let received_vec = received.expect("No received requests found");
    assert_eq!(received_vec.len(), 2);
    
    // The second request should NOT have "cookie" header
    let second_req = &received_vec[1];
    let has_cookie = second_req.headers.keys().any(|k| k.to_string().to_lowercase() == "cookie");
    assert!(!has_cookie, "Should not send cookie when cookies are disabled");
}
