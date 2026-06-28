# 本地 Mock 服务器 (Mock Server)

RuPost 提供了一个独立且高可扩展的轻量级本地 Mock 服务器，让您能够依据接口定义或请求历史快照一键搭建本地 Mock 桩。

---

## 1. 快速启动

你可以读入一个定义好的 Mock JSON 配置文件，或者直接编译 Markdown API 设计文档：
```bash
# 启动基于 JSON 规则定义的 Mock 服务
rupost mock examples/mock_config.json --port 9000

# 编译并热加载 Markdown 设计文档中的 Mock 示例
rupost m doc/plans/2026-06-06-mock-server-design.md
```

---

## 2. 核心特性与架构

### A. 双模数据输入自适应 (Untagged Deserialize)
Mock 引擎支持以下两类定义格式：
1. **条件匹配规则集 (Routes)**：显式定义 HTTP 方法、模糊路径、变体条件以及响应负载。
2. **历史请求快照 (Snapshots)**：直接读入由 RuPost 执行测试后导出的历史快照 JSON 包，引擎会自适应将其转译为默认的 Mock 匹配桩，实现“测试即录制，录制即 Mock”。

### B. Trie 树模糊路径匹配与路径参数捕获
Mock 引擎在底层自主实现了 Trie 树路由解析算法：
* 支持精确路径匹配（如 `/v1/users`）。
* 支持带参数的动态路径匹配（如 `/v1/users/:id`），引擎会自动提取 `:id` 值并写入当前 Mock 上下文中，以供后续响应体渲染。
* 支持通配符匹配（`*` 与 `**`）。

### C. 多路分支条件匹配 (`MockVariant`)
同一条路由（Route）可以挂载多个响应变体（Variant），引擎将按优先级依次匹配：
* **Header 条件**：匹配请求头中的 Key-Value 关系。
* **Query 条件**：匹配请求 URL 中的参数。
* **Body 匹配**：利用统一的 JSONPath 评估引擎（如匹配 `$.user.role == "admin"`），支持 `Equals`、`Contains` 以及 `Exists` 节点存在性判定。

每个匹配成功的变体支持自定义返回状态码（Status Code）、自定义响应头、以及对动态上下文变量的实时渲染替换（例如将捕获的路径参数 `:id` 自动渲染到响应的 JSON 体中）。

### D. 控制台高亮彩色访问日志
Mock 服务启动后，会在控制台以高雅彩色的日志输出每一次入站请求的详细链路信息（匹配的路径、提取的参数、最终命中的 Mock 变体及返回的包体大小），便于联调。
