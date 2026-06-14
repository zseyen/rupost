# RuPost 用户使用手册 (User Guide)

RuPost 是一个基于 Rust 构建的现代化、极简且富有终端美学的 HTTP 客户端与 API 测试工具。本文档为您详细讲解如何使用 RuPost 的核心特性，在日常开发与持续集成中高效进行接口测试与网络诊断。

---

## 1. 变量与环境管理 (Variables & Environments)

RuPost 设计了一套极简但高抗冲突的变量管理体系，支持静态配置与运行时动态覆盖。

### 1.1 引用与动态捕获
*   **引用变量**：在 `.http` 或 `.md` 测试用例中，使用 `{{var_name}}` 来引用已定义的变量。
*   **动态捕获 (`@capture`)**：可以在 HTTP 响应体或响应头中动态捕获数据并写入当前上下文，例如：
    ```http
    POST /auth/login
    Content-Type: application/json
    
    { "username": "admin" }
    
    @capture auth_token = body.token
    @capture session_id = headers.set-cookie.0
    ```

### 1.2 四层级变量优先级控制 (Cascading Priority)
当同一个变量在多个地方被定义时，RuPost 严格遵循以下级联优先级判定（从高到低）：

1.  **最高优先级：命令行参数覆盖 (`--var key=value`)**
    例如：`rupost t login.http --var base_url=http://127.0.0.1:8080`
2.  **第二优先级：当前进程的系统环境变量**
    *   *第一性原理隔离设计*：RuPost 在加载系统环境变量时，**仅限覆盖已存在的同名变量**，不会将系统中不相关的环境变量注入至用例上下文，防范全局命名空间污染。
3.  **第三优先级：本地局部环境变量文件 (`.env` 或 `.env.<env_name>`)**
    *   在本地联调时，可在项目根目录下创建 `.env` 文件。
    *   如果指定了 `-e prod`，RuPost 将自动寻找并合并 `.env.prod` 文件。
    *   可使用 `--env-file <path>` 显式指定加载某个局部变量文件。
4.  **最低优先级：共享 `rupost.toml` 配置文件**
    *   团队共享的环境变量配置文件，通过 `[environments.<env_name>]` 组织。

---

## 2. 多文件批量测试与拓扑依赖 (DAG Batch Testing)

RuPost 支持对整个目录进行递归扫描并根据声明式依赖自动编排执行顺序。

### 2.1 递归扫描与自动过滤
当输入路径为文件夹时，RuPost 自动递归扫描收集所有的 `.http` 与 `.md` 文件，同时主动过滤以下无关或隐藏目录：
*   `.git/`、`.rupost/`、`target/`、`node_modules/`

### 2.2 声明式跨文件依赖 (`### @depends-on`)
用例可以通过在文件头部添加 `### @depends-on <filename>` 声明前置依赖关系。例如 `profile.http` 依赖于 `login.http`：
```http
### @depends-on login.http
GET {{base_url}}/user/profile
Authorization: Bearer {{auth_token}}
```
RuPost 会构建**有向无环图 (DAG)**，利用 **Kahn 拓扑排序算法** 自动计算最优执行顺序。如果检测到循环依赖（如 A -> B -> A），测试将安全报错退出。

### 2.3 安全沙箱加载防护 (Sandbox Scope Jail)
在递归自动补全依赖文件时，RuPost 会校验物理路径，强制确保所有依赖文件**必须位于当前工作目录 (CWD) 或用户执行目录的子集内**。非本沙箱范围的文件或非支持测试扩展名（`.http`/`.md`）的文件一律拒绝加载，彻底防范路径穿越 (Path Traversal) 漏洞。

---

## 3. 顺序与并行模式下的状态单向克隆 (State Cloning)

批量测试支持双执行模式，利用 `State Cloning` 机制解决了高并发下“状态级联”与“数据孤岛”的矛盾。

### 3.1 双执行模式
*   **顺序模式 (Sequential)**：默认模式。用例按拓扑序串行执行，变量与 Cookie 状态自然级联传递。
*   **并行模式 (Parallel)**：通过 `--mode parallel` 启用。使用 `tokio::task::JoinSet` 与并发限制器（默认 `--concurrency 4`）异步调度无依赖关系的分支。

### 3.2 状态单向克隆机制 (State Cloning)
在并行执行时，为了保证有依赖关系的用例节点（例如 `profile.http` 依赖前置的 `login.http`）能够获取前置鉴权，同时又不引发并发读写冲突，RuPost 实现了状态单向克隆机制：
1.  **依赖传递**：当父节点执行完毕后，其生成的局部变量增量与序列化后的 Cookie 状态，将**单向克隆（Clone & Merged）**传递给子节点上下文。
2.  **并发安全**：不存在拓扑依赖关系的分支依然保持完全的隔离保护，每个分支在各自的沙箱环境中运行，杜绝多线程下的资源竞争与数据交叉污染。

---

## 4. 网络诊断工具 (Network Diagnostics)

网络诊断子命令 `diagnose` (简写 `d`) 独立于测试引擎，用于快速排查网络和证书故障。

### 4.1 使用命令
```bash
rupost diagnose https://example.com
```

### 4.2 诊断核心指标与时延瀑布图
基于第一性原理，RuPost 使用底层网络组件进行手动握手，将整体时延精确拆分为瀑布条形图：
*   **DNS Lookup**：域名解析耗时（蓝色占比条）。
*   **TCP Connect**：TCP 三次握手建立连接耗时（绿色占比条）。
*   **TLS Handshake**：HTTPS 安全握手协商耗时（紫色占比条，仅 HTTPS）。
*   **HTTP TTFB (Time to First Byte)**：发送请求到接收到响应第一个字节的服务器首字节时延（黄色占比条）。
*   **Total Latency**：完成本次 HTTP 诊断的总耗时。

### 4.3 X.509 证书深度分析
对于 HTTPS 链接，RuPost 解析服务器端下发的原始 DER 字节，解构 X.509 证书属性：
*   **主体名称 (Subject SAN)**
*   **颁发者 (Issuer)**
*   **有效期截止 (Valid Until)**
*   **到期状态高亮警示**：
    *   `剩余天数 <= 0`：终端红色高亮警告 `[EXPIRED]`。
    *   `剩余天数 <= 30`：终端黄色高亮警告 `[WARNING]`。
    *   `剩余天数 > 30`：终端绿色高亮提示 `[VALID]`。

---

## 5. 本地 Mock 服务器 (Mock Server)

RuPost 提供了一个开箱即用、完全可插拔的轻量级 Mock 服务器。

### 5.1 启动 Mock 服务
```bash
rupost mock examples/mock_config.json --port 9000
```
支持读入两类格式配置：
1.  **JSON 规则定义**：自定义的匹配路径与变体组。
2.  **历史请求快照**：RuPost 历史记录生成的快照文件，自动完成 Mock 规则映射。

### 5.2 Trie 树路径模糊匹配
Mock 路由调度核心采用 Trie 匹配树，支持以下路由格式：
*   **精确匹配**：`/api/v1/login`
*   **路径参数提取**：`/users/:id`（解析出的参数如 `id` 会自动写入 Context，供响应体动态模板渲染）
*   **单级通配符**：`/files/*`（匹配 `/files/a`，但不匹配 `/files/a/b`）
*   **多级通配符**：`/static/**`（匹配 `/static/js/app.js`）

### 5.3 多路分支条件匹配 (MockVariant)
支持针对同一路由定义多个响应变体（`MockVariant`），按照**有条件变体优先、无条件兜底**的顺序，依次对请求参数进行评估：
*   **评估源**：`Header`、`Query` (URL 查询参数)、`Body` (支持 Body JSONPath 提取)。
*   **操作符**：
    *   `Equals`：完全匹配。
    *   `Contains`：包含特定子串（若提取值为 JSON 数组，则判断数组是否包含该子项）。
    *   `Exists`：判断该字段是否存在（特殊：匹配值为 `"None"` 表示不存在该标头/查询参数/JSON 字段）。
*   **JSONPath 支持**：可处理复杂的 JSON 嵌套数据，如 `$.user.profile.age` 或带有数组下标的 `$.tags[0]`。
