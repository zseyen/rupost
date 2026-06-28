# 网络诊断工具 (Network Diagnostics)

为了帮助开发者在生产环境或本地开发时快速定位网络、TLS 握手及证书配置问题，RuPost 提供了一键式的网络诊断工具 `diagnose` (别名 `d`)。

---

## 1. 快速使用

通过 `diagnose` 命令行发起对特定 HTTP/HTTPS 或 WS/WSS URL 的全连通性分析：
```bash
rupost diagnose https://httpbin.org/get
# 或者使用简短的别名
rupost d https://api.example.com
```

---

## 2. 核心特性与诊断细节

### A. 细粒度时延瀑布图 (Latency Waterfall)
基于第一性原理，诊断模块通过手动调用 `lookup_host` 解析 IP，创建底层 `TcpStream` 进行物理 TCP 握手，并使用 `tokio-rustls` 执行 TLS 握手，从而精确测量并输出：
* **DNS 解析时延 (DNS Lookup)**：域名解析为 IP 的耗时。
* **TCP 握手时延 (TCP Connect)**：完成 TCP 三次握手建连的耗时。
* **TLS 握手时延 (TLS Handshake)**：进行 TLS 证书协商与握手加密的耗时（如果是 HTTPS/WSS 协议）。
* **首字节响应时延 (TTFB)**：自发送 HTTP/WS 请求头部后，收到服务器返回首个响应字节的耗时。

在终端中，RuPost 会将这些耗时以彩色的瀑布占比条形图形式直观绘制，便于快速定位慢网络节点。

### B. X.509 证书健康与安全分析
诊断模块会自动拦截 TLS 握手握持的证书链，利用 `x509-parser` 深度提取和分析证书的属性：
* **主体备用名称 (Subject Alternative Name, SAN)**：该证书匹配的域名范围。
* **颁发者信息 (Issuer)**：证书的 CA 颁发机构。
* **有效期期限 (Validity)**：证书的起止时间。
* **临期/过期预警 (Warning & Expiry)**：
  - 如果证书的剩余有效天数大于 30 天，显示正常的绿色通知。
  - 如果剩余天数小于等于 30 天，标记为黄色警告 `[WARNING]`。
  - 如果证书已过期，则显示红色的 `[EXPIRED]`。

### C. 网络层完全隔离
此诊断模块位于低级基础设施层（`src/http/diagnose.rs`），为完全独立的子命令，不与 `Reqwest` 的高层 HTTP/WSS 连接池混淆，能真实反映物理网络通道的最纯净表现。
