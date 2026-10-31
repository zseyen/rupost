# Rupost 历史记录存储优化与配置设计方案

本文件详细记录了针对 `rupost` 的请求/响应历史记录模块（`src/history/`）的存储优化与全局配置接入的技术方案。

本设计的目标是：
1. **可配置化**：通过 `rupost.toml` 允许用户定制历史记录的开启状态、目录位置、文件大小上限、最大保留条目数、是否存盘响应体（Response Body）以及响应体大小阈值。
2. **Response Body 存盘与大小限额**：在保障磁盘不膨胀的原则下，支持将特定大小以内的 Response Body 存入历史记录，以服务后续的 `replay`（重放）与 TUI 回溯。
3. **完美向前兼容**：兼容用户本地已有的无 Body 的旧历史记录，不触发反序列化崩溃。

---

## 🛠️ 技术方案与细节规划

### 1. 结构变更与全局配置设计

#### 1.1 `src/variable/types.rs` [MODIFY]
我们需要在全局配置文件加载实体 `VariableConfig` 中增加 `history` 全局配置段，并实现 `HistoryConfig` 及其各字段的 Serde 默认构造逻辑。

**需要新增的代码细节：**

```rust
/// 历史记录全局配置
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct HistoryConfig {
    /// 是否启用历史记录记录，默认为 true
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// 历史记录保存的目录，默认是 ".rupost"
    #[serde(default = "default_history_dir")]
    pub dir: String,

    /// 触发压缩的文件大小上限（字节），默认 10MB
    #[serde(default = "default_max_file_size")]
    pub max_file_size: u64,

    /// 压缩后保留的最大条目数，默认 1000 条
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,

    /// 是否记录响应体 (Response Body)，默认 false
    #[serde(default = "default_false")]
    pub record_response_body: bool,

    /// 记录响应体的最大大小（字节），默认 50KB (51200 字节)
    #[serde(default = "default_max_response_body_size")]
    pub max_response_body_size: usize,
}

fn default_true() -> bool { true }
fn default_false() -> bool { false }
fn default_history_dir() -> String { ".rupost".to_string() }
fn default_max_file_size() -> u64 { 10 * 1024 * 1024 } // 10MB
fn default_max_entries() -> usize { 1000 }
fn default_max_response_body_size() -> usize { 50 * 1024 } // 50KB

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            dir: default_history_dir(),
            max_file_size: default_max_file_size(),
            max_entries: default_max_entries(),
            record_response_body: default_false(),
            max_response_body_size: default_max_response_body_size(),
        }
    }
}
```

并将 `VariableConfig` 修改为：

```rust
/// 完整的变量配置文件
#[derive(Debug, Clone, Deserialize, Default)]
pub struct VariableConfig {
    /// 所有环境配置
    #[serde(default)]
    pub environments: HashMap<String, Environment>,

    /// 历史记录全局配置
    #[serde(default)]
    pub history: HistoryConfig,
}
```

---

### 2. 历史数据模型调整与向前兼容

#### 2.1 `src/history/model.rs` [MODIFY]
我们需要修改 `ResponseMeta` 的定义，将 `body` 作为可选字段加入。同时，必须为 `body` 挂载 `#[serde(default)]` 属性，以确保反序列化旧版本 JSONL 历史文件时，缺失的 `body` 字段会被自动填充为 `None`，不抛出异常。

**改动对比：**

```diff
 /// 响应元数据 (不包含 Body，节省空间)
 #[derive(Debug, Clone, Serialize, Deserialize)]
 pub struct ResponseMeta {
     pub status: u16,
 
     #[serde(with = "serialization::header_map")]
     pub headers: HeaderMap,
+
+    #[serde(default)]
+    pub body: Option<String>,
 }
```

---

### 3. 持久化存储 (`HistoryStorage`) 改造

#### 3.1 `src/history/storage.rs` [MODIFY]
目前 `HistoryStorage` 内部的数据上限参数为写死的常量。我们需要将其重构为持有 `HistoryConfig` 的动态载入模式：

1. **结构体定义变更**：
   ```rust
   pub struct HistoryStorage {
       file_path: PathBuf,
       pub config: crate::variable::types::HistoryConfig,
   }
   ```
2. **构造函数变更 (`Default`)**：
   当构造 `HistoryStorage` 时，主动通过 `ConfigLoader::find_and_load()` 读取 `rupost.toml` 中的 `history` 字段。如加载失败则回退至 `HistoryConfig::default()`：
   ```rust
   impl Default for HistoryStorage {
       fn default() -> Self {
           let config = crate::variable::ConfigLoader::find_and_load()
               .map(|c| c.history)
               .unwrap_or_default();
           let dir = std::env::var("RUPOST_HISTORY_DIR").unwrap_or_else(|_| config.dir.clone());
           let path = Path::new(&dir).join(HISTORY_FILE);
           Self { file_path: path, config }
       }
   }
   ```
3. **禁用机制落地 (`append`)**：
   在 `append` 顶部加入前置判定，当用户在配置中声明 `enabled = false` 时直接截断返回，不做任何磁盘 IO 写动作：
   ```rust
   pub fn append(&self, entry: &HistoryEntry) -> Result<()> {
       if !self.config.enabled {
           return Ok(());
       }
       // ...原有文件排他锁与追加写逻辑
   }
   ```
4. **清理阈值动态化 (`compact_if_needed`)**：
   将方法内部的 `COMPACTION_THRESHOLD_BYTES` 替换为 `self.config.max_file_size`；将 `MAX_ENTRIES` 替换为 `self.config.max_entries`：
   ```rust
   let metadata = file.metadata().map_err(RupostError::IoError)?;
   if metadata.len() < self.config.max_file_size {
       return Ok(());
   }
   // ...
   if entries.len() <= self.config.max_entries {
       return Ok(());
   }
   let keep_count = self.config.max_entries;
   ```

---

### 4. 历史记录器大小限制判定

#### 4.1 `src/history/recorder.rs` [MODIFY]
在 `record_history` 函数中，需要根据全局配置的字段决定如何录制 Response Body。

**核心逻辑细节：**

```rust
pub fn record_history(request: RequestSnapshot, response: &Response, source: Option<String>) {
    let storage = get_storage();
    // 1. 若配置中禁用了 history 则直接跳过
    if !storage.config.enabled {
        return;
    }

    // 2. 根据配置与响应体大小，决定是否存盘
    let body = if storage.config.record_response_body {
        if response.body.len() <= storage.config.max_response_body_size {
            Some(response.body.clone())
        } else {
            // 超出限额，不记录响应体，但仍然需要记录元数据
            None
        }
    } else {
        None
    };

    let history_entry = HistoryEntry {
        id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        duration_ms: response.duration.as_millis() as u64,
        request,
        source,
        response: ResponseMeta {
            status: response.status.code(),
            headers: response.headers.clone(),
            body,
        },
    };

    if let Err(e) = storage.append(&history_entry) {
        warn!("Failed to save request history: {}", e);
    }
}
```

---

## 🧪 自动化测试与验证用例设计

接手的 Flash 级别模型在实现完上述改动后，**必须编写/运行以下测试用例**以确保其正确性与向前兼容性：

### 1. 配置加载与反序列化测试
在 `src/variable/types.rs` 的 `mod tests` 中增加 `test_history_config_deserialization`：
```rust
#[test]
fn test_history_config_deserialization() {
    let toml_content = r#"
[history]
enabled = false
dir = ".custom_history"
max_file_size = 5000000
max_entries = 500
record_response_body = true
max_response_body_size = 10240
"#;
    let config: VariableConfig = toml::from_str(toml_content).unwrap();
    assert!(!config.history.enabled);
    assert_eq!(config.history.dir, ".custom_history");
    assert_eq!(config.history.max_file_size, 5_000_000);
    assert_eq!(config.history.max_entries, 500);
    assert!(config.history.record_response_body);
    assert_eq!(config.history.max_response_body_size, 10_240);
}
```

### 2. 向前兼容性测试
在 `src/history/storage.rs` 中编写测试，模拟加载老版本没有 `body` 字段的历史记录 JSONL 字符串，验证反序列化成功：
```rust
#[test]
fn test_backward_compatibility_without_body() {
    // 模拟旧版本只有 status 和 headers，没有 body 的 JSONL 字符串
    let old_json = r#"{"id":"123","timestamp":"2026-06-06T00:00:00Z","duration_ms":50,"request":{"method":"GET","url":"https://example.com","headers":{},"body":null},"source":null,"response":{"status":200,"headers":{}}}"#;
    let entry: std::result::Result<HistoryEntry, serde_json::Error> = serde_json::from_str(old_json);
    assert!(entry.is_ok());
    let entry = entry.unwrap();
    assert_eq!(entry.response.status, 200);
    assert!(entry.response.body.is_none()); // 自动填充为 None
}
```

### 3. 大小限额与关闭记录集成验证
在 `src/history/storage.rs` 的测试模块中编写集成测试：
* **用例一**：设置 `record_response_body = true` 和 `max_response_body_size = 10`。发送一个长度为 5 字节的 Body 响应，验证记录中包含 Body；发送一个长度为 15 字节 of Body 响应，验证记录成功但 `body` 字段为 `None`。
* **用例二**：设置 `enabled = false`，调用 `storage.append()` 写入一条历史数据，验证磁盘上没有创建文件或文件长度未发生增长。
