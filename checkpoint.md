# Checkpoint - 2026-06-10

## 当前状态
- **完成生产调试模式阶段 1：本地局部环境变量级联覆盖与热重载**：
  1. 实现了高健壮度的 `.env` 解析器，自动剥离引号、支持行内注释与前导/尾随空格。
  2. 建立了级联优先级体系：`CLI命令行覆盖 (--var key=val)` > `系统环境变量` > `本地局部 .env 文件` > `共享 rupost.toml 环境配置`。
  3. 新增了多段单元测试与集成测试，验证了局部 `.env` 覆盖本地网关端口和网络请求闭环。
- **完成生产调试模式阶段 2：网络连通性分析与高亮诊断工具**：
  1. 在 `src/cli.rs` 中为子命令新增 `Diagnose` 变体（别名 `d`），并连接 `src/main.rs` 分发。
  2. 编写 `src/http/diagnose.rs`，基于第一性原理，利用 `lookup_host`，`TcpStream`，及纯 Rust 的 `tokio-rustls` (显式注入 `ring` 密码库) 进行手动握手与探测，精准分离 DNS 解析时延、TCP 握手时延、TLS 协商时延、HTTP TTFB 时延和总耗时。
  3. 引入 `x509-parser` 解析服务器证书的 Validity 期限，算出剩余有效天数，并对临期（<= 30天）的场景显示黄色警告 `[WARNING]`，已过期的场景显示红色 `[EXPIRED]`，同时解析并显示 Issuer 和 Subject SAN 域名信息。
  4. 采用色块条形图与 `colored` 库在终端格式化输出诊断瀑布流及证书元数据。
  5. 编写 `tests/diagnose_integration_test.rs` 集成测试，验证了本地 HTTP mock 服务与公网 HTTPS 接口的正确探测与时耗输出。
- **完成生产调试模式阶段 7：模糊路径匹配与条件变体独立 Mock 服务器**：
  1. 遵循 Clean Architecture 架构解耦，匹配决策作为 Core Domain 层实现，Axum 作为网络层适配器插件（Adapter），支持完美“可插拔”。
  2. 实现 Trie 树模糊匹配引擎，支持精确段、`:param` 参数捕获、以及 `*` 和 `**` 多段通配符匹配与回溯。
  3. 支持 Header、Query、Body JSONPath 的 Equals/Contains/Exists 多路分支条件匹配变体，并在命中后进行占位符变量动态插值替换。
  4. 新增命令行子命令 `rupost mock <file> --port <port>`（别名 `m`），通过 `#[serde(untagged)]` 智能反序列化兼容路由配置包与快照包。
  5. 命令行日志输出高雅大方，高亮实时反映流量到达和 Trie 路径匹配命中详情。
  6. 新增测试套件和单元测试，全量 142 个测试用例 100% 成功运行通过。
- **全量测试通过**：
  - 全项目 142 个测试用例 100% 成功运行通过，编译警告全部消除。

## 下一步
- 准备开启 Sprint 3 的“高级特性与脚本引擎”开发（包括 `@loop` 循环, `@skip-if` 条件运行以及前置/后置 Javascript 脚本等引擎集成）。
- 开启**阶段 3：HTTP 请求/响应基础快照录制与原样重放**。
  - 规划快照文件的序列化结构。
  - 实现用例执行时自动带入 `--save-snapshot` 参数，将 HTTP 头部和 Body 数据落盘存储。
  - 实现独立重放命令 `rupost history replay` 原样发送请求。
