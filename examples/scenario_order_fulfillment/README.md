# 场景 A：电商交易与履约服务 (Order & Fulfillment Scenario)

本场景作为 RuPost **API 驱动开发、Mock 联调与自动化测试**的端到端真实黄金用例。

---

## 包含文件清单

1. **`order_spec.md`（设计态 SSOT）**：
   - 包含 Frontmatter 规范（`title`、`version`、`host`、`rules`）。
   - **单块双角色主契约 (`POST /api/v1/orders`)**：
     - 测试用例标记 `@test`，断言状态码与响应体字段。
     - Mock 多条件分支：
       - `header.Idempotency-Key == repeat_tx_001` → 200 OK 幂等响应
       - `query.channel == vip` → 201 Created VIP 折扣响应
       - `header.Authorization == None` → 401 Unauthorized 拦截
       - `@mock-default` → 201 Created 标准创建响应
   - **纯契约定义 (`GET /api/v1/orders/detail`)**：
     - 无 `@test` 标记，用于验证 `rupost test` 模式下**自动跳过执行**，而在 `rupost mock` 模式下提供服务。

2. **`order_test_suite.http`（执行态测试工件）**：
   - 覆盖正常下单、幂等防重、VIP 打折、缺失 Token、详情查询、404 边界异常等用例。
   - 包含第三方支付网关调用，用于验证域名逃逸机制。

3. **`openapi_orders.json`（参考规范输入源）**：
   - 标准 OpenAPI 3.0.3 格式定义，为后续双向引导生成（`import openapi`）提供黄金对标基准。

---

## 联调与回归验证操作

### 1. 启动 Mock 契约服务
```bash
cargo run -- mock examples/scenario_order_fulfillment/order_spec.md --port 9000
```

### 2. 运行自动化测试套件
在另一终端运行：
```bash
cargo run -- test examples/scenario_order_fulfillment/order_test_suite.http
```

### 3. 验证设计态文档（验证纯契约跳过机制）
```bash
cargo run -- test examples/scenario_order_fulfillment/order_spec.md
```
*预期结果*：
- `create-order-standard` 作为 `@test` 请求执行通过；
- `get-order-detail` 无 `@test` 标记，自动进入 `skipped`，零发包。
