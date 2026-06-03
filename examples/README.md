# RuPost 示例文件

这个目录包含了各种使用示例，既可以作为学习文档，也可以用于实际测试。

## 文件说明

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

## 使用方法

由于部分高级示例使用了变量（如 `{{base_url}}` 或 `{{baseUrl}}`），如果直接运行未配置环境的用例，引擎会友好拦截并提示未配置错误。我们把示例分为 **免配置直接执行** 与 **多环境变量执行** 两类：

### 1. 免配置直接执行 (开箱即用)

对于不含自定义变量、直接请求公共测试源（`https://httpbingo.org`）的示例，可以直接输入命令执行（如果未安装全局命令，可使用 `cargo run -- test` 替代 `rupost test`）：

```bash
# 运行基础 HTTP 示例
rupost test examples/basic.http

# 运行断言机制示例 (HTTP 文件版)
rupost test examples/assertions.http

# 运行断言机制示例 (Markdown 包含版)
rupost test examples/assertions.md

# 运行多请求链式批量执行
rupost test examples/multiple.http

# 运行 Markdown 嵌套代码块提取示例
rupost test examples/nested-blocks.md
```

### 2. 准备多环境变量

若要运行包含业务逻辑、使用 `{{base_url}}` 等变量的示例（如 `basic-api.http`, `auth-flow.http` 等），请按照以下步骤准备变量上下文：

#### 步骤 1: 拷贝配置文件
将示例中的 `rupost.toml` 复制到项目根目录（该配置默认将 `dev` / `test` 环境的 `base_url` 设置为稳定的公共源 `https://httpbingo.org`）：

```bash
cp examples/rupost.toml .
```

#### 步骤 2: 设置环境变量（可选）
如果配置文件中引用了系统环境变量（如 `${DEV_API_KEY}`）：

```bash
export DEV_API_KEY="your-dev-api-key"
export PROD_API_KEY="your-prod-api-key"
```

#### 步骤 3: 指定环境执行
运行时通过 `-e` 或 `--env` 指定加载哪个环境配置，以便引擎能够顺利解析出 `base_url`：

```bash
# 指定 dev 环境运行 basic-api.http
rupost test examples/basic-api.http --env dev

# 运行 Markdown 格式的完整 API 测试文档
rupost test examples/api-testing.md --env dev

# 在运行 cookie 演示时使用 dev 环境进行状态保持
rupost test examples/cookie_demo.md --env dev

# 覆盖配置文件中的变量
rupost test examples/basic-api.http --env dev --var api_key=custom-key

# 开启详细输出查看调试日志
rupost test examples/basic-api.http --env dev --verbose
```

## 变量使用说明

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

## 测试场景

每个示例文件都涵盖了特定的测试场景：

- **basic-api.http** - 适合快速验证 API 基本功能
- **auth-flow.http** - 测试认证和授权流程
- **crud-operations.http** - 测试完整的数据管理功能
- **api-testing.md** - 适合作为 API 文档和测试用例

## 提示

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

## 更多信息

查看项目主 README 了解更多功能和用法。
