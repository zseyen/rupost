# 03 - 插件 SPI 体系与环境自愈降级策略

- **状态**: Approved
- **归属任务**: `cross-scenario-engine-and-plugins`
- **核心原则**: **“开发期全能扩展，调试期优雅降级”** —— 插件缺失绝不破坏基础网络请求与数据录制。

---

## 1. 异构环境冲突场景与降级核心矛盾

在真实研发旅程中经常发生以下断裂：
* 开发者在本地 Mac 上写了测试用例，声明了 `@plugin aws-sigv4`（依赖本地 Python 和 boto3）；
* 生产报警后，运维或开发把这套 `.http` 用例直接扔上了一台极简的 Linux 生产跳板机，使用 `rupost` 单二进制快速排查；
* **致命矛盾**：如果因为生产机器没有 Python 或没装 `rupost-plugin-aws-sigv4`，`rupost` 就直接 Panic 报错退出，会导致**整个现场排障直接瘫痪**！

---

## 2. 声明式降级策略与自愈机制

为了让测试用例具备跨环境的自愈能力，`rupost` 提供**两层降级防线**：

```
                              [ 执行带有 @plugin 的测试用例 ]
                                             │
                                             ▼
                             [ PluginRegistry 本地/PATH 探测 ]
                                             │
                      ┌──────────────────────┴──────────────────────┐
                      ▼                                             ▼
                【探测成功】                                   【插件未找到】
                      │                                             │
                      ▼                                             ▼
             [ 执行外部 Stdio IPC 管道 ]              ┌───────────────────────────┐
             计算动态签名，注入 Header                │ 检查降级标志:              │
                                                      │ 1. @plugin optional=true ?│
                                                      │ 2. CLI --allow-missing ?  │
                                                      └─────────────┬─────────────┘
                                                                    │
                                     ┌──────────────────────────────┴──────────────────────────────┐
                                     ▼                                                             ▼
                               【允许降级】                                                  【未声明降级 (默认)】
                                     │                                                             │
                                     ▼                                                             ▼
             [ 记录 Warning，使用预置兜底 Header 发包 ]                                   [ 友好报错退出，列出插件搜寻路径 ]
             基础功能照常发包，录制完整原始现场至 dump.json
```

### 2.1 用例级声明降级：`@plugin <NAME> [optional=true]`
```http
### 获取受保护的订单详情
POST https://api.my-cloud.com/v1/orders
# 开发环境自动调用插件签名；调试环境若无插件，自动跳过插件并不阻断测试
@plugin aws-sigv4 region=us-east-1 optional=true

# 配合静态兜底 Header（现场调试时直接使用临时的运维 Token）
Authorization: Bearer {{DEBUG_FALLBACK_TOKEN}}
Content-Type: application/json

{"order_id": 123}
```

### 2.2 CLI 级全局降级开关：`--allow-missing-plugins`
在现场排障或即拷即走场景中，用户无需修改已有的 `.http` 文件，直接传参：
```bash
rupost test production_suite.http --allow-missing-plugins --save-snapshot /tmp/live-dump.json
```
* **行为**：
  * 所有找不到对应二进制的插件调用均被静默降级为“跳过”；
  * 向最终的测试报告与产物中追加明确警告：
    `[Warning] 插件 'aws-sigv4' 未安装，已降级跳过（现场数据已记录至快照）`；
  * 基础请求正常外发，完整的网络时序与响应内容被完整录入快照，保证现场排查任务能够顺利完成。

---

## 3. 插件通信协议规范 (Stdio IPC Protocol)

外部插件与微内核通信遵循无死锁、强类型的 JSON 管道规范：

### 3.1 运行时 Signer 协议
* **输入 (`stdin`)**:
  ```json
  {
    "protocol_version": "1.0",
    "action": "sign",
    "method": "POST",
    "url": "https://api.my-cloud.com/v1/orders",
    "headers": [["Content-Type", "application/json"]],
    "body": "{\"order_id\": 123}",
    "plugin_args": {"region": "us-east-1"}
  }
  ```
* **输出 (`stdout`)**:
  ```json
  {
    "success": true,
    "extra_headers": [["Authorization", "AWS4-HMAC-SHA256 ..."]],
    "mutated_body": null,
    "warnings": []
  }
  ```

### 3.2 熔断与隔离守护
* **超时硬熔断**：每个插件调用设置默认 5 秒超时，超时后利用 `tokio::process::Command::kill_on_drop(true)` 强制杀死子进程，绝不挂死主测试流；
* **时延隔离**：插件子进程的启动耗时独立统计为 `plugin_duration_ms`，**绝不混入 HTTP 请求的 `duration` 和 `ttfb`**。
