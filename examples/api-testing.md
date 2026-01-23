# API 测试文档

完整的 API 测试示例，使用 Markdown 格式。

使用方法：
```bash
rupost test examples/api-testing.md --env dev
```

## 基础健康检查

### 检查服务状态

```http
GET {{base_url}}/health
Accept: application/json
```

### 检查 API 版本

```http
GET {{base_url}}/version
Accept: application/json
```

## 用户管理 API

### 获取用户列表

获取所有用户的列表，支持分页。

```http
GET {{base_url}}/{{api_version}}/users?page=1&limit=20
Authorization: Bearer {{api_key}}
Accept: application/json
```

### 创建新用户

创建一个新的测试用户。

```http
POST {{base_url}}/{{api_version}}/users
Content-Type: application/json
Authorization: Bearer {{api_key}}

{
  "email": "{{test_user_email}}",
  "password": "{{test_user_password}}",
  "name": "Test User",
  "role": "user"
}
```

### 获取用户详情

```http
GET {{base_url}}/{{api_version}}/users/123
Authorization: Bearer {{api_key}}
Accept: application/json
```

### 更新用户信息

```http
PUT {{base_url}}/{{api_version}}/users/123
Content-Type: application/json
Authorization: Bearer {{api_key}}

{
  "name": "Updated User Name",
  "email": "updated@example.com"
}
```

### 删除用户

```http
DELETE {{base_url}}/{{api_version}}/users/123
Authorization: Bearer {{api_key}}
```

## 认证相关

### 用户登录

使用配置的测试账户进行登录。

```http
POST {{base_url}}/{{api_version}}/auth/login
Content-Type: application/json

{
  "email": "{{test_user_email}}",
  "password": "{{test_user_password}}"
}
```

### 获取当前用户信息

```http
GET {{base_url}}/{{api_version}}/auth/profile
Authorization: Bearer {{api_key}}
Accept: application/json
```

## 文件上传

### 上传头像

```http
POST {{base_url}}/{{api_version}}/upload/avatar
Content-Type: multipart/form-data
Authorization: Bearer {{api_key}}

file=@/path/to/avatar.jpg
```

### 上传文档

```http
POST {{base_url}}/{{api_version}}/upload/document
Content-Type: multipart/form-data
Authorization: Bearer {{api_key}}

file=@/path/to/document.pdf
title=Important Document
```

## 高级查询

### 复杂过滤查询

```http
GET {{base_url}}/{{api_version}}/posts?status=published&category=tech&sort=-created_at&page=1&per_page=10
Authorization: Bearer {{api_key}}
Accept: application/json
```

### 全文搜索

```http
GET {{base_url}}/{{api_version}}/search?q=Rust 教程&type=post
Authorization: Bearer {{api_key}}
Accept: application/json
```

## WebHook 测试

### 创建 WebHook

```http
POST {{base_url}}/{{api_version}}/webhooks
Content-Type: application/json
Authorization: Bearer {{api_key}}

{
  "url": "https://example.com/webhook",
  "events": ["user.created", "post.published"],
  "secret": "webhook_secret_key"
}
```

### 测试 WebHook

```http
POST {{base_url}}/{{api_version}}/webhooks/123/test
Authorization: Bearer {{api_key}}
```

## 数据导出

### 导出用户数据

```http
GET {{base_url}}/{{api_version}}/export/users?format=csv
Authorization: Bearer {{api_key}}
Accept: text/csv
```

### 导出统计报表

```http
GET {{base_url}}/{{api_version}}/export/stats?start_date=2024-01-01&end_date=2024-12-31&format=json
Authorization: Bearer {{api_key}}
Accept: application/json
```
