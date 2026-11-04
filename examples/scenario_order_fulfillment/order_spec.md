---
title: 电商交易与履约服务规格书 (Order Fulfillment API Contract)
version: 1.0.0
host: api.orderservice.local
rules: |
  1. 所有写操作必须携带有效的 Bearer Token。
  2. 订单创建接口支持基于 Idempotency-Key 的幂等防重机制。
  3. 金额单位统一为分 (amount: integer)。
---

# 订单履约服务接口规格 (SSOT)

本文件作为订单中心开发、前端联调、自动化测试与契约审计的**单一事实来源 (SSOT)**。

---

## 1. 创建订单接口 (Create Order)

`POST /api/v1/orders` 负责接收下单请求并生成履约单据。
该接口作为**单块双角色**契约：既是 Mock 服务的分支路由，又是自动化冒烟测试用例。

```http
@test
@name create-order-standard
POST /api/v1/orders
Authorization: Bearer {{auth_token}}
Content-Type: application/json

{
  "item_count": 2,
  "amount": 29900,
  "currency": "CNY"
}

@assert status == 201
@assert body.id exists

@mock-when header.Idempotency-Key == repeat_tx_001
HTTP/1.1 200 OK
Content-Type: application/json

{
  "id": "ord_existing_8888",
  "status": "processed",
  "amount": 29900,
  "duplicated": true,
  "message": "Idempotent response: order already created."
}

@mock-when query.channel == vip
HTTP/1.1 201 Created
Content-Type: application/json

{
  "id": "ord_vip_9999",
  "status": "pending_payment",
  "amount": 26910,
  "channel": "vip",
  "discount_applied": true,
  "message": "VIP 10% discount applied."
}

@mock-when header.Authorization == None
HTTP/1.1 401 Unauthorized
Content-Type: application/json

{
  "error": "Unauthorized",
  "message": "Missing Bearer token in Authorization header."
}

@mock-default
HTTP/1.1 201 Created
Content-Type: application/json

{
  "id": "ord_std_10001",
  "status": "pending_payment",
  "amount": 29900,
  "duplicated": false
}
```

---

## 2. 订单详情查询接口 (Get Order Details)

`GET /api/v1/orders/detail` 供客户端查询订单状态。
本代码块为**纯契约定义（无 @test 标记）**：在 Mock 模式下启动服务，在 `rupost test` 模式下**自动跳过执行**，杜绝向外网盲目发包。

```http
@name get-order-detail
GET /api/v1/orders/detail

@mock-when query.id == ord_non_exist
HTTP/1.1 404 Not Found
Content-Type: application/json

{
  "error": "NotFound",
  "message": "Order does not exist or has been deleted."
}

@mock-default
HTTP/1.1 200 OK
Content-Type: application/json

{
  "id": "ord_std_10001",
  "status": "paid",
  "amount": 29900,
  "item_count": 2,
  "tracking_number": "SF1002345678"
}
```
