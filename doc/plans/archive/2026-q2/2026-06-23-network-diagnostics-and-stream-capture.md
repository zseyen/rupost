# RuPost 深度网络诊断与大模型高级流式捕获技术与测试规范 (Stage 1)

本规范定义了 RuPost 后续演进第一阶段（Stage 1）的总体技术架构、向后兼容设计方案、以及相配套的单元与集成测试用例，旨在解决 `US 12`（深度网络诊断）与 `US 5`（流式大模型变量捕获）的核心诉求。

---

## 第一部分：技术方案 (Technical Specification)

### 1. 深度网络诊断指标共享与 DRY 重构

#### 1.1 数据结构共用设计 (Dry Principle)
为避免在测试模块和诊断模块（`rupost diagnose`）中引入冗余的数据耗时字段，我们提炼出基础的 `NetworkTiming` 结构体：

```rust
// src/http/diagnose.rs (或基础通用层)
use std::time::Duration;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkTiming {
    /// DNS 解析耗时
    pub dns_lookup_duration: Duration,
    /// TCP 握手耗时
    pub tcp_connect_duration: Duration,
    /// SSL/TLS 握手耗时（HTTP 请求为 None）
    pub tls_handshake_duration: Option<Duration>,
    /// TTFB 首字节耗时
    pub ttfb: Option<Duration>,
    /// 网络请求整体总耗时
    pub total_duration: Duration,
}
```

为了实现对历史数据（如 `history.jsonl`）和已有 API 规范的 **100% 零修改向后兼容**，重构后的 `DiagnosticsReport` 采用 `#[serde(flatten)]` 注解平铺嵌套的 `NetworkTiming`：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsReport {
    pub url: String,
    pub is_https: bool,
    pub resolved_ips: Vec<String>,
    
    /// 核心重构：使用 flatten 使得序列化时它的字段会直接平铺在一级，保证 JSON 结构完全不变
    #[serde(flatten)]
    pub timing: NetworkTiming,
    
    pub cert_info: Option<CertInfo>,
    pub http_status: Option<u16>,
    pub http_version: Option<String>,
}
```

#### 1.2 测试结果（`TestResult`）字段注入
在 `src/runner/types.rs` 中，为 `TestResult` 挂载可选的诊断细节：

```rust
pub struct TestResult {
    pub request_number: usize,
    pub name: Option<String>,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub duration: Duration, // 真实请求执行的物理总用时
    pub success: bool,
    pub error: Option<String>,
    
    /// 新增：与诊断模块共用的网络延迟指标细节 (非强制填充)
    pub timing_details: Option<NetworkTiming>,
}
```

#### 1.3 诊断探针触发与 Keep-Alive 智能对齐机制
*   **触发条件**：当执行用例遇到网络层连接失败时，或者用户在运行测试时附加了 `--debug` / `--diagnose` 选项时，`TestExecutor` 在执行请求后会调用 `diagnose_url` 触发并发探针探测，将细分延迟写入 `timing_details`。
*   **Keep-Alive 缓存链接处理**：为防止在 TCP 连接池复用时并发探测依然显示高握手时间导致用户误判，系统会执行 **时间差合理判定**：
    *   若真实执行耗时 `TestResult.duration` 显著小于探测的总耗时（如 `Real: 5ms` vs `Probe total: 50ms`），表明当前连接大概率是复用的缓存链接，此时终端瀑布图将不再强行展示诊断数值，而是渲染为：
        `[Connection Reused (Keep-Alive Cache)]`

---

### 2. 升级版流式变量捕获设计 (Advanced Stream Capture)

#### 2.1 捕获类型扩充 (CaptureSource)
在 `src/variable/capture.rs` 中，为 `CaptureSource` 引入流式专用的 `Stream` 事件过滤子类型，用以精准过滤并捕获大模型 SSE 事件流：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CaptureSource {
    Body(String),
    Header(String),
    TraceHeader,
    Cookie(String),
    Regex(String),
    
    /// 新增：特定于流式响应（SSE）的变量提取机制
    Stream {
        /// 提取的目标类型 (拼接出的完整文本，或是原始事件)
        target: StreamCaptureTarget,
        /// 可选：过滤特定的 event 类型 (如 "message" 或 "delta")
        event_filter: Option<String>,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StreamCaptureTarget {
    /// 自动合并拼接各帧 choices.delta.content 输出完整字符串
    LlmContent,
    /// 提取流中特定或全部 event 的 data payload 并以 JSON Array 格式拼接
    RawEvents,
}
```

#### 2.2 语法解析兼容设计
1.  **老语法兼容**：原有的 `# @capture var from body.path` 格式继续原封不动支持。
2.  **新语法扩展**：通过检测 `from` 关键字后的字符串前缀。若以 `"stream."` 开头，则进入流式捕获解析，支持以下语法：
    *   `# @capture var_name from stream.llm.content`
    *   `# @capture var_name from stream.events filter("event_type")`

---

## 第二部分：测试方案 (Test Plan)

为了实现 Stage 1 的卓越品质与极高的可靠性，将编写以下 5 个核心测试用例组：

### 1. 单元测试用例设计

#### 🧪 用例 1.1：`DiagnosticsReport` 平铺兼容性测试
*   **目的**：验证重构 `NetworkTiming` 后，已有的 `DiagnosticsReport` JSON 数据在对外传输和读取时没有格式损坏。
*   **前置条件**：具备包含 nested `NetworkTiming` 字段的 Mock 数据。
*   **测试步骤**：
    1. 实例化 `DiagnosticsReport` 对象，为其注入 `dns_lookup_duration = 10ms` 和 `tcp_connect_duration = 20ms`。
    2. 使用 `serde_json::to_string` 对其序列化。
    3. 断言生成的 JSON 字符串中，属性扁平展开在根节点下（即存在 `"dns_lookup_duration": ...`，而不存在嵌套的 `"timing": { ... }`）。
    4. 对生成的扁平 JSON 字符串使用 `serde_json::from_str` 反序列化为 `DiagnosticsReport`，断言字段值还原无损。
*   **预期结果**：序列化和反序列化顺利，完全实现对老版本数据存储格式的百分百兼容。

#### 🧪 用例 1.2：大模型流捕获（拼接文本）提取测试
*   **目的**：验证 `@capture var from stream.llm.content` 能够在长连接结束时完美合并 Token。
*   **前置条件**：Mock 模拟输出 3 帧标准的 OpenAI 格式流（如 `data: {"choices":[{"delta":{"content":"Rust"}}]}\n\n` 等）。
*   **测试步骤**：
    1. 在测试执行器中运行该 Mock 接口，并配置变量捕获规则：`VariableCapture::parse("final_reply", "stream.llm.content")`。
    2. 流式结束后，从执行上下文 `VariableContext` 中提取 `final_reply`。
    3. 断言提取出的字符串与期望拼接文本（例如 `Rust is perfect`）完全吻合。
*   **预期结果**：多帧增量 Token 被完整有序地重组成最终文本并注入上下文。

#### 🧪 用例 1.3：流式原始事件过滤提取测试
*   **目的**：验证流式事件过滤器 `filter("message")` 能够排除 heartbeat 等无关帧并输出 JSON Array。
*   **前置条件**：Mock 模拟输出 4 帧混合事件流（两帧 `event: message`, 一帧 `event: ping`, 一帧 `event: message`）。
*   **测试步骤**：
    1. 配置捕获规则：`VariableCapture::parse("raw_msgs", "stream.events filter(\"message\")")`。
    2. 执行流式数据包接收。
    3. 断言上下文变量 `raw_msgs` 中存入的值是包含 3 个元素的 JSON Array 字符串，且排除掉了 `ping` 帧。
*   **预期结果**：事件流根据指定的事件名称成功被过滤，非指定帧全数被拦截，产出数组格式符合契约。

---

### 2. 集成测试与手动验证方案

#### 🧪 用例 2.1：`timing_details` 指标注入集成测试
*   **目的**：验证常规 API 请求结束后，`timing_details` 探针数据被正确填充。
*   **前置条件**：本地 Mock 服务启动或可连通测试网络。
*   **测试步骤**：
    1. 配置常规的 HTTP 用例，显式指定在测试参数中开启 `--diagnose`。
    2. 运行 `TestExecutor::execute`。
    3. 获取返回的 `TestResult`，断言 `TestResult.timing_details` 为 `Some`。
    4. 断言其中的 `dns_lookup_duration` 和 `tcp_connect_duration` 的值大于 `0`（通常本地回路也会在几百微秒内完成）。
*   **预期结果**：诊断数据成功注入 `TestResult`，证明链路探测机制与测试执行流程成功串联。

#### 🧪 用例 2.2：Keep-Alive 缓存链接智能瀑布图拦截测试
*   **目的**：验证连接池复用时，系统不会被并发探测耗时误导。
*   **前置条件**：已建立长连接的 TCP 通道，使得下一次 API 交互实际开销极小。
*   **测试步骤**：
    1. 连跑 2 次发向同一个域名的请求。
    2. 第 2 次请求实际总耗时极小（例如 `3ms`），但并发探测的 TLS 握手时长仍可能为 `30ms`。
    3. 校验终端报告输出，确认没有为第二次请求强行渲染耗时瀑布图，而是安全退化为 `[Connection Reused (Keep-Alive Cache)]` 标识。
*   **预期结果**：前端展示逻辑智能规避了复用链接下的耗时指标误导，提供真实的定位依据。
