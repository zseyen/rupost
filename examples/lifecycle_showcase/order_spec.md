---
title: Order Service API Specification
version: 1.2.0
base_path: /api/v1
host: api.orderservice.local
rules: |
  - 所有金额字段 (amount) 必须以分为单位（整数）。
  - 创建订单的响应必须包含唯一标识 id。
---

# 订单服务接口规格书 (Order Service API Contract)

本文件作为开发与联调的统一契约事实来源 (SSOT)。

## 1. 创建订单接口

### 1.1 路由契约定义
```http
@name CreateOrder
POST /orders
Content-Type: application/json

{
  "item_count": 5,
  "amount": 15000
}

### 响应契约定义
# 弱 Schema 超集比对标准
# @spec-response-schema = {"id": "string", "item_count": "number", "amount": "number", "status": "string", "discount_applied": "boolean"}
```

### 1.2 联调 Mock 变体分支配置
下面定义了订单服务的 Mock 规则。引擎启动后会自动依据请求入参分流命中不同响应。

```http
### @mock-variant 大客户批量订单 (item_count > 100)
# @mock-when $.body.item_count > 100
HTTP/1.1 201 Created
Content-Type: application/json

{
  "id": "vip_order_88888",
  "item_count": 120,
  "amount": 360000,
  "status": "processed",
  "discount_applied": true
}

### @mock-variant 普通标准订单
HTTP/1.1 201 Created
Content-Type: application/json

{
  "id": "std_order_11111",
  "item_count": 5,
  "amount": 15000,
  "status": "pending",
  "discount_applied": false
}
```

---

## 2. 旧版本订单查询接口 (已废弃)

```http
@name GetOrderLegacy
@deprecated Use GET /api/v1/orders/:id instead
GET /orders/legacy/:id

### 响应契约定义
# @spec-response-schema = {"id": "string", "legacy_flag": "boolean"}
```
