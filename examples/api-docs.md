# HTTPBin API 文档

欢迎使用 HTTPBin API 测试文档。本文档包含可执行的 HTTP 请求示例。

## 目录

- [基础请求](#基础请求)
- [认证示例](#认证示例)
- [请求/响应操作](#请求响应操作)
- [测试端点](#测试端点)

---

## 基础请求

### GET 请求

最简单的 GET 请求示例，返回请求信息。

```http
GET https://httpbin.org/get
```

**期望响应**:
- HTTP 状态码: 200
- 返回请求的 headers、args 等信息

### POST 请求

发送 JSON 数据到服务器。

```rest
POST https://httpbin.org/post
Content-Type: application/json

{
  "name": "测试用户",
  "email": "test@example.com"
}
```

**期望响应**:
- 回显发送的 JSON 数据
- 包含请求 headers 信息

---

## 认证示例

### Basic Authentication

使用 HTTP Basic 认证。

```http
GET https://httpbin.org/basic-auth/user/passwd
Authorization: Basic dXNlcjpwYXNzd2Q=
```

> **提示**: `dXNlcjpwYXNzd2Q=` 是 `user:passwd` 的 Base64 编码。

### Bearer Token

使用 Bearer Token 认证。

```rest
GET https://httpbin.org/bearer
Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9
```

---

## 请求/响应操作

### 查看请求 Headers

这个端点返回你发送的所有 headers。

```http
GET https://httpbin.org/headers
User-Agent: RuPost/1.0
X-Custom-Header: 自定义值
Accept-Language: zh-CN
```

### 设置响应 Headers

可以通过查询参数设置响应 headers。

```rest
GET https://httpbin.org/response-headers?FOO=BAR&BIZ=BAZ
```

### POST Form Data

发送表单数据。

```http
POST https://httpbin.org/post
Content-Type: application/x-www-form-urlencoded

username=alice&password=secret123
```

---

## 测试端点

### 返回指定状态码

测试不同的 HTTP 状态码。

#### 成功 - 200 OK

```http
GET https://httpbin.org/status/200
```

#### 重定向 - 302 Found

```rest
GET https://httpbin.org/status/302
```

#### 客户端错误 - 404 Not Found

```http
GET https://httpbin.org/status/404
```

#### 服务端错误 - 500 Server Error

```http
GET https://httpbin.org/status/500
```

### 延迟响应

模拟慢速 API，延迟 2 秒后响应。

```rest
GET https://httpbin.org/delay/2
```

> **注意**: 适合测试超时配置。

### UUID 生成

生成一个随机 UUID。

```http
GET https://httpbin.org/uuid
```

**示例响应**:
```json
{
  "uuid": "7a4a5e5c-9f3a-4b8d-9c2e-1f7e3d6a4b2c"
}
```

### JSON 数据

返回一个示例 JSON 对象。

```rest
GET https://httpbin.org/json
Accept: application/json
```

---

## 高级功能

### 重定向测试

测试 HTTP 重定向。

```http
GET https://httpbin.org/redirect/3
```

这会进行 3 次重定向后返回最终结果。

### Cookies

#### 设置 Cookie

```rest
GET https://httpbin.org/cookies/set?name=value
```

#### 读取 Cookie

```http
GET https://httpbin.org/cookies
Cookie: name=value
```

### 图片响应

获取一个 JPEG 图片（注意：返回的是图片数据，不是 JSON）。

```rest
GET https://httpbin.org/image/jpeg
Accept: image/jpeg
```

---

## 压力测试

### 大响应体

返回指定字节数的随机数据。

```http
GET https://httpbin.org/bytes/1024
```

以上示例返回 1KB 的随机数据。

### 流式响应

```rest
GET https://httpbin.org/stream/10
```

返回 10 行流式 JSON 数据。

---

## 工具提示

### 如何使用本文档

1. **测试单个请求**:
   ```bash
   rupost run "GET https://httpbin.org/get"
   ```

2. **运行整个文档**:
   ```bash
   rupost test api-docs.md
   ```

3. **使用环境变量** (Phase 3):
   ```bash
   export BASE_URL=https://httpbin.org
   rupost test api-docs.md --env production
   ```

### 预期输出

成功的请求会显示：
- ✓ 请求名称
- HTTP 状态码
- 响应时间
- 响应 body（如果启用 `--verbose`）

---

## 反馈

如果你发现任何问题或有改进建议，请提交 Issue 到 GitHub 仓库。

---

**文档版本**: 1.0.0  
**最后更新**: 2026-01-21  
**作者**: RuPost Team
