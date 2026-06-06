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
