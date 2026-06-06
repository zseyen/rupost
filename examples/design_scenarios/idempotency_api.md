---
title: 订单支付幂等性 API 设计
version: 1.0.0
base_path: /api/v1
rules: |
  1. 所有写操作接口必须在 Header 中携带 `Idempotency-Key`，以保证交易唯一性。
  2. 针对同一 `Idempotency-Key` 的重复请求，必须返回 200 OK 且包含缓存的结果及 `"duplicated": true` 标记，不能重复扣款。
---

# 支付与交易幂等性契约

本文件展示了如何在 API 设计阶段设计高并发交易的幂等机制并使用 Mock 进行联调。

## 提交交易支付
通过提供 `Idempotency-Key` 来防止网络抖动导致的重复扣款。

```http
@name execute-payment
POST /api/v1/payments
Content-Type: application/json

@mock-when header.Idempotency-Key == repeat_key_12345
HTTP/1.1 200 OK
Content-Type: application/json

{
  "transaction_id": "tx_888999",
  "status": "success",
  "amount": 250.00,
  "duplicated": true,
  "msg": "Transaction processed previously."
}

@mock-when header.Idempotency-Key == None
HTTP/1.1 400 Bad Request
Content-Type: application/json

{
  "error": "Bad Request: Idempotency-Key header is required for transaction safety."
}

@mock-default
HTTP/1.1 201 Created
Content-Type: application/json

{
  "transaction_id": "tx_888999",
  "status": "success",
  "amount": 250.00,
  "duplicated": false
}
```
