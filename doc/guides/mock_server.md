# 本地 Mock 服务器 (Mock Server)

RuPost 提供独立、可扩展的本地 Mock 引擎，支持通过 **CLI**、**TUI（`M` 键）** 与 **Web 工作台（Mock Studio）** 三种入口启动、热重载与实时观测。

---

## 1. 快速启动

### CLI 启动
```bash
# 启动基于 JSON 规则定义的 Mock 服务（默认端口 9000，默认开启智能 CORS）
rupost mock examples/debug_demo/mock_backend.json --port 9000

# 编译并加载 .http / .md 活文档中的 @mock 契约块（支持别名 m）
rupost m examples/design_scenarios/abstract_base_url_api.md -p 9000

# 关闭智能 CORS（当需要显式 Mock OPTIONS 预检请求时）
rupost mock routes.json --no-cors
```

### TUI 快捷启停与实时流量
1. 在 TUI 编辑器中打开并**保存**包含 `@mock-when` / `@mock-default` 或 Mock JSON 的文件。
2. 按 **`M`**（`Shift+M`）在随机空闲端口拉起/停止 Mock 服务；状态栏显示 `MOCK :<port>` 与最新命中状态，`Response Viewer` 实时展示入站流量。

### Web 工作台（Mock Studio）
启动 `rupost web` 后可使用以下 API：
- `GET /api/v1/mock/routes`：扫描工作区全部 `@mock` 路由及变体数量（`variant_count`）。
- `POST /api/v1/mock/simulate`：基于 `TrieRouteMatcher` 仿真匹配路径、捕获路径参数（`captured_params`）并预览渲染后的响应头与响应体。
- `POST /api/v1/mock/start` / `POST /api/v1/mock/reload` / `POST /api/v1/mock/stop`：托管启动、**原地热重载（端口不变，编译失败保留旧路由）**与停止。
- `GET /api/v1/mock/traffic?id=<id>`：查询托管 Mock 的实时入站流量与请求计数（`request_count`）。

---

## 2. 活文档语法示例（`.http` / `.md`）

在请求块中以空行分隔请求与变体区后，使用 `@mock-when` 与 `@mock-default` 声明多路响应（兼容 `//` 与 `#` 注释前缀）：

```http
GET {{base_url}}/api/users/:id?debug=1
Accept: application/json

@mock-when header.X-Role == admin
HTTP/1.1 200 OK
Content-Type: application/json

{"id": "{{id}}", "role": "admin"}

@mock-when header.Authorization == None
HTTP/1.1 401 Unauthorized

{"error": "missing auth"}

@mock-default
HTTP/1.1 403 Forbidden
Content-Type: application/json

{"error": "forbidden"}
```

> **提示**：
> - `@mock-when` 表达式语法支持 `header.<Key> == <Val>`、`query.<Key> == <Val>`、`body.<JSONPath> == <Val>`；右侧为 `None`（不区分大小写）时匹配字段缺失或 `null`。
> - 当响应头包含 `Content-Type: text/event-stream` 时，Mock 服务自动按 SSE 空行事件帧（`\n\n`）逐帧流式推送。

---

## 3. 核心特性与架构

1. **三模输入自适应**：支持活文档（`.http` / `.md` / `.markdown`）、Routes JSON 与历史快照 Snapshots JSON；历史快照自动按 `(Method, Path)` 聚合变体。
2. **Trie 树路由与变体合并**：支持字面量、`:id` / `{id}` 参数捕获（自动渲染 `{{id}}`）、`*` 单段通配与 `**` 多段通配；同一 `(Method, Path)` 在多块/多文件重复声明时自动合并变体，且 `@mock-default` 永远沉底兜底。
3. **单次 JSON 解析与严格编译**：请求体仅惰性反序列化一次（`EvalContext`）；非法 `@mock-when` 表达式在启动/重载/仿真时显式报错，绝不静默退化为兜底变体。
4. **智能 CORS 中间件**：默认处理 `OPTIONS` 204 预检、动态回显 `Origin` + `Access-Control-Allow-Credentials: true` 及 `Access-Control-Request-Headers`。
