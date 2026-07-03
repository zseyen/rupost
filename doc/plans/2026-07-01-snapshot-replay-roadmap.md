# HTTP 快照录制与原样重放：后续功能路线图 (Roadmap)

本文件详细记录了在完成 MVP 基础闭环后，RuPost 快照录制与原样重放功能后续需要逐步迭代实现的增强功能与技术规划。

---

## 📅 后续待开发功能清单 (Backlog)

### 1. P0：敏感机密信息就地掩码脱敏 (Secrets Masker)
*   **需求背景**：快照包可能会在外部介质传输或提交给其他开发调试。若快照内明文记录了客户生产环境的 Authorization 令牌、Cookie 密码或包含敏感 PII 的个人信息，会带来严重的安全合规风险。
*   **设计方案**：
    1.  在快照文件保存（`write_snapshot_suite`）之前，对内存中的 `SnapshotSuite` 深度遍历执行脱敏。
    2.  **Headers 脱敏**：将诸如 `Authorization`、`Proxy-Authorization`、`Cookie`、`Set-Cookie` 等核心头部的值一律重写为 `***` 或仅保留长度前缀（如 `Bearer ***`）。
    3.  **Body 敏感词掩码**：配置内置正则引擎，识别诸如手机号、银行卡号、电子邮箱以及特定密码 JSON 键（如 `"password": "..."`），在 JSON 或纯文本串中利用正则进行打码替换。

### 2. P1：易抖动动态字段的智能 JSON Diff 比对 (Smart Diff)
*   **需求背景**：许多 API 响应体中包含诸如时间戳（`$.timestamp`）、动态 Request ID（`$.uuid`）或生存时间等字段。如果重放时只是进行硬编码的 `String` 相等比对，会导致每次重放即使业务逻辑完全正确也依然报错。
*   **设计方案**：
    1.  在 `ReplayExecutor` 比对 Body 时，先探测 Content-Type。若是 JSON，则将新旧 Body 解析为 `serde_json::Value`。
    2.  实现一个比对过滤器，自动忽略特定的动态键。默认排除列表：
        - `timestamp`, `time`, `date`
        - `uuid`, `request_id`, `requestId`, `trace_id`, `traceId`
        - `token`, `session_id`, `sessionId`
    3.  支持在 `rupost.toml` 全局配置或在重放命令行中通过 `--ignore-fields <JSON_PATHS>` 手动指定要忽略的 JSONPath 指向。

### 3. P1：网络连接超时与断网事故现场捕获 (Virtual Response)
*   **需求背景**：当测试失败是由于网络连接超时、DNS 无法解析或连接重置引起的，此时并没有获取到正常的 HTTP Response，如果直接忽略会导致快照包没有记录下关键的错误动作。
*   **设计方案**：
    1.  当 `execute_batch` 返回 `Err` 且没有 `Response` 时，构造一个虚拟的 `ResponseSnapshot`。
    2.  预留特殊的 HTTP 状态码（例如 `599`）作为连接错误标识。
    3.  在响应 `body` 中将底层的物理连接错误消息（例如 `Connection timed out`, `DNS lookup failed`）进行格式化保存，从而使 Steve 在现场能够一键重放这些网络连接类故障。

### 4. P1：事后从历史记录补录导出快照 (Post-Facto Export)
*   **需求背景**：Dave 常常在未开启 `--save-snapshot` 时正常测试，但需要在此之后把最近一次的执行数据补录导出为快照。
*   **设计方案**：
    1.  **重构历史数据源**：由于现有的 `HistoryEntry` 其 `response` 字段类型是 `ResponseMeta`，不带 Body。我们需要将其重构为 `ResponseSnapshot`（包含完整的响应 Body）。
    2.  由于项目已经支持 20MB 阈值的 Lazy Compaction 压实机制，即使写入 Body，对本地磁盘和微秒级读写性能也毫无影响。
    3.  **实现 `export` 子命令**：
        - `rupost history export --last <N> --output <FILE>`：读取 `history.jsonl` 中最近的 N 次接口执行记录，提取并序列化生成 `SnapshotSuite`。
        - `rupost history export --interactive`：在终端调起简易交互面板供勾选导出。

### 5. P2：安全传输快照对称加密 (Encrypted Packages)
*   **需求背景**：部分高密金融或军工部署现场，严格禁止任何明文脚本与网络数据的导出传输。
*   **设计方案**：
    1.  在录制时，支持附加 `--encrypt` 命令。
    2.  提示或传递密码，使用 AES-256-GCM 对序列化后的 JSON 字符串进行高强度加密。
    3.  在重放时，通过 `rupost replay --decrypt` 输入解密密码后，临时在内存中解密重放，拒绝明文落盘，最大化合规性。
