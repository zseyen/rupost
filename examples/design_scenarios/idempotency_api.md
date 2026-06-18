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

## @test 验证支付缺失幂等键时被拦截
写操作接口如果未携带 Idempotency-Key 应被拦截返回 400。

```http
@name test-payment-missing-idempotency-key
@test
POST http://localhost:9000/api/v1/payments
Content-Type: application/json

@assert status == 400
@assert body.error contains Idempotency-Key header is required
```

## @test 验证重复流水幂等处理
相同的幂等键请求应返回之前处理的缓存结果，且带有重复标记。

```http
@name test-payment-idempotency-duplicate
@test
POST http://localhost:9000/api/v1/payments
Idempotency-Key: repeat_key_12345
Content-Type: application/json

@assert status == 200
@assert body.duplicated == true
@assert body.transaction_id == tx_888999
```

## @test 验证首次交易创建成功
全新的幂等键请求应成功创建交易，返回 201。

```http
@name test-payment-first-time-success
@test
POST http://localhost:9000/api/v1/payments
Idempotency-Key: fresh_key_77777
Content-Type: application/json

@assert status == 201
@assert body.duplicated == false
@assert body.status == success
```


---

## 📖 使用说明与场景运行

本文件（`idempotency_api.md`）定义了支付和资金交易中的幂等性校验规则与 Mock 联调设计。

### 1. 启动幂等性 Mock 服务器
在后台启动 9000 端口上的 Mock 服务器：
```bash
rupost mock examples/design_scenarios/idempotency_api.md --port 9000
```

### 2. 通过 Curl 手动模拟幂等流程
- **不带 Idempotency-Key（网关拦截）**：
  ```bash
  curl -i -X POST http://localhost:9000/api/v1/payments
  # 返回 400 Bad Request
  ```

- **正常首次支付请求**：
  ```bash
  curl -i -X POST -H "Idempotency-Key: fresh_key_77777" http://localhost:9000/api/v1/payments
  # 返回 201 Created，包含 "duplicated": false
  ```

- **重试相同请求（命中幂等缓存）**：
  ```bash
  curl -i -X POST -H "Idempotency-Key: repeat_key_12345" http://localhost:9000/api/v1/payments
  # 返回 200 OK，包含 "duplicated": true，模拟防止二次扣款
  ```

### 3. 一键执行自动化幂等回归测试
在 9000 端口 Mock 运行中，执行下述命令：
```bash
rupost test examples/design_scenarios/idempotency_api.md
```
系统将会依次验证缺省头拦截、重复请求幂等命中、以及首次调用正常创建的断言行为。
