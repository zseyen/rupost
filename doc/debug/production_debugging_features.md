# 生产环境问题排查功能设计

## 使用场景

在局域网内的生产环境中，重现测试环境的测试，当出现问题时，工具需要能够辅助快速查找和定位问题。

### 典型流程
1. 测试环境：测试通过 ✅
2. 生产环境：相同请求失败 ❌
3. 需求：**快速定位差异和失败原因**

---

## 核心功能需求

### 🎯 优先级 P0（Phase 2/3 必须实现）

#### 1. 环境管理与变量替换

**目标**: 快速切换测试/生产环境配置，无需修改测试文件

**配置文件设计** (`rupost.toml`):

```toml
[environments.test]
base_url = "http://test-api.internal:8080"
api_key = "test-key-123"
timeout = 5000

[environments.staging]
base_url = "http://staging-api.internal:8080"
api_key = "${STAGING_API_KEY}"  # 从环境变量读取

[environments.prod]
base_url = "https://api.production.com"
api_key = "${PROD_API_KEY}"
timeout = 10000

# 全局默认配置
[defaults]
timeout = 5000
headers = { "User-Agent" = "rupost/0.1.0" }
```

**测试文件使用变量**:

```http
### 登录接口
POST {{base_url}}/api/login
Authorization: Bearer {{api_key}}
Content-Type: application/json

{
  "username": "admin",
  "password": "secret"
}
```

**命令行使用**:

```bash
# 使用测试环境
rupost test api-tests.http --env test

# 使用生产环境
rupost test api-tests.http --env prod

# 覆盖单个变量（局域网环境）
rupost test api-tests.http --env prod \
  --var base_url=http://192.168.1.10:8080

# 从文件加载环境变量
rupost test api-tests.http --env-file .env.prod
```

**实现要点**:
- **变量级联优先级**：`命令行覆盖 (--var key=val)` > `系统环境变量` > `本地局部环境文件 (.env 或 .env.<env>)` > `共享 rupost.toml 环境配置`
- **本地局部变量覆盖**：支持在根目录或指定路径自动检测并加载 `.env`，避免了团队成员共享 `rupost.toml` 时修改变量产生的 Git 提交冲突。可使用 `--env-file <file>` 显式重写。
- **系统环境变量覆盖**：采用“同名精准覆盖”原则，仅用当前进程的系统环境变量去覆盖已有同名键（例如 `base_url`），不会将不相干的系统全局变量（如 `PATH`）大面积引入污染 Context。
- **支持嵌套变量**: `{{base_url}}/{{api_version}}/login`
- **敏感信息保护**: API key 不出现在日志中

---

#### 2. 详细调试输出模式

**目标**: 提供完整的网络交互细节，快速定位问题

**命令**:

```bash
# 详细模式
rupost test api-tests.http --verbose

# 调试模式（更详细）
rupost test api-tests.http --debug

# 仅输出失败的详细信息
rupost test api-tests.http --debug-on-failure
```

**输出示例**:

```
[DEBUG] Request #1: 登录接口
  ┌─ Request Details ─────────────────────────────────────
  │ Timestamp: 2026-01-21 21:30:15.234
  │ Method: POST
  │ URL: https://api.production.com/api/login
  │ 
  │ Headers:
  │   Authorization: Bearer prod-***-key (masked)
  │   Content-Type: application/json
  │   User-Agent: rupost/0.1.0
  │   Accept: */*
  │ 
  │ Body (52 bytes):
  │   {"username": "admin", "password": "***"}
  │ 
  │ Network Diagnostics:
  │   DNS Lookup: 23ms → 192.168.1.10
  │   TCP Connect: 15ms
  │   TLS Handshake: 87ms (TLS 1.3)
  │   Time to First Byte: 234ms
  └───────────────────────────────────────────────────────
  
  ┌─ Response Details ────────────────────────────────────
  │ Status: 401 Unauthorized
  │ Duration: 256ms
  │ 
  │ Headers:
  │   Content-Type: application/json; charset=utf-8
  │   Content-Length: 89
  │   X-Request-Id: req-abc-123
  │   X-RateLimit-Remaining: 98
  │   Date: Tue, 21 Jan 2026 13:30:15 GMT
  │ 
  │ Body (89 bytes, formatted):
  │   {
  │     "error": "Invalid credentials",
  │     "code": "AUTH_FAILED",
  │     "timestamp": "2026-01-21T13:30:15Z",
  │     "request_id": "req-abc-123"
  │   }
  └───────────────────────────────────────────────────────
  
  ✗ FAILED
    Assertion: status == 200
    Actual: 401
    
  [Debugging Hints]
  - Check if API key is valid
  - Verify environment variable PROD_API_KEY is set
  - Check backend logs with: grep req-abc-123 /var/log/app.log
```

**关键诊断信息**:
- DNS 解析时间和结果（排查域名问题）
- TCP 连接时间（网络延迟）
- TLS 握手时间和版本（证书/协议问题）
- TTFB（服务器响应速度）
- Request ID（关联后端日志）
- Rate Limit 信息（API 限流）

**实现**:
```rust
pub struct Diagnostics {
    pub dns_lookup_time: Duration,
    pub dns_resolved_ip: Option<IpAddr>,
    pub tcp_connect_time: Duration,
    pub tls_handshake_time: Option<Duration>,
    pub tls_version: Option<String>,
    pub time_to_first_byte: Duration,
    pub total_time: Duration,
}
```

---

#### 3. 测试快照保存与对比

**目标**: 保存测试结果快照，方便历史对比和问题追溯

**保存快照**:

```bash
# 保存当前测试结果
rupost test api-tests.http --env prod --save-snapshot prod-baseline

# 自动生成时间戳命名
rupost test api-tests.http --env prod --save-snapshot
# 保存到: ~/.rupost/snapshots/prod-2026-01-21-213015.json
```

**快照格式** (`~/.rupost/snapshots/prod-baseline.json`):

```json
{
  "snapshot_name": "prod-baseline",
  "timestamp": "2026-01-21T13:30:15Z",
  "environment": "prod",
  "rupost_version": "0.1.0",
  "system_info": {
    "os": "macOS",
    "hostname": "dev-machine"
  },
  "requests": [
    {
      "name": "登录接口",
      "index": 1,
      "request": {
        "method": "POST",
        "url": "https://api.production.com/api/login",
        "headers": {
          "Authorization": "Bearer ***",
          "Content-Type": "application/json"
        },
        "body": "{\"username\":\"admin\",\"password\":\"***\"}"
      },
      "response": {
        "status": 200,
        "headers": {
          "Content-Type": "application/json",
          "X-Request-Id": "req-xyz-789"
        },
        "body": "{\"token\":\"...\",\"user_id\":123}",
        "body_hash": "sha256:abc123..."
      },
      "diagnostics": {
        "dns_lookup_ms": 23,
        "tcp_connect_ms": 15,
        "tls_handshake_ms": 87,
        "ttfb_ms": 156,
        "total_ms": 256
      },
      "assertions": {
        "passed": 3,
        "failed": 0,
        "details": [
          {"type": "status", "expected": "200", "actual": "200", "passed": true}
        ]
      }
    }
  ],
  "summary": {
    "total": 5,
    "passed": 5,
    "failed": 0,
    "duration_ms": 1234
  }
}
```

**对比快照**:

```bash
# 对比当前结果与历史快照
rupost test api-tests.http --env prod --compare-with prod-baseline

# 对比两个快照
rupost compare-snapshots prod-baseline prod-current
```

**差异报告输出**:

```
Snapshot Comparison
  Baseline: prod-baseline (2026-01-20 10:00:00)
  Current:  current run    (2026-01-21 21:30:15)
  Environment: prod

┌──────────────────┬────────────┬────────────┬────────┐
│ Request          │ Baseline   │ Current    │ Status │
├──────────────────┼────────────┼────────────┼────────┤
│ 登录接口         │ 200 (156ms)│ 401 (256ms)│ ✗ FAIL │
│ 获取用户列表     │ 200 (23ms) │ 200 (134ms)│ ⚠ SLOW │
│ 创建订单         │ 201 (67ms) │ 201 (189ms)│ ⚠ SLOW │
│ 更新用户信息     │ 200 (45ms) │ 200 (43ms) │ ✓ PASS │
│ 删除订单         │ 204 (34ms) │ 204 (36ms) │ ✓ PASS │
└──────────────────┴────────────┴────────────┴────────┘

[Detailed Differences]

Request #1: 登录接口
  Status:   200 → 401     ✗ CHANGED
  Duration: 156ms → 256ms ⚠ SLOWER (+64%)
  
  Response Headers Diff:
    - X-Request-Id: req-xyz-789
    + X-Request-Id: req-abc-123
  
  Response Body Diff:
    - {"token": "...", "user_id": 123}
    + {"error": "Invalid credentials", "code": "AUTH_FAILED"}
  
  [Possible Causes]
  - API key may have expired
  - Different authentication configuration in prod
  - Rate limiting or security policy change

Request #2: 获取用户列表
  Status:   200 → 200     ✓ SAME
  Duration: 23ms → 134ms  ⚠ SLOWER (+483%)
  
  [Performance Degradation Alert]
  - Response time increased significantly
  - Check database query performance
  - Check network latency between services
```

---

### 🎯 优先级 P1（Phase 3）

#### 4. 网络诊断工具

**目标**: 内置网络诊断，快速排查连接问题

**命令**:

```bash
# 诊断指定 URL
rupost diagnose https://api.production.com/health

# 诊断所有测试文件中的端点
rupost diagnose --from-file api-tests.http --env prod
```

**输出**:

```
[Connectivity Diagnostics]
Target: https://api.production.com/health

✓ DNS Resolution
  Query:    api.production.com
  Result:   192.168.1.10
  Time:     23ms
  Nameserver: 8.8.8.8

✓ TCP Connection
  Address:  192.168.1.10:443
  Time:     15ms
  
✓ TLS Handshake
  Protocol: TLS 1.3
  Cipher:   TLS_AES_256_GCM_SHA384
  Time:     87ms
  
✓ HTTP Response
  Status:   200 OK
  Time:     234ms
  
[Certificate Information]
  Subject:       CN=api.production.com
  Issuer:        Let's Encrypt Authority X3
  Valid From:    2026-01-01 00:00:00 UTC
  Valid Until:   2026-12-31 23:59:59 UTC
  Days Remaining: 345
  SANs:          api.production.com, *.production.com
  
[Performance Breakdown]
  DNS Lookup:    23ms  (6%)
  TCP Connect:   15ms  (4%)
  TLS Handshake: 87ms  (24%)
  Server Process:109ms (30%)
  Response:      125ms (35%)
  ─────────────────────────
  Total:         359ms (100%)
  
[Network Path]
  Hop 1: 192.168.1.1     (1ms)
  Hop 2: 10.0.0.1        (5ms)
  Hop 3: 192.168.1.10    (15ms)
  
✓ All checks passed
```

---

#### 5. 请求录制与回放

**目标**: 录制成功的测试，在生产环境精确重现

**录制**:

```bash
# 录制测试环境的请求
rupost record api-tests.http --env test --output test-baseline.json
```

**回放**:

```bash
# 在生产环境回放
rupost replay test-baseline.json --env prod

# 回放并对比
rupost replay test-baseline.json --env prod --compare
```

---

#### 6. 增强的断言失败报告

**目标**: 断言失败时提供可操作的调试建议

**输出示例**:

```
✗ Assertion Failed: Request #1 - 登录接口

  Expected: status == 200
  Actual:   status == 401
  
  [Request Context]
  - Method: POST
  - URL: https://api.production.com/api/login
  - Environment: prod
  - Request ID: req-abc-123
  
  [Possible Causes]
  1. Authorization header may be incorrect
     Current value: Bearer prod-***-key
     Verify with: echo $PROD_API_KEY
     
  2. API key may have expired
     Check expiration: rupost config show --env prod
     
  3. Rate limiting or IP blocking
     Check response headers: X-RateLimit-Remaining
     
  [Quick Debugging Commands]
  # Test with curl
  curl -v -H "Authorization: Bearer $PROD_API_KEY" \
       https://api.production.com/api/login
  
  # Check environment config
  rupost config show --env prod
  
  # Check backend logs (if accessible)
  grep req-abc-123 /var/log/app.log
  
  [Related Documentation]
  - Authentication guide: docs/auth.md
  - Environment setup: docs/environments.md
```

---

### 🎯 优先级 P2（Phase 4，可选）

#### 7. 并行环境对比

```bash
# 同时测试两个环境并对比
rupost compare --env test --env prod api-tests.http
```

#### 8. Mock Server 功能

```bash
# 基于快照启动 mock server
rupost mock --snapshot test-baseline.json --port 8080
```

---

## 数据结构设计

### 增强的测试结果

```rust
// src/runner/result.rs

pub struct TestResult {
    pub name: String,
    pub index: usize,
    pub status: TestStatus,
    pub duration: Duration,
    
    // 新增：详细诊断信息
    pub diagnostics: Diagnostics,
    
    // 新增：快照支持
    pub request_snapshot: RequestSnapshot,
    pub response_snapshot: ResponseSnapshot,
    
    // 新增：断言详情
    pub assertions: Vec<AssertionResult>,
}

pub struct Diagnostics {
    pub timestamp: DateTime<Utc>,
    pub dns_lookup_time: Duration,
    pub dns_resolved_ip: Option<IpAddr>,
    pub tcp_connect_time: Duration,
    pub tls_handshake_time: Option<Duration>,
    pub tls_version: Option<String>,
    pub tls_cipher: Option<String>,
    pub time_to_first_byte: Duration,
    pub total_time: Duration,
}

pub struct RequestSnapshot {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
    pub body_hash: Option<String>,
}

pub struct ResponseSnapshot {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
    pub body_hash: String, // SHA-256
}

pub struct AssertionResult {
    pub assertion_type: String,
    pub expected: String,
    pub actual: String,
    pub passed: bool,
    pub message: Option<String>,
}
```

---

## 存储结构

```
~/.rupost/
├── config.toml              # 全局配置
├── environments/
│   ├── test.toml            # 测试环境配置
│   ├── staging.toml         # 预发布环境
│   └── prod.toml            # 生产环境配置
├── snapshots/
│   ├── prod-baseline.json
│   ├── test-baseline.json
│   └── prod-2026-01-21-213015.json
└── history/
    └── 2026-01-21/
        ├── 13-22-00.json
        └── 21-30-15.json
```

---

## CLI 命令扩展

```bash
# 环境管理
rupost config show --env prod           # 显示环境配置
rupost config list                       # 列出所有环境
rupost config validate --env prod       # 验证环境配置

# 快照管理
rupost snapshot list                    # 列出所有快照
rupost snapshot show prod-baseline      # 显示快照详情
rupost snapshot diff baseline current   # 对比两个快照
rupost snapshot clean --older-than 30d  # 清理旧快照

# 诊断工具
rupost diagnose <url>                   # 诊断单个 URL
rupost diagnose --from-file <file>      # 诊断文件中所有端点

# 历史记录
rupost history list                     # 列出历史记录
rupost history show <id>                # 显示详细记录
rupost history compare <id1> <id2>      # 对比两次测试
```

---

## 实施优先级

### Phase 2（基础 MVP）
- [ ] 基础环境变量替换功能
- [ ] `--verbose` 详细输出模式
- [ ] 简单的请求/响应日志

### Phase 3（调试增强）
- [ ] 完整的环境管理系统
- [ ] `--debug` 模式（包含网络诊断）
- [ ] 快照保存与加载
- [ ] 快照对比功能
- [ ] 网络诊断工具

### Phase 4（高级功能）
- [ ] 请求录制与回放
- [ ] 并行环境对比
- [ ] Mock Server
- [ ] 智能错误诊断建议

---

## 成功标准

✅ 能够快速切换测试/生产环境  
✅ 提供详细的网络诊断信息  
✅ 支持测试结果快照和对比  
✅ 断言失败时给出可操作的调试建议  
✅ 敏感信息（API key）得到保护  
✅ 历史记录可追溯  
✅ 用户能在 5 分钟内定位生产环境问题

---

## 差异化定位更新

> **RuPost: API 文档即测试，生产调试更简单**
> 
> - 📝 Markdown 写文档，同步跑测试
> - 🔄 一键切换环境，快速排查问题
> - 🔍 详细诊断信息，精准定位故障
> - 📸 快照对比功能，追溯历史变化
> - � 日志自动关联，快速定位根因
> - �🛠️ 调试友好设计，提升开发效率

---

## 日志集成与追踪功能

### 🎯 优先级 P1（Phase 3）

#### 9. Trace ID / Request ID 自动提取与关联

**目标**: 自动从响应中提取 Trace ID，提供快速日志查询命令

---

**工作机制**:

RuPost 会自动从后端响应的 HTTP headers 中提取 Trace ID，主要有以下几种方式：

##### 方式 1: 从响应中自动提取（主要方式）

```
[1] RuPost 发送请求
    POST /api/login
    ↓

[2] 后端服务处理请求
    - 生成/接收 Request ID: req-abc-123
    - 生成/传播 Trace ID: trace-xyz-789
    - 记录日志时附带这些 ID
    ↓

[3] 后端返回响应 (带 headers)
    HTTP/1.1 200 OK
    X-Request-Id: req-abc-123        ← RuPost 自动提取
    X-Trace-Id: trace-xyz-789        ← RuPost 自动提取
    X-Correlation-Id: corr-123       ← RuPost 自动提取
    ↓

[4] RuPost 自动识别并显示
    [Trace Information]
    Request ID: req-abc-123
    Trace ID: trace-xyz-789
    ↓

[5] 用于后续日志查询
    rupost logs show --trace-id trace-xyz-789
```

---

**自动识别的 Header 列表**:

```rust
// 自动识别的 Trace/Request ID headers
const TRACE_HEADERS: &[&str] = &[
    "X-Request-Id",
    "X-Trace-Id",
    "X-Correlation-Id",
    "X-B3-TraceId",           // Zipkin
    "Traceparent",            // W3C Trace Context
    "X-Amzn-Trace-Id",        // AWS
    "X-Cloud-Trace-Context",  // GCP
];
```

---

**配置自定义 Header**:

```toml
# rupost.toml
[trace]
# 自定义要识别的 header 名称
headers = ["X-Request-Id", "X-My-Custom-Trace-Id"]

# 自动为请求生成 ID
auto_generate_request_id = true
request_id_header = "X-Request-Id"
format = "uuid_v4"  # 或 "timestamp" 或 "sequential"

# 日志查询命令模板
log_query_template = "grep {trace_id} /var/log/app/*.log"

# SSH 远程日志查询
[trace.remote]
enabled = true
host = "prod-server.internal"
user = "deploy"
log_path = "/var/log/app/application.log"
```

---

**自动提取并显示**:

```
[DEBUG] Response Headers
  Content-Type: application/json
  X-Request-Id: req-abc-123       ← [TRACE ID]
  X-Correlation-Id: corr-xyz-789  ← [TRACE ID]

[Trace Information]
  Request ID: req-abc-123
  Correlation ID: corr-xyz-789
  
  [Quick Log Query]
  Local:  grep req-abc-123 /var/log/app/*.log
  Remote: ssh deploy@prod-server.internal "grep req-abc-123 /var/log/app/application.log"
```

---

##### 方式 2: 在请求中传递 Trace ID（链路追踪）

**场景 A: 手动指定**

```http
### 登录接口
POST {{base_url}}/api/login
X-Request-Id: my-custom-id-123
X-Trace-Id: inherited-trace-456
Content-Type: application/json

{"username": "admin"}
```

**场景 B: 自动生成（配置后）**

```toml
# rupost.toml
[trace]
auto_generate = true
header_name = "X-Request-Id"
format = "uuid_v4"
```

RuPost 会自动添加 header：
```http
POST /api/login
X-Request-Id: 550e8400-e29b-41d4-a716-446655440000  ← 自动生成
```

---

##### 方式 3: 多步骤测试中传递 Trace ID

支持在测试文件中捕获并传递 Trace ID（需要变量捕获功能，Phase 3）：

```http
### Step 1: 登录
POST {{base_url}}/api/login
Content-Type: application/json

{"username": "admin"}

# @capture trace_id from header X-Trace-Id

### Step 2: 获取用户信息（使用同一个 Trace ID）
GET {{base_url}}/api/users/me
X-Trace-Id: {{trace_id}}  ← 传递上一步的 Trace ID
Authorization: Bearer {{token}}
```

这样所有步骤共享同一个 Trace ID，方便关联日志和追踪完整链路。

---

**实现示例**:

```rust
// src/http/client.rs
pub async fn execute(&self, request: Request) -> Result<Response> {
    let resp = self.reqwest_client.execute(request.into()).await?;
    
    // 提取 Trace 信息
    let trace_info = extract_trace_info(&resp.headers());
    
    Ok(Response {
        status: resp.status().as_u16(),
        headers: resp.headers().clone(),
        body: resp.text().await?,
        trace_info, // ← 保存提取的 Trace ID
    })
}

fn extract_trace_info(headers: &HeaderMap) -> Option<TraceInfo> {
    let mut trace_info = TraceInfo::default();
    
    // 检查所有可能的 header
    for &header_name in TRACE_HEADERS {
        if let Some(value) = headers.get(header_name) {
            if let Ok(id) = value.to_str() {
                match header_name {
                    "X-Request-Id" => trace_info.request_id = Some(id.to_string()),
                    "X-Trace-Id" | "Traceparent" => trace_info.trace_id = Some(id.to_string()),
                    "X-Correlation-Id" => trace_info.correlation_id = Some(id.to_string()),
                    _ => {}
                }
            }
        }
    }
    
    if trace_info.has_any_id() {
        Some(trace_info)
    } else {
        None
    }
}
```

---

**使用场景示例**:

假设你在测试一个支付流程：

```http
### 创建订单
POST {{base_url}}/api/orders
Content-Type: application/json

{"amount": 100, "currency": "USD"}

# 后端响应:
# X-Request-Id: req-order-123
# X-Trace-Id: trace-payment-789
```

RuPost 自动提取后，你可以：

```bash
# 1. 查看这个请求在所有服务中的日志
rupost logs show --trace-id trace-payment-789

# 2. 查看分布式追踪链路
rupost trace show --trace-id trace-payment-789

# 输出:
# [Distributed Trace: trace-payment-789]
# api-gateway (234ms)
#   → order-service (123ms)
#     → inventory-service (45ms)
#     → payment-service (89ms)
#       → payment-gateway (67ms)

# 3. 在 Kubernetes 中查询
rupost logs show --trace-id trace-payment-789 --source kubernetes --all-pods

# 4. 从 Elasticsearch 查询
rupost logs show --trace-id trace-payment-789 --source elasticsearch
```


---

#### 10. 远程日志查询集成

**目标**: 自动获取生产环境日志，无需手动 SSH

**方式 1: SSH 自动查询**

配置:
```toml
# rupost.toml
[logs.ssh]
enabled = true
host = "prod-server.internal"
user = "deploy"
key_file = "~/.ssh/id_rsa"
log_path = "/var/log/app/application.log"
context_lines = 10  # 前后各显示 10 行
```

命令:
```bash
# 自动查询最近一次测试的日志
rupost logs show

# 查询指定 trace ID 的日志
rupost logs show --trace-id req-abc-123

# 实时跟踪日志（类似 tail -f）
rupost test api-tests.http --env prod --follow-logs
```

输出:
```
[Remote Logs: prod-server.internal]
Searching for trace ID: req-abc-123

2026-01-21 21:30:15.123 INFO  [req-abc-123] Received login request from 192.168.1.100
2026-01-21 21:30:15.145 DEBUG [req-abc-123] Validating API key: prod-***
2026-01-21 21:30:15.167 WARN  [req-abc-123] API key validation failed: key expired
2026-01-21 21:30:15.178 ERROR [req-abc-123] Authentication failed for user: admin
2026-01-21 21:30:15.189 INFO  [req-abc-123] Returning 401 Unauthorized

[Log Analysis]
  ✗ ERROR detected: Authentication failed
  Cause: API key expired
  
  [Recommendation]
  - Rotate API key in environment config
  - Update PROD_API_KEY environment variable
```

---

**方式 2: Kubernetes Pod 日志查询**

**目标**: 从 Kubernetes 集群中获取 Pod 日志（适用于 k8s/k9s 管理的容器环境）

配置:
```toml
# rupost.toml
[logs.kubernetes]
enabled = true

# Kubernetes 配置文件路径
kubeconfig = "~/.kube/config"

# 或者使用 in-cluster 配置（当 rupost 本身运行在 k8s 中）
# in_cluster = true

# 目标命名空间
namespace = "production"

# Pod 选择器（支持多种方式）
pod_selector = "app=api-service"  # Label selector

# 或者直接指定 Pod 名称前缀
# pod_name_prefix = "api-service"

# 容器名称（如果 Pod 中有多个容器）
container = "api"

# 日志行数限制
tail_lines = 500

# 是否包含之前的日志（Pod 重启前）
previous = false
```

**自动识别 Pod**:

RuPost 可以通过多种方式自动找到正确的 Pod：

1. **通过 Label Selector**（推荐）
```toml
[logs.kubernetes]
namespace = "production"
pod_selector = "app=api-service,version=v1.2.3"
```

2. **通过 Deployment/Service 名称**
```toml
[logs.kubernetes]
namespace = "production"
deployment = "api-service"
```

3. **通过环境变量中的元数据**
```toml
[logs.kubernetes]
# 从环境配置中读取
namespace = "${K8S_NAMESPACE}"
pod_selector = "app=${SERVICE_NAME}"
```

命令:
```bash
# 自动查询最近一次测试的日志
rupost logs show --source kubernetes

# 查询指定 trace ID 的日志
rupost logs show --trace-id req-abc-123 --source kubernetes

# 实时跟踪日志（类似 kubectl logs -f）
rupost test api-tests.http --env prod --follow-logs --source kubernetes

# 查看 Pod 重启前的日志
rupost logs show --trace-id req-abc-123 --previous

# 手动指定 Pod
rupost logs show --pod api-service-7d9f8b6c4-abc12 --namespace production
```

输出:
```
[Kubernetes Logs]
Cluster: production-cluster
Namespace: production
Pod: api-service-7d9f8b6c4-abc12
Container: api

[Pod Information]
  Node: node-3.internal
  Status: Running
  Uptime: 2d 15h 34m
  Restarts: 0
  
Searching for trace ID: req-abc-123

2026-01-21 21:30:15.123 INFO  [req-abc-123] Received login request from 192.168.1.100
2026-01-21 21:30:15.145 DEBUG [req-abc-123] Validating API key: prod-***
2026-01-21 21:30:15.167 WARN  [req-abc-123] API key validation failed: key expired
2026-01-21 21:30:15.178 ERROR [req-abc-123] Authentication failed for user: admin
2026-01-21 21:30:15.189 INFO  [req-abc-123] Returning 401 Unauthorized

[Log Analysis]
  ✗ ERROR detected: Authentication failed
  Cause: API key expired
  
  [Pod Context]
  - Container Image: registry.internal/api-service:v1.2.3
  - Environment: production
  - Replicas: 3 running
  
  [Quick Commands]
  # View all pods
  kubectl get pods -n production -l app=api-service
  
  # Check pod events
  kubectl describe pod api-service-7d9f8b6c4-abc12 -n production
  
  # Execute into pod
  kubectl exec -it api-service-7d9f8b6c4-abc12 -n production -- /bin/sh
```

**多 Pod 聚合日志**:

当服务有多个副本时，RuPost 可以聚合所有 Pod 的日志：

```bash
# 聚合所有匹配的 Pod 日志
rupost logs show --trace-id req-abc-123 --all-pods

# 只显示包含该 trace ID 的 Pod
rupost logs show --trace-id req-abc-123 --filter-pods
```

输出:
```
[Kubernetes Logs - Multiple Pods]
Namespace: production
Selector: app=api-service
Found 3 pods

[Pod: api-service-7d9f8b6c4-abc12] Node: node-3
  ✓ Found 5 log lines with trace ID: req-abc-123
  
[Pod: api-service-7d9f8b6c4-def34] Node: node-1
  ✗ No matching logs
  
[Pod: api-service-7d9f8b6c4-ghi56] Node: node-2
  ✗ No matching logs

[Aggregated Logs from api-service-7d9f8b6c4-abc12]
2026-01-21 21:30:15.123 INFO  [req-abc-123] Received login request...
2026-01-21 21:30:15.145 DEBUG [req-abc-123] Validating API key...
...

[Analysis]
Request was handled by: api-service-7d9f8b6c4-abc12 on node-3
Other replicas did not receive this request (load balancer routing)
```

**与 k9s 集成**:

如果你已经在使用 k9s，RuPost 可以直接使用相同的配置：

```toml
# rupost.toml
[logs.kubernetes]
# 使用与 k9s 相同的 kubeconfig
kubeconfig = "~/.kube/config"

# 可以设置 k9s 命令别名
[logs.kubernetes.k9s]
enabled = true
# 失败时提供 k9s 命令
show_k9s_command = true
```

输出包含 k9s 快捷命令:
```
[Quick Access with k9s]
  # View pod logs in k9s
  k9s -n production --pod api-service-7d9f8b6c4-abc12
  
  # View pod details
  k9s -n production --pod api-service-7d9f8b6c4-abc12 --describe
```

---

**方式 3: 日志聚合平台 API 集成**


支持常见的日志平台:
- **Elasticsearch + Kibana**
- **Grafana Loki**
- **Splunk**
- **DataDog**
- **CloudWatch Logs (AWS)**

配置示例:
```toml
# rupost.toml
[logs.elasticsearch]
enabled = true
endpoint = "https://elasticsearch.internal:9200"
index_pattern = "app-logs-*"
auth_token = "${ES_AUTH_TOKEN}"
```

```toml
[logs.loki]
enabled = true
endpoint = "https://loki.internal:3100"
query_template = '{app="api-service"} |= "{trace_id}"'
```

命令:
```bash
# 自动从 Elasticsearch 查询
rupost logs show --trace-id req-abc-123 --source elasticsearch

# 指定时间范围
rupost logs show --trace-id req-abc-123 --since "10m ago"
```

输出:
```
[Elasticsearch Query]
Index: app-logs-2026-01-21
Query: X-Request-Id:"req-abc-123"
Time Range: Last 10 minutes

Found 5 log entries:

[21:30:15.123] INFO  | service=api | trace_id=req-abc-123
  message: "Received login request from 192.168.1.100"

[21:30:15.167] WARN  | service=api | trace_id=req-abc-123
  message: "API key validation failed: key expired"
  details: {
    "key_hash": "sha256:abc...",
    "expiry_date": "2026-01-20T00:00:00Z"
  }

[21:30:15.178] ERROR | service=api | trace_id=req-abc-123
  message: "Authentication failed for user: admin"
  error_code: "AUTH_KEY_EXPIRED"
  
[Log Statistics]
  Total: 5 entries
  Errors: 1
  Warnings: 1
  Duration: 66ms (from first to last log)
```

---

#### 11. 分布式追踪集成

**目标**: 集成分布式追踪系统，查看完整的请求链路

**支持的追踪系统**:
- **Jaeger**
- **Zipkin**
- **OpenTelemetry**
- **AWS X-Ray**

配置:
```toml
[trace.jaeger]
enabled = true
endpoint = "https://jaeger.internal:16686"
ui_base_url = "https://jaeger-ui.internal"
```

命令:
```bash
# 查看追踪链路
rupost trace show --trace-id req-abc-123

# 在浏览器中打开追踪 UI
rupost trace open --trace-id req-abc-123
```

输出:
```
[Distributed Trace: req-abc-123]
Service: api-gateway → auth-service → user-service → database

┌────────────────────────────────────────────┐
│ Span Timeline                              │
├────────────────────────────────────────────┤
│ api-gateway     [████████████] 256ms       │
│   ├─ auth-svc   [████] 67ms                │
│   │   └─ cache  [█] 5ms                    │
│   └─ user-svc   [██████] 145ms             │
│       └─ db     [████] 89ms                │
└────────────────────────────────────────────┘

[Trace Details]
  Total Duration: 256ms
  Total Spans: 5
  Services: 4
  Errors: 1 (auth-service: key validation failed)
  
[Open in Browser]
  https://jaeger-ui.internal/trace/req-abc-123
```

---

#### 12. 智能日志分析与错误聚合

**目标**: 自动分析日志，识别常见错误模式

**自动识别错误模式**:

```
[Automatic Log Analysis]
Analyzing logs for trace ID: req-abc-123

✗ Error Pattern Detected: API_KEY_EXPIRED

  [Pattern Signature]
  - Error Code: AUTH_KEY_EXPIRED
  - Frequency: 234 occurrences in last 24h
  - First Seen: 2026-01-21 08:00:00
  - Affected Users: 15
  
  [Root Cause Analysis]
  API keys expired on 2026-01-20 00:00:00 but not rotated
  
  [Similar Errors]
  - req-def-456 (10 minutes ago)
  - req-ghi-789 (15 minutes ago)
  - 232 more...
  
  [Recommended Actions]
  1. Rotate API keys immediately
  2. Update deployment configuration
  3. Notify affected users
  
  [Related Incidents]
  - INC-2024-001: API key rotation failure
  - Previous occurrence: 2025-12-15
```

---

#### 13. 依赖服务健康检查

**目标**: 自动检查依赖服务状态，快速定位问题源头

配置:
```toml
[dependencies]
[[dependencies.service]]
name = "auth-service"
health_url = "http://auth-service.internal/health"

[[dependencies.service]]
name = "user-database"
health_url = "http://db-proxy.internal:8080/health"

[[dependencies.service]]
name = "cache-redis"
health_url = "http://redis.internal:6379/ping"
```

命令:
```bash
# 检查所有依赖服务
rupost health check

# 测试前自动检查
rupost test api-tests.http --env prod --check-health
```

输出:
```
[Dependency Health Check]

✓ auth-service
  Status: Healthy
  Response Time: 23ms
  Uptime: 45d 3h 12m

✗ user-database
  Status: Degraded
  Response Time: 2345ms  ⚠ SLOW (threshold: 500ms)
  Uptime: 2d 15h
  Error: Connection pool exhausted
  
✓ cache-redis
  Status: Healthy
  Response Time: 5ms
  Uptime: 120d 8h

[Overall Status]
  2/3 healthy
  1 degraded
  
[Impact Analysis]
  Failed request: POST /api/login
  Possible cause: user-database degradation
  
  [Recommendation]
  - Check database connection pool settings
  - Review recent database queries
  - Consider scaling database instances
```

---

#### 14. 实时错误通知

**目标**: 测试失败时自动发送通知

配置:
```toml
[notifications]
enabled = true

[notifications.slack]
webhook_url = "${SLACK_WEBHOOK_URL}"
channel = "#api-alerts"
notify_on = ["error", "slow_response"]

[notifications.email]
smtp_server = "smtp.company.com"
to = ["team@company.com"]
notify_on = ["error"]
```

当测试失败时:
```
📱 Slack notification sent to #api-alerts
📧 Email notification sent to team@company.com

[Notification Content]
Title: ❌ API Test Failed: Login Endpoint
Environment: production
Request ID: req-abc-123
Error: 401 Unauthorized
Timestamp: 2026-01-21 21:30:15

View details: https://jaeger-ui.internal/trace/req-abc-123
```

---

## 增强的数据结构

```rust
// src/runner/result.rs

pub struct TestResult {
    // ... 现有字段 ...
    
    // 新增：追踪信息
    pub trace_info: Option<TraceInfo>,
    
    // 新增：日志链接
    pub log_links: Vec<LogLink>,
    
    // 新增：依赖服务状态
    pub dependency_health: Option<Vec<DependencyStatus>>,
}

pub struct TraceInfo {
    pub request_id: Option<String>,
    pub trace_id: Option<String>,
    pub correlation_id: Option<String>,
    pub parent_span_id: Option<String>,
}

pub struct LogLink {
    pub platform: String,  // "elasticsearch", "loki", "ssh"
    pub query_url: Option<String>,
    pub query_command: Option<String>,
}

pub struct DependencyStatus {
    pub name: String,
    pub status: HealthStatus,
    pub response_time: Duration,
    pub error_message: Option<String>,
}

pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}
```

---

## 扩展的 CLI 命令

```bash
# 日志查询
rupost logs show                         # 显示最近一次测试的日志
rupost logs show --trace-id <id>         # 查询指定 trace ID
rupost logs show --source elasticsearch  # 指定日志源
rupost logs follow                       # 实时跟踪日志

# 追踪查看
rupost trace show --trace-id <id>        # 显示追踪详情
rupost trace open --trace-id <id>        # 在浏览器打开追踪 UI
rupost trace analyze --trace-id <id>     # 分析追踪性能

# 健康检查
rupost health check                      # 检查所有依赖服务
rupost health watch                      # 持续监控服务健康

# 通知配置
rupost notify test                       # 测试通知配置
rupost notify list                       # 列出通知配置
```

---

## 依赖库建议

```toml
[dependencies]
# SSH 连接
ssh2 = "0.9"

# Kubernetes API 客户端
kube = { version = "0.87", features = ["runtime", "derive"] }
k8s-openapi = { version = "0.20", features = ["v1_28"] }

# 日志解析
regex = "1.10"

# HTTP 客户端（日志平台 API）
reqwest = { version = "0.13", features = ["json"] }

# JSON 查询（Elasticsearch 结果解析）
serde_json = "1.0"
jql = "7.1"  # JSON Query Language

# 通知
lettre = "0.11"  # Email
```

---

## 实施优先级更新

### Phase 2（基础 MVP）
- [ ] 基础环境变量替换功能
- [ ] `--verbose` 详细输出模式
- [ ] 简单的请求/响应日志
- [ ] **Trace ID 自动提取和显示**

### Phase 3（调试增强）
- [ ] 完整的环境管理系统
- [ ] `--debug` 模式（包含网络诊断）
- [ ] 快照保存与加载
- [ ] 快照对比功能
- [ ] 网络诊断工具
- [ ] **远程日志查询（SSH）**
- [ ] **日志聚合平台集成**
- [ ] **依赖服务健康检查**

### Phase 4（高级功能）
- [ ] 请求录制与回放
- [ ] 并行环境对比
- [ ] Mock Server
- [ ] 智能错误诊断建议
- [ ] **分布式追踪集成**
- [ ] **智能日志分析**
- [ ] **实时错误通知**

---

## 成功标准（更新）

✅ 能够快速切换测试/生产环境  
✅ 提供详细的网络诊断信息  
✅ 支持测试结果快照和对比  
✅ 断言失败时给出可操作的调试建议  
✅ **自动提取并关联 Trace ID**  
✅ **支持远程日志查询和实时跟踪**  
✅ **集成常见日志聚合平台**  
✅ **自动检查依赖服务健康状态**  
✅ 敏感信息（API key）得到保护  
✅ 历史记录可追溯  
✅ 用户能在 **3 分钟**内定位生产环境问题根因

---

## 使用场景示例：完整的调试流程

### 问题：生产环境登录接口突然开始返回 401

```bash
# 1. 运行测试，自动检查依赖服务健康
$ rupost test api-tests.http --env prod --check-health --follow-logs

[Dependency Health Check]
✓ auth-service (23ms)
✗ user-database (2345ms) - SLOW
✓ cache-redis (5ms)

[Test Execution]
✗ Request #1: 登录接口 - FAILED (401)
  Request ID: req-abc-123
  Trace ID: trace-xyz-789

[Remote Logs - Live]
2026-01-21 21:30:15.167 WARN  [req-abc-123] API key validation failed: key expired
2026-01-21 21:30:15.178 ERROR [req-abc-123] Authentication failed for user: admin

[Root Cause Detected]
API key expired on 2026-01-20
Affected: 234 requests in last 24h

[Recommendations]
1. Rotate API key: rupost config set --env prod api_key=<new_key>
2. View full trace: rupost trace open --trace-id trace-xyz-789
3. Check similar errors: rupost logs show --pattern "API key" --since 24h

# 2. 查看完整的追踪链路
$ rupost trace show --trace-id trace-xyz-789

[Distributed Trace]
api-gateway (256ms) → auth-service (67ms) → cache (5ms)
                   → [FAILED] Key validation

# 3. 查看历史趋势
$ rupost logs show --pattern "key expired" --since 7d --aggregate

[Pattern Analysis: "key expired"]
2026-01-21: 234 occurrences  ← Current issue
2026-01-20: 0
2026-01-14: 187 occurrences  ← Previous key rotation
...

[Solution]
API keys are configured with 7-day expiration.
Set up automated key rotation or extend expiration period.
```

**结果**: 3 分钟内定位问题根因（API key 过期），并获得可执行的解决方案。
