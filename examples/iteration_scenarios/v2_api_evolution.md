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

## @test 验证 V2 网关演进兼容性
下面是用以自动化校验 V2 版本网关向前兼容逻辑与拦截策略的回归测试用例。

```http
@name test-v2-legacy-compat-query
@test
GET http://localhost:9000/api/v1/orders/102?status=completed
@assert status == 200
@assert body.compat_mode == true
@assert body.payment_method == legacy

@name test-v2-create-order-upgrade-needed
@test
POST http://localhost:9000/api/v2/orders
Content-Type: application/json

{
  "amount": 199.0,
  "payment_method": "alipay"
}

@assert status == 400
@assert body.error contains Upgrade required

@name test-v2-create-order-success
@test
POST http://localhost:9000/api/v2/orders
X-App-Version: 2.0.0
Content-Type: application/json

{
  "amount": 199.0,
  "payment_method": "alipay"
}

@assert status == 201
@assert body.x_app_validated == true
@assert body.order_id == 999
```

---

## 📖 使用说明与场景运行

本文件（`v2_api_evolution.md`）定义了 V2 演进版接口的网关校验拦截与旧客户端平滑兼容逻辑。

### 1. 启动演进契约的 Mock 服务
在独立终端运行以下命令开启 9000 端口上的 Mock 服务器：
```bash
rupost mock examples/iteration_scenarios/v2_api_evolution.md --port 9000
```

### 2. 通过 Curl 进行手工验证
- **测试 V1 接口平滑升级（已合并 V2 补丁字段）**：
  ```bash
  curl -i "http://localhost:9000/api/v1/orders/102?status=completed"
  # 返回 200 OK，且字段中带有 "compat_mode": true
  ```

- **测试 V2 新写操作被网关强校验拦截**：
  ```bash
  curl -i -X POST http://localhost:9000/api/v2/orders
  # 返回 400 Bad Request，并提示 "Upgrade required"
  ```

- **测试带上正确版本参数放行**：
  ```bash
  curl -i -X POST -H "X-App-Version: 2.0.0" http://localhost:9000/api/v2/orders
  # 返回 201 Created，且包含 "x_app_validated": true
  ```

### 3. 一键回归集成用例
确保 Mock 服务器在后台 9000 端口运行，使用下述命令执行回归自动化测试：
```bash
rupost test examples/iteration_scenarios/v2_api_evolution.md
```
系统将对 V1 兼容模式、V2 缺少版本头拒绝以及 V2 创建成功这三大场景进行全套断言回归检验。
