# RuPost 示例文件

这个目录包含了各种使用示例，既可以作为学习文档，也可以用于实际测试。

## 📁 文件说明

### 配置文件

- **`rupost.toml`** - 完整的配置文件示例
  - 展示多环境配置（dev/test/staging/prod）
  - 演示系统环境变量的使用
  - 包含各种常用配置项

### HTTP 测试文件

- **`basic-api.http`** - 基础 API 测试示例
  - GET/POST/PUT/DELETE 基本操作
  - 展示变量在不同位置的使用（URL、Header、Body）
  
- **`auth-flow.http`** - 认证流程示例
  - 完整的用户认证流程
  - 登录、注册、密码管理等
  
- **`crud-operations.http`** - CRUD 操作示例
  - 以文章管理为例的完整 CRUD 操作
  - 包含批量操作和高级查询

### Markdown 文档

- **`variables.md`** - 变量系统使用示例
  - 演示变量替换的各种场景
  
- **`api-testing.md`** - API 测试文档
  - 类似 API 文档的格式
  - 可以直接作为测试用例执行

## 🚀 使用方法

### 1. 准备配置文件

将 `rupost.toml` 复制到项目根目录：

```bash
cp examples/rupost.toml .
```

### 2. 设置环境变量（可选）

如果使用系统环境变量：

```bash
export DEV_API_KEY="your-dev-api-key"
export PROD_API_KEY="your-prod-api-key"
```

### 3. 运行测试

**使用默认环境：**
```bash
rupost test examples/basic-api.http
```

**指定环境：**
```bash
rupost test examples/basic-api.http --env dev
rupost test examples/auth-flow.http --env prod
```

**覆盖变量：**
```bash
rupost test examples/basic-api.http --env dev --var api_key=custom-key
```

**测试 Markdown 文件：**
```bash
rupost test examples/api-testing.md --env dev
```

**详细输出：**
```bash
rupost test examples/basic-api.http --env dev --verbose
```

## 📝 变量使用说明

### 配置文件中的变量

在 `rupost.toml` 中定义：

```toml
[environments.dev]
base_url = "http://localhost:3000"
api_version = "v1"
api_key = "${DEV_API_KEY}"  # 引用系统环境变量
```

### 请求文件中使用变量

在 `.http` 或 `.md` 文件中：

```http
GET {{base_url}}/{{api_version}}/users
Authorization: Bearer {{api_key}}
```

### 变量优先级

1. **CLI 参数** (`--var`) - 最高优先级
2. **环境配置** (`rupost.toml` 中的环境)
3. **系统环境变量** (`${VAR}`)

## 🎯 测试场景

每个示例文件都涵盖了特定的测试场景：

- **basic-api.http** - 适合快速验证 API 基本功能
- **auth-flow.http** - 测试认证和授权流程
- **crud-operations.http** - 测试完整的数据管理功能
- **api-testing.md** - 适合作为 API 文档和测试用例

## 💡 提示

1. **修改示例以适应你的 API**
   - 替换 URL 和端点
   - 调整请求体结构
   - 添加你的自定义 Header

2. **跳过特定请求**
   ```http
   ### 此请求将被跳过
   # @skip
   DELETE {{base_url}}/dangerous-operation
   ```

3. **添加断言**
   ```http
   ### 验证响应
   # @assert status == 200
   # @assert body.success == true
   GET {{base_url}}/api/status
   ```

4. **设置超时**
   ```http
   ### 长时间运行的请求
   # @timeout 60s
   POST {{base_url}}/heavy-operation
   ```

## 🔍 更多信息

查看项目主 README 了解更多功能和用法。
