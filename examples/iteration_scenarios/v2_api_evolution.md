---
title: 订单管理系统 API (V2 演进版)
version: 2.0.0
base_path: /api/v2
rules: |
  1. 所有 V2 版本的写操作 (POST) 必须校验 Header 中是否包含 `X-App-Version` 且其版本号 >= 2.0.0，否则拒绝请求。
  2. 订单状态扩充为: pending, paid, shipped, completed。
  3. 新增 payment_method (支付方式) 作为必填字段。
---

# 订单服务 V2 规范与 Mock 兼容性契约

本文件展示了系统向 V2 升级后的设计契约，同时保留了老版本 API 的兼容性 Mock。

## 兼容性设计：保留并升级 V1 查询接口
为确保老旧客户端在过渡期间不崩溃，系统继续提供 V1 接口的 Mock，并在响应中自动补齐并融入过渡字段 `"compat_mode": true` 和 `"payment_method": "legacy"`。

```http
@name get-order-v1-compat
GET /api/v1/orders/:id

@mock-when query.status == completed
HTTP/1.1 200 OK
Content-Type: application/json

{
  "order_id": "{{id}}",
  "status": "completed",
  "amount": 99.8,
  "payment_method": "legacy",
  "compat_mode": true
}

@mock-default
HTTP/1.1 404 Not Found
Content-Type: application/json

{
  "error": "Order {{id}} not found (legacy compat mode)"
}
```

## V2 新功能：创建订单
新增创建订单接口，必须包含 X-App-Version 安全校验。

```http
@name create-order-v2
POST /api/v2/orders
Content-Type: application/json

@mock-when header.X-App-Version == 2.0.0
HTTP/1.1 201 Created
Content-Type: application/json

{
  "order_id": "999",
  "status": "pending",
  "payment_method": "wechat",
  "x_app_validated": true
}

@mock-default
HTTP/1.1 400 Bad Request
Content-Type: application/json

{
  "error": "Bad Request: Missing or invalid X-App-Version. Upgrade required."
}
```
