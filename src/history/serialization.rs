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
            // Convert header value to string (lossy if not UTF-8)
            let value = v.to_str().unwrap_or("").to_string();
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
