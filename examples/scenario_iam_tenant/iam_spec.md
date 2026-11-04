---
title: 多租户企业级权限与身份服务规格 (IAM & Tenant RBAC Spec)
version: 1.0.0
host: iam.enterprise.local
rules: |
  1. 所有接口必须在请求头显式提供 `X-Tenant-ID` 标明租户上下文。
  2. 针对处于冻结状态 (suspended) 的租户，必须阻断访问并返回 403 Forbidden。
  3. 角色权限定义遵循最小特权原则。
---

# 组织与身份服务契约 (IAM Specification)

本文件规范多租户 SaaS 架构下的角色授权与成员管理契约。

---

## 1. 租户角色权限查询 (Get Tenant Roles & Permissions)

`GET /api/v1/tenants/roles` 用于根据租户 ID 与角色标识获取权限矩阵。
作为**单块双角色契约**：既定义权限分流 Mock，又作为自动化测试用例。

```http
@test
@name query-tenant-roles
GET /api/v1/tenants/roles
X-Tenant-ID: corp_alpha
Authorization: Bearer dev_iam_token_xyz

@assert status == 200
@assert body.tenant_id == "corp_alpha"

@mock-when header.X-Tenant-ID == suspended_corp
HTTP/1.1 403 Forbidden
Content-Type: application/json

{
  "error": "TenantSuspended",
  "message": "Tenant corp has been suspended due to overdue billing."
}

@mock-when query.role == admin
HTTP/1.1 200 OK
Content-Type: application/json

{
  "tenant_id": "corp_alpha",
  "role": "admin",
  "permissions": [
    "user:create",
    "user:delete",
    "billing:read",
    "billing:write",
    "audit:export"
  ]
}

@mock-when query.role == member
HTTP/1.1 200 OK
Content-Type: application/json

{
  "tenant_id": "corp_alpha",
  "role": "member",
  "permissions": [
    "project:read",
    "project:write"
  ]
}

@mock-default
HTTP/1.1 200 OK
Content-Type: application/json

{
  "tenant_id": "corp_alpha",
  "role": "guest",
  "permissions": [
    "project:read"
  ]
}
```

---

## 2. 租户用户创建 (Create Tenant Member)

`POST /api/v1/tenants/users` 负责在指定租户下分配与创建成员。
本代码块为**纯契约定义（无 @test 标记）**：测试期自动跳过执行，Mock 联调时提供分支响应。

```http
@name create-tenant-user
POST /api/v1/tenants/users
X-Tenant-ID: corp_alpha
Content-Type: application/json

{
  "email": "alex@corp-alpha.com",
  "name": "Alex Mercer",
  "role": "member"
}

@mock-when header.X-Tenant-ID == None
HTTP/1.1 400 Bad Request
Content-Type: application/json

{
  "error": "MissingTenantContext",
  "message": "X-Tenant-ID header is required."
}

@mock-when query.conflict == true
HTTP/1.1 409 Conflict
Content-Type: application/json

{
  "error": "UserAlreadyExists",
  "message": "The member email is already registered in this tenant."
}

@mock-default
HTTP/1.1 201 Created
Content-Type: application/json

{
  "id": "usr_tenant_9901",
  "tenant_id": "corp_alpha",
  "email": "alex@corp-alpha.com",
  "status": "active"
}
```
