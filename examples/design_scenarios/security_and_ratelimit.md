---
title: 账户与网关安全 API 设计
version: 1.0.0
base_path: /api/v1
security:
  - type: BearerAuth
    description: 账户接口必须进行 Token 认证，非法或过期的 Token 会触发 401 或 403。
rules: |
  1. 所有以 /api/v1/accounts/ 开头的接口属于敏感操作，网关将对其强校验 Authorization Header。
  2. 智能体审计规则：若响应体包含未脱敏的 password 或 plain_credit_card 字段，则判定契约违规。
---

# 账户服务与网关模拟契约

本文件展示了如何在 API 设计阶段定义安全鉴权与限流的网关模拟行为。

## 敏感操作：重置支付密码
该敏感接口要求完整的鉴权校验与防刷限制，Mock 服务根据客户端发来的 Token 类型模拟网关的响应。

```http
@name reset-payment-password
POST /api/v1/accounts/reset-password
Content-Type: application/json

@mock-when header.Authorization == None
HTTP/1.1 401 Unauthorized
Content-Type: application/json

{
  "error": "Unauthorized: Missing Authorization header."
}

@mock-when header.Authorization == Bearer expired_token
HTTP/1.1 403 Forbidden
Content-Type: application/json

{
  "error": "Forbidden: Token has expired. Please log in again."
}

@mock-when header.X-RateLimit-Trigger == true
HTTP/1.1 429 Too Many Requests
Content-Type: application/json

{
  "error": "Too Many Requests: Rate limit exceeded. Try again in 60s."
}

@mock-default
HTTP/1.1 200 OK
Content-Type: application/json

{
  "status": "success",
  "message": "Password reset email sent."
}
```

## @test 验证网关安全与限流防护
下面是自动化检测网关前置鉴权、过期拦截与频控阻断的测试用例：

```http
@name test-gateway-auth-none
@test
POST http://localhost:9000/api/v1/accounts/reset-password
Content-Type: application/json

@assert status == 401
@assert body.error contains Missing Authorization

@name test-gateway-auth-expired
@test
POST http://localhost:9000/api/v1/accounts/reset-password
Authorization: Bearer expired_token
Content-Type: application/json

@assert status == 403
@assert body.error contains expired

@name test-gateway-ratelimit-block
@test
POST http://localhost:9000/api/v1/accounts/reset-password
Authorization: Bearer valid_token
X-RateLimit-Trigger: true
Content-Type: application/json

@assert status == 429
@assert body.error contains Rate limit exceeded

@name test-gateway-normal-flow
@test
POST http://localhost:9000/api/v1/accounts/reset-password
Authorization: Bearer valid_token
Content-Type: application/json

@assert status == 200
@assert body.status == success
```

---

## 📖 使用说明与场景运行

本文件（`security_and_ratelimit.md`）模拟了生产级 API 网关的安全控制与高频访问限流策略。

### 1. 启动安全 Mock 服务器
在后台启动监听 9000 端口的 Mock 引擎：
```bash
rupost mock examples/design_scenarios/security_and_ratelimit.md --port 9000
```

### 2. 通过 Curl 进行模拟攻击与正常访问
- **缺失鉴权头（None 匹配场景）**：
  ```bash
  curl -i -X POST http://localhost:9000/api/v1/accounts/reset-password
  # 返回 401 Unauthorized
  ```

- **携带失效密钥**：
  ```bash
  curl -i -X POST -H "Authorization: Bearer expired_token" http://localhost:9000/api/v1/accounts/reset-password
  # 返回 403 Forbidden
  ```

- **触发系统限流拦截**：
  ```bash
  curl -i -X POST -H "X-RateLimit-Trigger: true" http://localhost:9000/api/v1/accounts/reset-password
  # 返回 429 Too Many Requests
  ```

- **正常鉴权请求**：
  ```bash
  curl -i -X POST -H "Authorization: Bearer valid_token" http://localhost:9000/api/v1/accounts/reset-password
  # 返回 200 OK
  ```

### 3. 一键执行自动化安全基线回归
保持服务在 9000 端口开启状态下，执行：
```bash
rupost test examples/design_scenarios/security_and_ratelimit.md
```
系统将会依次验证缺失认证、过期拦截、限流防护及正常业务流程的所有状态码和返回消息断言。
