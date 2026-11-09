use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;

pub mod header_map {
    use super::*;

    pub fn serialize<S>(headers: &HeaderMap, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = HashMap::new();
        for (k, v) in headers.iter() {
            // Convert header name to string
            let key = k.as_str().to_string();
            // lossy 读回：UTF-8（含 obs-text 非 ASCII）无损，真损坏字节显式替换——
            // 旧 to_str().unwrap_or("") 使非 ASCII 值静默变空串（审查 P1-3 第四层）
            let value = String::from_utf8_lossy(v.as_bytes()).into_owned();
            map.insert(key, value);
        }
        map.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HeaderMap, D::Error>
    where
        D: Deserializer<'de>,
    {
        let map: HashMap<String, String> = HashMap::deserialize(deserializer)?;
        let mut headers = HeaderMap::new();
        for (k, v) in map {
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(&v),
            ) {
                headers.insert(name, value);
            }
        }
        Ok(headers)
    }
}

#[cfg(test)]
mod tests {
    use reqwest::header::{HeaderMap, HeaderValue};
    /// 非 ASCII header 序列化往返（审查 P1-3 第四层）：真实流量落盘路径
    /// （history record → JSON → 回读）中文值必须无损
    #[test]
    fn test_non_ascii_header_serialization_round_trip() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct W {
            #[serde(with = "super::header_map")]
            headers: HeaderMap,
        }
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-note",
            HeaderValue::from_bytes("中文备注".as_bytes()).unwrap(),
        );
        let w = W { headers };

        let json = serde_json::to_string(&w).unwrap();
        assert!(json.contains("中文备注"), "serialize must preserve: {json}");

        let back: W = serde_json::from_str(&json).unwrap();
        assert_eq!(
            String::from_utf8_lossy(back.headers.get("x-note").unwrap().as_bytes()),
            "中文备注"
        );
    }

    /// 损坏字节分支锁定（审查 P2①）：0xFF（合法 obs-text 但非 UTF-8）序列化为
    /// U+FFFD（数据改写、非静默丢失），回读 from_str 对 U+FFFD 成功——不丢头
    #[test]
    fn test_damaged_bytes_serialize_to_replacement_char() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct W {
            #[serde(with = "super::header_map")]
            headers: HeaderMap,
        }
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-bin",
            HeaderValue::from_bytes(&[0x48, 0xFF, 0x49]).unwrap(),
        );
        let w = W { headers };

        let json = serde_json::to_string(&w).unwrap();
        assert!(
            json.contains("\u{fffd}") || json.contains("�"),
            "damaged bytes must serialize visibly as U+FFFD: {json}"
        );

        let back: W = serde_json::from_str(&json).unwrap();
        assert!(
            back.headers.contains_key("x-bin"),
            "header must survive round trip (no silent drop)"
        );
    }
}
