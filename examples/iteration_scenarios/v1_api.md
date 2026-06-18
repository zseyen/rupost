---
title: 订单管理系统 API (V1)
version: 1.0.0
base_path: /api/v1
rules: |
  1. V1 阶段订单状态仅支持 pending (待处理) 和 completed (已完成)。
  2. 查询订单时，路径参数 id 必须为数字。
---

# 订单服务 V1 规范与 Mock 契约

本文件定义了 V1 版本的订单核心契约。

## 查询订单详情
根据订单 ID 获取订单信息。支持通过 query.status 触发不同的 mock 状态。

```http
@name get-order-v1
GET /api/v1/orders/:id

@mock-when query.status == completed
HTTP/1.1 200 OK
Content-Type: application/json

{
  "order_id": "{{id}}",
  "status": "completed",
  "amount": 99.8,
  "created_at": "2026-06-01T12:00:00Z"
}

@mock-when query.status == pending
HTTP/1.1 200 OK
Content-Type: application/json

{
  "order_id": "{{id}}",
  "status": "pending",
  "amount": 99.8,
  "created_at": "2026-06-01T12:00:00Z"
}

@mock-default
HTTP/1.1 404 Not Found
Content-Type: application/json

{
  "error": "Order {{id}} not found"
}
```

## @test 验证 V1 基础查询
此代码块作为 V1 阶段的自动化集成测试，由测试运行器执行，不作为 Mock 变体。

```http
@name test-v1-completed-flow
@test
GET http://localhost:9000/api/v1/orders/101?status=completed
@assert status == 200
@assert body.status == completed
@assert body.amount == 99.8
```

---

## 📖 使用说明与场景运行

Rupost 提供了**声明式契约驱动设计与测试**能力。本文件（`v1_api.md`）既是 API 交互的规范定义，也是可直接拉起的 Mock 服务和执行集成测试的唯一事实来源。

### 1. 启动契约的 Mock 服务
您可以通过 `rupost mock` 命令拉起此文件所定义接口的本地 Mock 服务器：
```bash
# 启动 V1 版本的 Mock 服务，默认监听 9000 端口
rupost mock examples/iteration_scenarios/v1_api.md --port 9000
```

### 2. 向 Mock 服务发送联调请求
打开另一个终端，使用 `curl` 验证多分支状态匹配：

- **分支 1（命中 `query.status == completed`）**：
  ```bash
  curl -i "http://localhost:9000/api/v1/orders/101?status=completed"
  # 返回 200 OK，包含 "status": "completed" 并自动渲染 order_id: 101
  ```

- **分支 2（命中 `query.status == pending`）**：
  ```bash
  curl -i "http://localhost:9000/api/v1/orders/101?status=pending"
  # 返回 200 OK，包含 "status": "pending"
  ```

- **兜底分支（未匹配条件时触发 `@mock-default`）**：
  ```bash
  curl -i "http://localhost:9000/api/v1/orders/101?status=cancelled"
  # 返回 404 Not Found
  ```

### 3. 一键执行自动化回归测试
您可以使用 `rupost test` 执行此文件中用 `@test` 声明的自动化验证用例（确保 9000 端口 Mock 服务器正在后台运行）：
```bash
rupost test examples/iteration_scenarios/v1_api.md
```
系统将会读取并执行 `test-v1-completed-flow` 测试用例，输出详细的断言验证报告。
