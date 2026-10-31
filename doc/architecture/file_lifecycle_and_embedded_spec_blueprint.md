# RuPost 文件生命周期契约校对与内联嵌入蓝图 (Embedded Contract Spec)

本设计方案旨在解决 RuPost API 研发过程中**“开发环境多物理文件”**与**“测试/生产现场纯 HTTP 用例”**之间的生命周期演变矛盾。同时，针对 `.http` 文件通用性、第三方外网 API 校验、Mock 可插拔生成机制、CI/CD 物理过滤分发、逻辑联动调试以及 **SSE（流式推送）与 WebSocket（双向长连接）契约比对**进行专项设计。

---

## 🗺️ 1. 背景与文件生命周期演变

### 1.1 生命周期阶段与文件物理处理方式
结合实际部署场景，RuPost 的 API 生命周期演变需要解决**设计契约与执行用例的物理隔离与 Git 资产管理**：

```
 [开发环境: Git 集中管理]
   ├── doc/specs/                    <-- 存放设计契约（例如 order_spec.md，包含 HTTP/SSE/WS 路由与 Schema）
   └── tests/integration/            <-- 存放测试脚本（例如 order_test.http，含 @extends-spec 引用）
            │
            ▼ (CI/CD 自动化构建: 运行 rupost export --all 编译)
 [CI 构建/打包阶段]
   ├── 1. 扫描 tests/integration/ 下的所有 .http 文件
   ├── 2. 解析继承相对路径，读取 doc/specs/ 下的契约设计
   ├── 3. 将编译后的契约快照（# @spec-）无损回写至 .http 顶部
   └── 4. 物理裁剪 (Spec Stripping):
          * 交付包中仅包含 tests/integration/ 目录的 .http 文件
          * 显式过滤/排除整个 doc/specs/ 目录 (防止核心设计资产外泄)
            │
            ▼ (将仅含 .http 的交付包部署到现场执行环境)
 [测试/生产执行现场]
   ├── 物理上只有 tests/integration/order_test.http
   └── 运行 rupost test：由于 spec.md 缺失，引擎自动降级解析 .http 顶部的内联契约，闭环完成校验
```

### 1.2 物理隔离打包实践指南
在 CI/CD 打包流水线中，应在 `.dockerignore` 或 Zip 归档脚本中进行如下配置，以实现生产现场的文件裁剪：
```bash
# 1. 执行全局编译导出，把契约刷入 .http 中
rupost export doc/specs/ -o tests/integration/ --all-in-place

# 2. 打包分发时，排除规格说明书，仅保留用例
# 在部署脚本中只拷贝 tests/ 目录，忽略 doc/ 目录
tar --exclude='doc/specs' -czf rupost-delivery.tar.gz tests/ rupost.toml
```

---

## 📝 2. 内联嵌入契约 (Embedded Spec) 语法规范与通用性保障

为了**保障 `.http` 文件的通用性**（能在 VSCode/JetBrains 的 HTTP 客户端无阻碍运行），我们彻底摒弃了特定边界符，采用普通的单行注释：

```http
# @spec-version = 1.2.0
# @spec-base-path = /api/v1
# @spec-host = api.orderservice.local
# @spec-route = POST /orders (schema: {"id": "string", "item_count": "number", "status": "string"})
# @spec-route = GET /orders/legacy/:id (deprecated: "Use GET /api/v1/orders/:id instead", schema: {"id": "string"})
# @spec-route = GET /orders/stream (sse: true, events: {"order_created": {"id": "string", "amount": "number"}, "order_paid": {"id": "string", "pay_time": "string"}})
# @spec-route = GET /orders/ws (websocket: true, client_send: {"subscribe": {"topic": "string"}}, server_push: {"order_update": {"id": "string", "price": "number"}, "error": {"code": "number"}})

# =========================================================================
# 以下为集成测试用例，可被通用客户端正常解析运行
@base_url = http://localhost:9000
@auth_token = dev_admin_token_xyz

### 用例 1: 创建普通订单
POST {{base_url}}/api/v1/orders
Authorization: Bearer {{auth_token}}
Content-Type: application/json

{
  "item_count": 5,
  "amount": 15000
}

@assert status == 201
```

---

## ⚙️ 3. 核心机制 1：无感就地覆写更新 (In-place Compile-on-Export)

运行 `rupost export doc/specs/order_spec.md -o tests/integration/order_test.http` 时，若目标文件已存在，执行**无感就地覆写**：
1.  **扫描与过滤**：遍历 `order_test.http` 的每一行，过滤/剔除掉所有以 `# @spec-` 或 `#@spec-` 开头的行。
2.  **重新写入**：在过滤后的文本最顶部插上由 `order_spec.md` 重新编译生成的内联契约注释块。
3.  **继承路径转换**：在导出时，原 `### @extends-spec ../../doc/specs/order_spec.md` 注释可以被自动重写为 `### @extends-spec order_spec.md`，使得现场机器在找不到原相对路径时，能够感知到这只是一个快照校验器。

---

## 🏛️ 4. 核心机制 2：运行时契约查找与分场景校验 (Runtime Lookup & Escaping)

在运行 `rupost test <http_file>` 时，校验器必须按照以下优先级确定契约比对的“单事实来源 (SSOT)”：
1.  **优先本地物理路径**：尝试根据 `extends_spec_path` 寻找本地物理 `.md` 规范。如果找到，以 `.md` 物理文件为 SSOT。
2.  **优雅退化 (Graceful Degradation)**：若本地找不到对应的 `.md`，校验器自动解析当前运行的 `.http` 文件顶部以 `# @spec-` 开头的注释行，还原出契约树。
3.  若两者均缺失，则仅作为普通集成测试运行，免去契约一致性校验。

---

## 🔬 5. 被忽略的点与关键边界细节 (Critical Ignored Points)

### 🔑 5.1 场景讨论：请求第三方 URL 的智能逃逸机制与联动逻辑调试

#### A. 第三方 URL 逃逸匹配示例
假设本地测试服务基准主机为 `http://localhost:9000` (由 `@spec-host` 显式定义为 `api.orderservice.local` 或 `localhost`)：
*   **本地请求**：`POST http://localhost:9000/api/v1/orders` -> Host 匹配，必须严格进行契约和 Schema 超集校验。
*   **第三方外网请求**：`POST https://api.mch.weixin.qq.com/v3/pay/transactions/jsapi` -> 请求 Host 为三方域名，且契约中未声明，**系统自动触发智能逃逸**，直接放行，不引发“未定义路由契约错误”。

#### B. 逻辑调试场景：数据初始化、状态清理与接口联动 (Workflow Chaining)
在现场调试和回归测试中，接口通常具有状态依赖关系，需要“逻辑调试”：
1.  **依赖数据初始化 (Setup)**：
    *   通过 `### @depends-on db_init.http` 指令声明，保证拓扑排序在当前文件运行前，执行数据初始化（如用户注册、清零账户余额等）。
2.  **接口联动与变量捕获 (Chaining & Capturing)**：
    *   调试时，需要将用例 1 创建订单返回的 `id` 传递给用例 2 进行订单查询：
        ```http
        ### 用例 1: 创建订单并捕获 ID
        POST {{base_url}}/api/v1/orders
        
        @capture order_id from body.id
        
        ### 用例 2: 使用捕获的变量查询订单
        GET {{base_url}}/api/v1/orders/{{order_id}}
        
        @assert status == 200
        ```
    *   在有向图执行和本地调试重试时，RuPost 运行时引擎必须确保变量在同一个文件执行域的上下文（`VariableContext`）中进行持久化流转，以支持完整的接口逻辑链条。

---

### 🔑 5.2 SSE (Server-Sent Events) 与 WebSocket 契约校验规范

针对流式推送和双向长连接，契约校验器在设计态及内联快照态必须遵守如下检验规则：

#### A. Server-Sent Events (SSE) 流帧校验机制
由于 SSE 通信是流式且异步单向的，同一个端点在不同时间会推送不同事件类型（`event`）的帧，因此不能用单一的 Response JSON Schema 校验。
*   **契约快照语法**：
    `# @spec-route = <METHOD> <path> (sse: true, events: {"<event_name_1>": <schema_1>, "<event_name_2>": <schema_2>})`
*   **运行时拦截与匹配校验算法**：
    1.  当用例执行命中声明有 `sse: true` 的路由时，引擎的流解析器（`SseParser`）对返回的事件流进行拦截。
    2.  对接收到的每一帧，读取其 `event: <name>` 和 `data: <json>`。
    3.  **分流路由比对**：根据 `event` 的名称路由至该路由契约的 `events` 字典中：
        *   若契约未定义该事件的 Schema（或者推送了未知事件且非逃逸），引擎高亮警告或置为失败。
        *   若命中对应的事件 Schema，对该帧的 `data` 部分（反序列化为 JSON）进行**弱 Schema 超集递归校验**，检测缺失字段或类型不符。

#### B. WebSocket 双向通道契约校验机制
WebSocket 是双向长连接，且通常使用单一端点升级握手。在此连接上发生的数据交换同样需要受到客户端发送（C2S）与服务端推送（S2C）的双向严格审计。
*   **约定机制 (Message Dispatcher)**：双向 JSON 通信帧必须包含一个类型标志字段（缺省键为 `"type"`，例如 `{"type": "subscribe", "topic": "orders"}`）。
*   **契约快照语法**：
    `# @spec-route = <METHOD> <path> (websocket: true, client_send: {"<msg_type_1>": <schema_1>}, server_push: {"<msg_type_2>": <schema_2>})`
*   **运行时拦截与校验算法**：
    1.  **发送审计（C2S）**：当测试用例执行 `@send` 指令发出数据包时，引擎在发出前对 Payload 拦截，提取并匹配 `type` 字段，去契约的 `client_send` 中查找对应的 Schema。如果发送的数据结构不合规，引擎立刻引发本地测试中断并报错，防止向外发送畸形协议数据。
    2.  **接收审计（S2C）**：当 WebSocket 接收到服务器推送的文本帧时，读取 `type` 并去 `server_push` 字典中路由匹配对应的 Schema，对推送的 JSON 进行弱 Schema 超集校验，若报错则实时记录到测试报告的流异常断言中。

---

### 🔑 5.3 可插拔式 Mock 数据自动生成插件接口 (Pluggable Mock Generator)
*   *设计考量*：当进行联调且接口未手写响应变体时，我们需要基于 Schema 自动合成数据。为了支持未来更强大的动态生成算法（如 WASM 发生器插件或 AI 模型），本系统应将 Mock 生成逻辑抽离为**可插拔式 trait**，供核心引擎调用。

```rust
use serde_json::Value;

/// 契约生成插件异常类型
#[derive(Debug, thiserror::Error)]
pub enum GeneratorError {
    #[error("Failed to generate data from schema: {0}")]
    GenerationFailed(String),
}

/// 预留的可插拔 Mock 数据发生器接口
pub trait MockGeneratorPlugin: Send + Sync {
    /// 插件的唯一标识名 (如 "schema-default", "ai-generator", "faker-plugin")
    fn name(&self) -> &str;

    /// 依据弱 Schema AST 生成符合契约规范的 Mock 响应 JSON Body
    fn generate_mock_body(&self, schema: &Value) -> Result<Value, GeneratorError>;
}
```
引擎在需要对零 Body 接口合成数据时，会根据 `rupost.toml` 中的配置，路由并加载实现了该 Trait 的生成插件（默认装载基于 Schema 字段类型推断的零依赖发生器）。

---

### 🔑 5.4 展望：图形化契约设计器 (TUI/GUI Spec Designer)
*   *后续扩展*：在 `rupost` 的架构模型中，为未来扩展图形化/交互式设计器预留端口。
    *   **TUI 交互式生成器**：未来将在 TUI 中加入表单输入模式，Alex 只需在终端填写 API Path、Method 和 Schema，系统便能自动格式化并输出标准的 `.md` 契约文档。
    *   **VSCode GUI 插件**：RuPost 的 AST 暴露为通用的 JSON schema 结构，未来可基于 VSCode Extension 提供图形化表单填报界面。
