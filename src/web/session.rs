//! Web 会话隔离适配层（差异化执行计划 WP-1，P0 修复）。
//!
//! 每个浏览器会话独立持有 Cookie 桥与捕获变量——修复此前 `AppState` 全局
//! `cookie_bridge` / `run_variables` 的跨客户端泄漏（A1 验收）。
//!
//! 边界契约：
//! - Session ID 由服务端 `uuid` 生成，绝不接受请求体传入身份；
//! - Cookie 名固定 `rupost_session`，属性 `Path=/api/v1; HttpOnly; SameSite=Strict;
//!   Max-Age=1800`（host-only，不指定 Domain；本地 HTTP 模式不加 `Secure`）；
//! - 存储仅进程内，TTL 30 分钟（`Instant` 计算，免疫系统时间跳变），上限 128 会话，
//!   每次 load 先清过期再按最久未访问淘汰；
//! - 无 Cookie / 未知 ID / 非法格式 / 过期一律等价新会话，不向客户端泄露内部原因；
//! - 绝不跨 `.await` 持锁：load 返回克隆快照供执行，执行完成后仅短临界区合并增量。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 会话 Cookie 名（本批次唯一新增 transport 契约）
pub const SESSION_COOKIE_NAME: &str = "rupost_session";
/// 会话 TTL：30 分钟（与 Cookie Max-Age 一致）
const SESSION_TTL: Duration = Duration::from_secs(30 * 60);
/// Cookie 属性串：host-only（无 Domain），本地 HTTP 不加 Secure
pub const SESSION_COOKIE_ATTRIBUTES: &str = "Path=/api/v1; HttpOnly; SameSite=Strict; Max-Age=1800";
/// 活跃会话上限（防内存放大），超出按最久未访问淘汰
const MAX_SESSIONS: usize = 128;

/// 单个浏览器会话的执行态资产（服务端存储：浏览器绝不接触上游认证 Cookie
/// 或捕获 secret；不落盘、不进历史/报告/日志）
#[derive(Debug)]
pub struct WebSession {
    pub cookie_state: Option<serde_json::Value>,
    pub captured_variables: HashMap<String, String>,
    last_access: Instant,
}

impl Default for WebSession {
    fn default() -> Self {
        Self {
            cookie_state: None,
            captured_variables: HashMap::new(),
            last_access: Instant::now(),
        }
    }
}

/// load 返回的执行期快照（克隆数据，调用方持于 `.await` 期间，不持锁）
#[derive(Debug, Default)]
pub struct WebSessionSnapshot {
    pub cookie_state: Option<serde_json::Value>,
    pub captured_variables: HashMap<String, String>,
}

/// `load_or_create` 的结果：会话身份 + 执行期快照 + 是否首次创建（需下发 Set-Cookie）
pub struct SessionHandle {
    pub id: String,
    pub snapshot: WebSessionSnapshot,
    pub created: bool,
}

/// 进程内会话存储（`AppState` 唯一会话字段）
#[derive(Default)]
pub struct WebSessionStore {
    sessions: Mutex<HashMap<String, WebSession>>,
}

/// 从 Cookie 请求头解析 `rupost_session` 值：严格匹配 `名=值` 属性，
/// 其他 cookie 忽略；值非法/为空等价于无会话
fn parse_session_id(cookie_header: &str) -> Option<String> {
    cookie_header.split(';').map(str::trim).find_map(|attr| {
        attr.strip_prefix(SESSION_COOKIE_NAME)
            .and_then(|rest| rest.strip_prefix('='))
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    })
}

/// 首次创建会话时下发的 `Set-Cookie` 值
pub fn session_set_cookie_value(session_id: &str) -> String {
    format!("{SESSION_COOKIE_NAME}={session_id}; {SESSION_COOKIE_ATTRIBUTES}")
}

impl WebSessionStore {
    /// 从请求 Cookie 头装载或创建会话：无 Cookie / 非法 / 未知 ID / 过期一律
    /// 等价新会话（`created = true`，调用方需附 `Set-Cookie`）
    pub fn load_or_create(&self, cookie_header: Option<&str>) -> SessionHandle {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        Self::prune_expired(&mut sessions);
        let existing = cookie_header
            .and_then(parse_session_id)
            .and_then(|id| sessions.get_mut(&id).map(|sess| (id, sess)));
        if let Some((id, sess)) = existing {
            sess.last_access = Instant::now();
            return SessionHandle {
                id,
                snapshot: WebSessionSnapshot {
                    cookie_state: sess.cookie_state.clone(),
                    captured_variables: sess.captured_variables.clone(),
                },
                created: false,
            };
        }
        // 新会话：先按最久未访问淘汰至容量内，再插入
        Self::evict_lru(&mut sessions);
        let id = uuid::Uuid::new_v4().to_string();
        sessions.insert(
            id.clone(),
            WebSession {
                last_access: Instant::now(),
                ..WebSession::default()
            },
        );
        SessionHandle {
            id,
            snapshot: WebSessionSnapshot::default(),
            created: true,
        }
    }

    /// 执行完成后合并增量（短临界区，绝不跨 `.await` 持锁）：
    /// `cookie_state` 为 Some 时整体替换导出态；变量增量按键合并。
    /// 会话若在执行期间过期/被淘汰，增量静默丢弃（下次请求等价新会话）。
    pub fn persist_execution(
        &self,
        session_id: &str,
        cookie_state: Option<serde_json::Value>,
        variable_deltas: impl IntoIterator<Item = (String, String)>,
    ) {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(sess) = sessions.get_mut(session_id) {
            if cookie_state.is_some() {
                sess.cookie_state = cookie_state;
            }
            for (k, v) in variable_deltas {
                sess.captured_variables.insert(k, v);
            }
            sess.last_access = Instant::now();
        }
    }

    /// 清理过期会话（每次 load/create 前执行）
    fn prune_expired(sessions: &mut HashMap<String, WebSession>) {
        let now = Instant::now();
        sessions.retain(|_, s| now.duration_since(s.last_access) < SESSION_TTL);
    }

    /// 按最久未访问淘汰，直至容量上限内
    fn evict_lru(sessions: &mut HashMap<String, WebSession>) {
        while sessions.len() >= MAX_SESSIONS {
            let oldest = sessions
                .iter()
                .min_by_key(|(_, s)| s.last_access)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(id) => {
                    sessions.remove(&id);
                }
                None => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// 表驱动锁定 Cookie 头解析契约：正常值、多 cookie 共存、前后缀干扰、
    /// 空值、缺名、空串（跨客户端边界解析必须零歧义）
    #[test]
    fn test_parse_session_id_table() {
        let cases = [
            ("rupost_session=abc-123", Some("abc-123")),
            ("other=1; rupost_session=abc-123", Some("abc-123")),
            ("rupost_session=abc-123; other=2", Some("abc-123")),
            ("  rupost_session=abc-123  ", Some("abc-123")),
            ("rupost_session=", None),
            ("rupost_session =abc", None),
            ("session_rupost=abc", None),
            ("other=1", None),
            ("", None),
        ];
        for (input, expected) in cases {
            assert_eq!(
                parse_session_id(input),
                expected.map(str::to_string),
                "parse {input:?}"
            );
        }
    }

    /// Set-Cookie 值固定契约：名、Path、HttpOnly、SameSite=Strict、Max-Age=1800，
    /// 本地 HTTP 不含 Secure、不指定 Domain
    #[test]
    fn test_session_set_cookie_value_format() {
        let v = session_set_cookie_value("s1");
        assert!(v.starts_with("rupost_session=s1; "), "{v}");
        assert!(v.contains("Path=/api/v1"), "{v}");
        assert!(v.contains("HttpOnly"), "{v}");
        assert!(v.contains("SameSite=Strict"), "{v}");
        assert!(v.contains("Max-Age=1800"), "{v}");
        assert!(
            !v.to_ascii_lowercase().contains("secure"),
            "local HTTP must not set Secure: {v}"
        );
        assert!(
            !v.contains("Domain="),
            "host-only cookie must not set Domain: {v}"
        );
    }

    /// 无 Cookie / 未知 ID 一律等价新会话（created=true，快照为空）
    #[test]
    fn test_load_or_create_without_or_unknown_cookie_is_new_session() {
        let store = WebSessionStore::default();
        for header in [None, Some("rupost_session=ghost"), Some("garbage")] {
            let h = store.load_or_create(header);
            assert!(h.created, "{header:?} must be treated as a new session");
            assert!(h.snapshot.cookie_state.is_none());
            assert!(h.snapshot.captured_variables.is_empty());
        }
    }

    /// 往返契约：created 会话经 persist 合并增量后，同 ID 再 load 必须拿到
    /// 全量数据且 created=false；persist 空导出态不清空既有 cookie_state
    #[test]
    fn test_load_persist_roundtrip_and_null_export_keeps_cookie_state() {
        let store = WebSessionStore::default();
        let first = store.load_or_create(None);
        assert!(first.created);

        store.persist_execution(
            &first.id,
            Some(serde_json::json!({"sid": "upstream"})),
            [("auth_token".to_string(), "tok-a".to_string())],
        );

        let second = store.load_or_create(Some(&format!("{}={}", SESSION_COOKIE_NAME, first.id)));
        assert!(!second.created);
        assert_eq!(
            second.snapshot.cookie_state,
            Some(serde_json::json!({"sid": "upstream"}))
        );
        assert_eq!(
            second
                .snapshot
                .captured_variables
                .get("auth_token")
                .map(String::as_str),
            Some("tok-a")
        );

        // executor 无 cookie 可导出（Ok(None)/Err）时不得清空既有桥态
        store.persist_execution(&first.id, None, []);
        let third = store.load_or_create(Some(&format!("{}={}", SESSION_COOKIE_NAME, first.id)));
        assert_eq!(
            third.snapshot.cookie_state,
            Some(serde_json::json!({"sid": "upstream"}))
        );
    }

    /// 变量增量按键合并（新键插入、旧键覆盖），不丢同会话其他键
    #[test]
    fn test_persist_merges_variable_deltas() {
        let store = WebSessionStore::default();
        let h = store.load_or_create(None);
        store.persist_execution(&h.id, None, [("a".to_string(), "1".to_string())]);
        store.persist_execution(
            &h.id,
            None,
            [
                ("a".to_string(), "2".to_string()),
                ("b".to_string(), "3".to_string()),
            ],
        );
        let again = store.load_or_create(Some(&format!("{}={}", SESSION_COOKIE_NAME, h.id)));
        let vars = again.snapshot.captured_variables;
        assert_eq!(vars.get("a").map(String::as_str), Some("2"));
        assert_eq!(vars.get("b").map(String::as_str), Some("3"));
    }

    /// TTL：过期会话在下次 load 时被清理，同 ID 再来等价新会话
    #[test]
    fn test_expired_session_pruned_on_load() {
        let store = WebSessionStore::default();
        let h = store.load_or_create(None);
        {
            let mut sessions = store.sessions.lock().unwrap_or_else(|e| e.into_inner());
            let sess = sessions.get_mut(&h.id).unwrap();
            sess.last_access = Instant::now() - SESSION_TTL - Duration::from_secs(1);
        }
        let again = store.load_or_create(Some(&format!("{}={}", SESSION_COOKIE_NAME, h.id)));
        assert!(again.created, "expired session must be treated as new");
    }

    /// 容量上限：超出 128 时按最久未访问淘汰，且新会话仍可创建
    #[test]
    fn test_lru_eviction_at_capacity() {
        let store = WebSessionStore::default();
        let first = store.load_or_create(None);
        // 注入至容量上限，并把 first 的 last_access 回拨为最旧
        for i in 0..MAX_SESSIONS {
            let h = store.load_or_create(None);
            let mut sessions = store.sessions.lock().unwrap_or_else(|e| e.into_inner());
            let sess = sessions.get_mut(&h.id).unwrap();
            sess.last_access = Instant::now() - Duration::from_secs(3_600 - i as u64);
        }
        {
            let mut sessions = store.sessions.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(sess) = sessions.get_mut(&first.id) {
                sess.last_access = Instant::now() - Duration::from_secs(7_200);
            }
        }
        let overflow = store.load_or_create(None);
        let sessions = store.sessions.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            sessions.len() <= MAX_SESSIONS,
            "session count must stay within cap"
        );
        assert!(
            !sessions.contains_key(&first.id),
            "least-recently-used session must be evicted"
        );
        assert!(sessions.contains_key(&overflow.id));
    }
}
