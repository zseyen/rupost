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
