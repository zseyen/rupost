# RuPost 网络诊断工具使用与讲解 (Network Diagnostics Guide)

`rupost diagnose`（别名 `rupost d`）是一个独立的连通性分析与高亮诊断工具。当测试用例因为网络、证书或反向代理网关问题失败时，你可以使用它快速定位故障。

---

## 设计原理与第一性原理

普通的 HTTP 库（如 `reqwest`）将底层连接细节进行了封装。如果请求超时或报错，很难清晰区分到底是 **DNS 解析慢**、**TCP 握手被拦截**、**TLS 握手协商失败** 还是 **后端接口响应迟缓（TTFB 慢）**。

RuPost 网络诊断遵循**第一性原理**：
1. **绕过高级 HTTP 库**，直接操作底层 `tokio::net::TcpStream`。
2. **手动控制网络握手阶段**：
   * 显式调用 `lookup_host` 测量 DNS 解析耗时。
   * 手动对目标 IP 和端口发起 TCP 连接，测量建连耗时。
   * 手动通过 `tokio-rustls`（配合 `ring` 加密后端）发起 TLS 握手，测量证书验证及秘钥协商耗时。
   * 手动发送原始规范 HTTP / WebSocket Upgrade 报文，并测量读取到第一个字节响应的时延（TTFB）。

---

## 时延拆分瀑布图指标说明

在诊断结果中，RuPost 会在终端为您绘制一张精美的 Latency Breakdown 瀑布图。各项指标的定义如下：

| 指标 (Metric) | 含义解释 | 常见故障排查建议 |
| :--- | :--- | :--- |
| **DNS Lookup** | 将域名（例如 `example.com`）解析为 IP 地址列表的时间。 | 如果耗时过长（如超过 500ms），说明 DNS 服务器响应慢，或者本机的 `/etc/hosts` 中有错误的硬编码。 |
| **TCP Connect** | 客户端与服务器 IP 建立 TCP 三次握手成功的时间。 | 若耗时过长或超时失败，说明物理网络延迟极高，或者服务器有防火墙规则拦截了该端口，也可能服务器进程根本没有启动。 |
| **TLS Handshake** | *(仅 HTTPS/WSS)* 客户端与服务器完成 TLS 安全通道握手的时间。包含证书下载、双向验证和密钥协商。 | 若在此处报错，一般为 TLS 协议版本不匹配，或者根证书存储区缺少该证书的签发机构。如果握手耗时长，可能是服务器 CPU 忙或地理位置太远。 |
| **HTTP TTFB** | **Time To First Byte**（首字节时间）。从发送完请求报文到读取到服务器返回第一个字节数据的时间。 | 是衡量服务器响应性能的绝对指标。如果 TTFB 过高（如 > 1s），说明网络没有问题，而是服务器后端接口处理慢、数据库查询慢、或遇到了死锁和高负载。 |
| **Total Latency** | 从诊断开始到收到服务器首包响应的完整时延总和。 | 系统的端到端总时延表现。 |

---

## TLS 证书健康度诊断

在 HTTPS 握手期间，网络诊断会捕获服务器返回的 X.509 证书链（DER 裸字节），并通过 `x509-parser` 进行底层解构。

输出包括以下指标：
* **Subject (SAN)**：证书的主题替代名（Subject Alternative Names），表明该证书适用于哪些域名。
* **Issuer**：签发机构的 Distinguished Name（DN）。
* **Valid Until**：证书的过期时间（UTC）。
* **Status (状态分级警告)**：
  * [VALID]：证书有效，且距离过期天数大于 30 天。
  * [WARNING]：证书临期（距离过期天数 $\le$ 30 天）。提醒管理员必须尽快更换证书，避免服务突发中断。
  * [EXPIRED]：证书已过期（天数显示为负数），客户端发起 HTTPS 连接时会因安全校验失败而直接报错。

---

## WebSocket 升级握手诊断

对于 `ws://` 和 `wss://` 协议，RuPost 会模拟发送标准的 WebSocket 握手升级（Upgrade）请求头部：
```http
GET / HTTP/1.1
Host: <host>
Upgrade: websocket
Connection: Upgrade
Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==
Sec-WebSocket-Version: 13
```

诊断字段说明：
* [WS UPGRADE SUCCESS]：说明服务端已正确切换协议，可以建立长连接通道。
* [WS UPGRADE FAILED]：升级失败。
  * **排障建议**：一般是由于 Nginx / 反向代理网关未配置 `Upgrade` 和 `Connection` 头的透传。你需要检查 Nginx 配置文件：
    ```nginx
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    ```

---

## 诊断命令

```bash
# 诊断公网 HTTPS 接口
rupost diagnose https://httpbingo.org/get

# 诊断公网 WebSocket 接口
rupost d wss://echo.websocket.org

# 诊断本地开发接口
rupost d http://localhost:8080/api/v1/users
```

---

## 全局调试参数同步与场景联动 (CLI Global Flags)

除了在特定请求中声明 `# @diagnose` 注解外，RuPost 还在测试执行器（`rupost test`）中同步并集成了全局的命令行调试标志，以支持更灵活的按需连通性诊断。

### 1. 联动工作流与触发判定
在批量或单个用例运行中，诊断引擎将结合请求元数据与命令行标志，智能决定是否执行 `diagnose_url` 细粒度探测：
1. **声明式优先**：只要用例文件内部包含了 `# @diagnose` 注解，该请求在执行后**必定**进行网络诊断，以确保性能/证书断言提取成功。
2. **全局调试强制开启 (`--debug`)**：如果运行测试时指定了全局 `--debug` 标志，则对**所有测试步骤**（无论是否标注注解）都会强制追加网络诊断，并在终端打印时延瀑布图。
3. **失败自动补测 (`--debug-on-failure`)**：如果运行测试时指定了全局 `--debug-on-failure` 标志，在低压测试下不产生多余的诊断握手。**一旦且仅当**某个测试请求由于网络故障（例如超时、拒绝连接）或断言校验失败被判定为失败（`!success`）时，引擎才会异步补测，并将生成的诊断报告输出至控制台，或导出至结构化 JSON 报告中。

### 2. 核心使用场景
*   **场景 A：本地接口性能敏捷排查**
    在本地开发微服务接口时，直接使用 `--debug` 运行测试：
    ```bash
    rupost test path/to/apis.http --debug
    ```
    每一项测试都会输出网络分解瀑布图，快速识别 DNS 耗时、TCP 握手与首字节响应（TTFB）的开销比重。
*   **场景 B：CI/CD 契约构建卡点**
    通过 `# @diagnose` 元注解按需锁定重要核心路径，编写断言：
    ```http
    # @diagnose
    GET https://api.yourservice.com/health
    # @assert cert.days_remaining > 15
    ```
    一旦检测出网络延迟异常或 SSL 证书即将到期，CI 构建即自动阻断发布。
*   **场景 C：生产级高并发巡检（零网络打扰 + 故障精确恢复）**
    常规巡检期间，不加 `# @diagnose` 标签，仅配置 `--debug-on-failure` 和 `--report json`：
    ```bash
    rupost test suite.http --debug-on-failure --report json
    ```
    正常状态下接口以超高性能闪速跑完；一旦发生偶发性网络抖动或请求崩溃，会自动补测并生成诊断信息保存于 JSON 报告中，供监控报警与运维定位。
