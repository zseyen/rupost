# Authentication Examples

各种认证方式的示例。

## HTTP Basic Authentication

### 使用正确凭据

```http
GET https://httpbingo.org/basic-auth/testuser/testpass
Authorization: Basic dGVzdHVzZXI6dGVzdHBhc3M=
@assert status == 200
```

> 注：`dGVzdHVzZXI6dGVzdHBhc3M=` 是 `testuser:testpass` 的 Base64 编码

### 使用错误凭据（应该失败）

```rest
GET https://httpbingo.org/basic-auth/user/pass
Authorization: Basic wrong-credentials
@assert status == 401
```

## Bearer Token 认证

### 有效 Bearer Token

```http
@name bearer-auth-success
GET https://httpbingo.org/bearer
Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9
@assert status == 200
```

### 无 Token（应该失败）

```rest
GET https://httpbingo.org/bearer
@assert status == 401
```

## 自定义 Header 认证

### API Key 认证

```http
@name api-key-auth
GET https://httpbingo.org/headers
X-API-Key: secret-api-key-12345
X-Client-ID: client-abc-123
@assert status == 200
```

### 多种认证信息

```rest
POST https://httpbingo.org/post
Authorization: Bearer token123
X-API-Key: api-key-456
X-Request-ID: req-789
Content-Type: application/json

{
  "action": "authenticate"
}
@assert status == 200
```
