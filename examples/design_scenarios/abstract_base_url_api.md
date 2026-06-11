---
title: 用户角色授权服务 (抽象 Base URL 演示)
version: 1.0.0
base_path: /api/v1/auth
rules: |
  1. 通过传入 query.user 判定角色权限。
  2. 测试用例中使用 {{base_url}} 动态替换，不硬编码服务器主机与端口。
---

# 角色授权契约

本契约展示了如何在 API 设计阶段抽象 `base_url` 进行解耦测试。

## 查询用户角色权限

```http
@name get-user-roles
GET /api/v1/auth/roles

@mock-when query.user == admin
HTTP/1.1 200 OK
Content-Type: application/json

[
  "admin",
  "user"
]

@mock-default
HTTP/1.1 403 Forbidden
Content-Type: application/json

{
  "error": "Forbidden: Access denied for non-admin user."
}
```

## @test 验证管理员角色授权正常
管理员通过 `user=admin` 获取角色列表。

```http
@name test-roles-admin-success
@test
GET {{base_url}}/api/v1/auth/roles?user=admin
@assert status == 200
@assert body.0 == "admin"
```

## @test 验证访客角色权限受限
访客通过 `user=guest` 尝试访问被拒绝。

```http
@name test-roles-guest-forbidden
@test
GET {{base_url}}/api/v1/auth/roles?user=guest
@assert status == 403
@assert body.error contains Forbidden
```

---

## 📖 使用说明与场景运行

本文件示范了通过变量进行**契约测试重定向**的最佳实践。

### 1. 启动 Mock 适配器
在终端中为本契约拉起 Mock 服务，监听在 9000 端口：
```bash
rupost mock examples/design_scenarios/abstract_base_url_api.md --port 9000
```

### 2. 通过注入 `--var` 参数定向测试本地 Mock
在 Mock 服务器运行期间，执行下述命令：
```bash
rupost test examples/design_scenarios/abstract_base_url_api.md --var base_url=http://localhost:9000
```
此时系统会将所有 `{{base_url}}` 变量动态解析并渲染为 `http://localhost:9000`，测试运行器会把请求发往本地 Mock 服务，输出断言通过报告。

### 3. 一键切换至真实后端开发环境
当真实的后端角色授权服务部署在 staging 环境后（例如 `https://auth-stage.internal.corp`），无需更改任何测试用例代码，直接执行：
```bash
rupost test examples/design_scenarios/abstract_base_url_api.md --var base_url=https://auth-stage.internal.corp
```
测试执行引擎将自动重定向发包，无缝验证真实开发的接口是否完美履行了设计契约！
