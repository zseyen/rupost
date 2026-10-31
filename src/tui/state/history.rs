//! 历史域：滑窗视口加载与请求快照 → .http 反向格式化（纯函数）。

use super::*;

impl AppState {
    pub fn load_viewport_sliding_window(&mut self, scroll_y: usize, height: usize) {
        let file_path = match &self.log_file_path {
            Some(p) => p,
            None => return,
        };

        let buffer_size = 50; // 前后缓冲区扩展行数
        let start = scroll_y.saturating_sub(buffer_size);
        let end = (scroll_y + height + buffer_size).min(self.total_log_lines);

        // 如果当前的缓存已经包含了我们计算出来的范围，并且不为空，则不需要重新读取物理文件
        if !self.viewport_cache.is_empty()
            && start >= self.cache_start_line
            && end <= self.cache_end_line
        {
            return;
        }

        // 重新读取文件指定行
        if let Ok(file) = std::fs::File::open(file_path) {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(file);
            let mut new_cache = std::collections::VecDeque::new();

            for (idx, line) in reader.lines().enumerate() {
                if idx < start {
                    continue;
                }
                if idx >= end {
                    break;
                }
                if let Ok(l) = line {
                    let is_send = l.starts_with("[→]");
                    let is_recv = l.starts_with("[←]");
                    let (timestamp, content) = if (is_send || is_recv) && l.len() >= 15 {
                        let ts = l.chars().skip(4).take(8).collect::<String>();
                        let c = l.chars().skip(15).collect::<String>();
                        (ts, c)
                    } else {
                        (
                            chrono::Local::now().format("%H:%M:%S").to_string(),
                            l.clone(),
                        )
                    };

                    new_cache.push_back(WsFrameRecord {
                        is_send,
                        timestamp,
                        content,
                    });
                }
            }

            self.viewport_cache = new_cache;
            self.cache_start_line = start;
            self.cache_end_line = end;
        }
    }
}

pub fn format_request_snapshot_to_http(req: &crate::history::model::RequestSnapshot) -> String {
    let mut s = String::new();
    s.push_str(&format!("{} {}\n", req.method, req.url));
    for (k, v) in &req.headers {
        if let Ok(val_str) = v.to_str() {
            s.push_str(&format!("{}: {}\n", k.as_str(), val_str));
        }
    }
    s.push('\n');
    if let Some(ref body) = req.body {
        s.push_str(body);
    }
    s
}

/// 提取 URL 中的 Host 和 Path 组合，剥离协议前缀（如 http://, https://）
/// 例如："https://baidu.com/api/v1?q=1" -> "baidu.com/api/v1"
/// 如果解析失败，则做降级的前缀剥离。
pub fn format_url_host_and_path(url_str: &str) -> String {
    if let Ok(u) = url::Url::parse(url_str) {
        let scheme = u.scheme();
        if scheme == "http" || scheme == "https" || scheme == "ws" || scheme == "wss" {
            let host = u.host_str().unwrap_or("");
            let port_part = if let Some(port) = u.port() {
                format!(":{}", port)
            } else {
                "".to_string()
            };
            let path = u.path();
            let combined = format!("{}{}{}", host, port_part, path);
            if !combined.is_empty() {
                return combined;
            }
        }
    }

    let stripped = url_str
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    if let Some(pos) = stripped.find('?') {
        stripped[..pos].to_string()
    } else if let Some(pos) = stripped.find('#') {
        stripped[..pos].to_string()
    } else {
        stripped.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::model::RequestSnapshot;
    use reqwest::header::HeaderMap;

    #[test]
    fn test_format_url_host_and_path() {
        assert_eq!(
            format_url_host_and_path("https://baidu.com/api/v1?q=1"),
            "baidu.com/api/v1"
        );
        assert_eq!(
            format_url_host_and_path("http://localhost:8080/users/1#fragment"),
            "localhost:8080/users/1"
        );
        assert_eq!(format_url_host_and_path("http://google.com"), "google.com/");
        assert_eq!(
            format_url_host_and_path("localhost:3000/test?foo=bar"),
            "localhost:3000/test"
        );
        assert_eq!(format_url_host_and_path("/relative/path"), "/relative/path");
    }

    #[test]
    fn test_history_restore_format() {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert("x-custom", "value".parse().unwrap());

        let req = RequestSnapshot {
            timeout: None,
            method: "POST".to_string(),
            url: "http://example.com/api".to_string(),
            headers,
            body: Some("{\"test\":true}".to_string()),
        };

        let http_text = format_request_snapshot_to_http(&req);
        let parsed = crate::parser::parse_content(&http_text).unwrap();
        let requests = parsed.requests;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method_or_default(), "POST");
        assert_eq!(requests[0].url, "http://example.com/api");
        assert_eq!(requests[0].headers.len(), 2);
        assert_eq!(requests[0].body, Some("{\"test\":true}".to_string()));
    }

    // ---- 快速新建用例：安全与错误浮出（设计文档 Phase 0 特征测试）----
}
