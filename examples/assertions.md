# Assertion Examples

演示如何在 Markdown 中使用断言。

## 状态码断言

### 成功请求

```http
@name success-request
@assert status == 200
GET https://httpbin.org/status/200
```

### 创建资源

```rest
@name created-resource
@assert status == 201
@assert headers.Content-Type contains "json"
POST https://httpbin.org/status/201
```

## Body 断言

### JSON 响应断言

```http
@name json-body-check
@assert status == 200
@assert body.url exists
@assert body.headers exists
GET https://httpbin.org/get
```

### 嵌套字段断言

```rest
@name nested-field-check
@assert status == 200
@assert body.json.name == "测试"
@assert body.json.active == true
POST https://httpbin.org/post
Content-Type: application/json

{
  "name": "测试",
  "active": true,
  "count": 42
}
```

## Headers 断言

### 检查响应头

```http
@name response-headers-check
@assert status == 200
@assert headers.Content-Type exists
@assert headers.Server exists
GET https://httpbin.org/get
```

### 自定义响应头

```rest
@name custom-response-headers
@assert status == 200
@assert headers.X-Custom-Header == "test-value"
GET https://httpbin.org/response-headers?X-Custom-Header=test-value
```

## @name 覆盖测试

下面的请求名称会使用 `@name` 而不是 Markdown 标题：

### 这是 Markdown 标题

```http
@name this-is-custom-name
@assert status == 200
GET https://httpbin.org/get
```

## 复杂断言组合

### 完整验证示例

```rest
@name full-validation
@assert status == 200
@assert headers.Content-Type contains "application/json"
@assert body.url == "https://httpbin.org/post"
@assert body.json.email == "test@example.com"
@assert body.json.age == 30
POST https://httpbin.org/post
Content-Type: application/json

{
  "email": "test@example.com",
  "age": 30
}
```
