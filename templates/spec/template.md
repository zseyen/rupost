---
title: 服务契约规格书 (Living API Specification)
version: 1.0.0
host: api.example.com
rules: |
  1. 所有写操作接口必须携带有效的认证凭据。
  2. 接口支持变体规则调度与自动化契约校验。
---

# 活文档服务契约与测试规范 (Living API Specification)

本文件作为业务开发、前端联调、自动化测试与契约审计的**单一事实来源 (SSOT)**。
可直接运行：
- 启动 Mock 仿真服务: `rupost mock spec.md --port 9000`
- 执行自动化端到端测试: `rupost test spec.md --env dev`
- 启动交互式工作台编辑与原位调试: `rupost web`

---

## 1. 用户信息查询 (User Profile Query)

该接口支持单块双角色：既可作为测试用例，也可作为本地 Mock 变体服务。

```http
@test
@name get-user-profile
GET /api/v1/users/profile
Authorization: Bearer {{auth_token}}
Accept: application/json

@assert status == 200
@assert body.id exists

@mock-when header.X-Role == admin
HTTP/1.1 200 OK
Content-Type: application/json

{
  "id": "usr_admin_001",
  "username": "superadmin",
  "role": "admin",
  "permissions": ["all"]
}

@mock-default
HTTP/1.1 200 OK
Content-Type: application/json

{
  "id": "usr_standard_100",
  "username": "alex",
  "role": "user",
  "permissions": ["read"]
}
```

---

## 2. 订单创建接口 (Create Order)

演示带请求体、断言与多状态码 Mock 分支。

```http
@test
@name create-order
POST /api/v1/orders
Authorization: Bearer {{auth_token}}
Content-Type: application/json

{
  "item_id": "item_9901",
  "quantity": 1,
  "amount": 4900
}

@assert status == 201
@assert body.order_id exists

@mock-when query.dry_run == true
HTTP/1.1 200 OK
Content-Type: application/json

{
  "simulated": true,
  "estimated_fee": 4900
}

@mock-default
HTTP/1.1 201 Created
Content-Type: application/json

{
  "order_id": "ord_20261107_8899",
  "status": "pending_payment",
  "amount": 4900
}
```
