# RuPost 文件格式规范

## 概述

RuPost 支持两种文件格式用于定义和运行 HTTP 请求：
- **`.http` 文件**: 专用的 HTTP 请求格式，语法简洁
- **`.md` 文件**: Markdown 文档格式，支持文档与可执行请求混合

---

## `.http` 文件格式

### 基本结构

```http
### 请求名称（可选）
# @name request-name
METHOD URL
Header-Name: Header-Value
...

Request Body (optional)
```

### 支持的 HTTP 方法

- `GET`
- `POST`
- `PUT`
- `DELETE`
- `PATCH`
- `HEAD`
- `OPTIONS`

### Headers 格式

Headers 使用 `Key: Value` 格式，每行一个 header：

```http
Content-Type: application/json
Authorization: Bearer token123
X-Custom-Header: custom-value
```

### Body 格式

Body 在 headers 之后，用一个空行分隔：

#### JSON Body

```http
POST https://api.example.com/users
Content-Type: application/json

{
  "name": "Alice",
  "email": "alice@example.com"
}
```

#### Plain Text Body

```http
POST https://api.example.com/notes
Content-Type: text/plain

This is a plain text note.
```

#### Form Data (URL-encoded)

```http
POST https://api.example.com/login
Content-Type: application/x-www-form-urlencoded

username=alice&password=secret123
```

### 请求分隔符

使用 `###` 分隔多个请求：

```http
### 第一个请求
GET https://api.example.com/users

###

 第二个请求
POST https://api.example.com/users
Content-Type: application/json

{"name": "Bob"}
```

### 注释语法

支持两种注释格式：

#### 单行注释 (`#`)

```http
# 这是一个注释
GET https://api.example.com/users
```

#### 双斜杠注释 (`//`)

```http
// 这也是注释
POST https://api.example.com/users
```

---

## 元数据语法

元数据使用特殊的注释格式 `# @key value`：

### `@name` - 请求名称

为请求指定一个名称，用于输出和日志：

```http
# @name get-user-list
GET https://api.example.com/users
```

### `@skip` - 跳过请求

标记该请求在批量执行时跳过：

```http
# @skip
# @name broken-endpoint
GET https://api.example.com/deprecated
```

或带原因说明：

```http
# @skip This endpoint is under maintenance
POST https://api.example.com/beta-feature
```

### `@timeout` - 超时时间（可选）

设置请求的超时时间（毫秒）：

```http
# @timeout 5000
# @name slow-query
GET https://api.example.com/heavy-endpoint
```

### `@assert` - 断言（Phase 2 Stage 5 实现）

定义响应断言：

```http
# @assert status == 200
# @assert body contains "success"
# @assert header Content-Type == "application/json"
POST https://api.example.com/login
```

---

## `.md` 文件格式（Markdown）

### 基本规则

1. RuPost 会提取所有标记为 `http` 或 `rest` 的代码块
2. 代码块前的最近一个 Markdown 标题作为请求名称
3. 代码块内容按 `.http` 格式解析

### 示例

````markdown
# API 文档

## 用户管理 API

### 获取用户列表

这个接口返回所有用户的列表。

```http
GET https://api.example.com/users
Authorization: Bearer token123
```

**期望响应**:
- 状态码: 200
- 返回用户数组

### 创建新用户

使用此接口创建新用户。

```rest
POST https://api.example.com/users
Content-Type: application/json

{
  "name": "Alice",
  "email": "alice@example.com"
}
```
````

### 提取规则

- **语言标识符**: ` ```http ` 或 ` ```rest `
- **请求名称**: 从最近的 Markdown 标题提取（# 或 ## 或 ###）
- **元数据**: 代码块内的 `@name` 等元数据优先级更高

---

## 变量替换（Phase 3 实现）

### 变量语法

使用 `{{variable_name}}` 语法：

```http
GET {{base_url}}/api/users
Authorization: Bearer {{api_token}}
```

### 环境配置

```toml
# rupost.toml
[environments.dev]
base_url = "http://localhost:8080"
api_token = "dev-token-123"

[environments.prod]
base_url = "https://api.production.com"
api_token = "${PROD_API_TOKEN}"  # 从环境变量读取
```

### 使用

```bash
# 使用开发环境
rupost test api.http --env dev

# 使用生产环境
rupost test api.http --env prod
```

---

## 完整示例

### `.http` 文件示例

```http
### 用户认证流程

# 1. 登录获取 token
# @name login
# @assert status == 200
# @assert body contains "token"
POST https://api.example.com/auth/login
Content-Type: application/json

{
  "username": "admin",
  "password": "secret123"
}

###

# 2. 获取当前用户信息
# @name get-current-user
# @assert status == 200
GET https://api.example.com/users/me
Authorization: Bearer jwt-token-here

###

# 3. 更新用户资料
# @name update-profile
POST https://api.example.com/users/me
Authorization: Bearer jwt-token-here
Content-Type: application/json

{
  "displayName": "Admin User",
  "bio": "System Administrator"
}
```

### `.md` 文件示例

````markdown
# 用户 API 文档

## 认证

### 登录

使用用户名和密码登录，获取 JWT token。

```http
POST https://api.example.com/auth/login
Content-Type: application/json

{
  "username": "admin",
  "password": "secret123"
}
```

**响应示例**:
```json
{
  "token": "eyJhbGc...",
  "expiresIn": 3600
}
```

## 用户管理

### 获取用户列表

返回所有注册用户的列表。

```rest
GET https://api.example.com/users
Authorization: Bearer eyJhbGc...
```

**查询参数**:
- `page`: 页码（默认 1）
- `limit`: 每页数量（默认 20）

````

---

## 语法高亮

### VS Code

安装 "REST Client" 扩展后，`.http` 文件会自动高亮。

### IntelliJ IDEA

内置支持 `.http` 文件格式。

### Markdown 编辑器

Markdown 中的 `http` 代码块会自动高亮。

---

## 最佳实践

### 1. 文件组织

```
project/
├── api-tests/
│   ├── auth.http         # 认证相关
│   ├── users.http        # 用户管理
│   └── products.http     # 产品管理
└── docs/
    └── api-guide.md      # API 文档（含可执行请求）
```

### 2. 命名约定

- 使用描述性的请求名称: `# @name create-new-user`
- 文件名反映功能模块: `user-management.http`
- Markdown 文档使用清晰的标题

### 3. 注释说明

```http
# 创建订单接口
# 
# 注意：
# - 需要有效的 JWT token
# - amount 必须大于 0
# - currency 默认为 USD
# 
# @name create-order
# @assert status == 201
POST https://api.example.com/orders
```

### 4. 版本控制

- ✅ 提交 `.http` 和 `.md` 文件到 Git
- ✅ 环境变量使用占位符 `{{var}}`
- ❌ 不要硬编码敏感信息（token, password）

---

## 与其他工具的兼容性

### IntelliJ HTTP Client

RuPost 的 `.http` 格式与 IntelliJ HTTP Client 基本兼容：
- ✅ 请求分隔符 `###`
- ✅ Headers 格式
- ✅ Body 格式
- ⚠️ 元数据语法略有不同（RuPost 使用 `@` 前缀）

### VS Code REST Client

大部分语法兼容：
- ✅ `.http` 文件格式
- ✅ 变量语法 `{{var}}`
- ⚠️ 断言语法不同（RuPost 使用 `@assert`）

### Hurl

不兼容（Hurl 使用专有 `.hurl` 格式）

---

## 限制和注意事项

### 当前版本（Phase 2）

- ❌ 暂不支持文件上传（multipart/form-data）
- ❌ 暂不支持变量捕获和传递（Phase 3）
- ❌ 暂不支持脚本化断言（Phase 3+）
- ⚠️ GraphQL 支持有限（作为普通 POST 请求）

### 文件编码

- ✅ 必须使用 UTF-8 编码
- ❌ 不支持 BOM (Byte Order Mark)

---

## 错误处理

### 常见解析错误

#### 1. 缺少请求行

```http
# 错误：缺少 METHOD 和 URL
Content-Type: application/json

{"data": "value"}
```

应该是:
```http
POST https://api.example.com/data
Content-Type: application/json

{"data": "value"}
```

#### 2. Headers 和 Body 之间缺少空行

```http
# 错误：没有空行分隔
POST https://api.example.com/data
Content-Type: application/json
{"data": "value"}
```

应该是:
```http
POST https://api.example.com/data
Content-Type: application/json

{"data": "value"}
```

#### 3. 无效的元数据

```http
# 错误：缺少 @ 符号
# name my-request
GET https://api.example.com/data
```

应该是:
```http
# @name my-request
GET https://api.example.com/data
```

---

## 更新日志

- **v0.1.0** (Phase 2 Stage 1):
  - 初始格式规范
  - `.http` 和 `.md` 文件支持
  - 基础元数据语法

- **v0.2.0** (Phase 2 Stage 5, 计划中):
  - 断言语法 `@assert`
  - JSONPath 查询支持

- **v0.3.0** (Phase 3, 计划中):
  - 变量捕获 `@capture`
  - 环境变量替换
  - 请求链支持
