//! 执行观察者抽象（将历史记录等副作用从协议执行器解耦）。

use crate::history::model::RequestSnapshot;
use crate::http::Response;

/// 请求执行生命周期观察者。
///
/// 协议执行器（HTTP / WS / SSE）通过此 trait 发出“请求已完成”事件，
/// 由外层（如 `HistoryObserver`）决定是否落盘，避免 runner 硬编码依赖 `history::recorder`。
pub trait ExecutionObserver: Send + Sync {
    fn on_record_history(
        &self,
        request: RequestSnapshot,
        response: &Response,
        source: Option<String>,
    );
}

/// 空实现观察者（默认不产生任何副作用）。
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopExecutionObserver;

impl ExecutionObserver for NoopExecutionObserver {
    fn on_record_history(
        &self,
        _request: RequestSnapshot,
        _response: &Response,
        _source: Option<String>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    type RecordedHistory = Arc<Mutex<Vec<(String, u16, Option<String>)>>>;

    struct RecordingObserver {
        recorded: RecordedHistory,
    }

    impl ExecutionObserver for RecordingObserver {
        fn on_record_history(
            &self,
            request: RequestSnapshot,
            response: &Response,
            source: Option<String>,
        ) {
            self.recorded
                .lock()
                .unwrap()
                .push((request.url, response.status.code(), source));
        }
    }

    #[test]
    fn test_execution_observer_noop_and_custom_recording() {
        let req = RequestSnapshot {
            method: "GET".to_string(),
            url: "http://localhost/ping".to_string(),
            headers: HeaderMap::new(),
            body: None,
            timeout: None,
        };
        let resp = Response::new(
            200,
            HeaderMap::new(),
            "pong".to_string(),
            Duration::from_millis(5),
            Duration::from_millis(3),
            Duration::from_millis(2),
        )
        .unwrap();

        let noop = NoopExecutionObserver;
        noop.on_record_history(req.clone(), &resp, None);

        let recorded = Arc::new(Mutex::new(Vec::new()));
        let obs: Arc<dyn ExecutionObserver> = Arc::new(RecordingObserver {
            recorded: recorded.clone(),
        });
        obs.on_record_history(req, &resp, Some("suite.http".to_string()));

        let items = recorded.lock().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0],
            (
                "http://localhost/ping".to_string(),
                200,
                Some("suite.http".to_string())
            )
        );
    }
}
