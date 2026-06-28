# 变量与环境管理 (Variables & Environments)

RuPost 提供了一套灵活且强大的变量系统，遵循“一次配置，多处复用”的原则。

---

## 1. 定义变量

### A. 在 `rupost.toml` 中定义 (推荐)
在项目根目录创建或编辑 `rupost.toml`，按环境组织变量。支持直接引用系统的环境变量：
```toml
[environments.dev]
base_url = "https://httpbin.org"
token = "dev-token-123"

[environments.prod]
base_url = "https://api.example.com"
token = "${PROD_TOKEN}" # 引用系统环境变量
```

### B. 从响应中动态捕获 (`@capture`)
支持在测试块尾部动态解析响应内容，并将提取的字段绑定为局部变量：
```http
POST /login
Content-Type: application/json

{ "username": "admin", "password": "123" }

@capture auth_token from body.token
```

### C. 通过命令行临时定义 (`--var`)
在测试执行时，可以临时覆盖或新增变量：
```bash
rupost t test.http --var base_url=http://localhost:8080
```

### D. 本地局部环境变量 (`.env`)
在本地开发联调时，如果不想修改共享的 `rupost.toml`，可以在根目录下创建一个 `.env` 或 `.env.<env_name>` 文件：
```env
base_url = http://localhost:8081
api_key = my-local-key
```
RuPost 启动时会自动检测并加载该局部文件，覆盖默认的环境默认配置。

### E. 全局共享变量 (`@capture global. & env.`)
* **`global.` 前缀共享**：在并发测试模式下，如果想要跨测试文件安全共享数据（例如在 `login.http` 里拿到 Token，让并行的 `user.http` 读取），可以通过 `@capture global.token from body.token` 捕获。此时该变量会写入全局线程安全的数据区，其他文件通过 `{{global.token}}` 实时引用最新值。
* **系统环境变量直射**：支持直接通过 `{{env.USER}}` 或 `{{env.PATH}}` 等语法映射并解析当前终端进程的系统环境变量。

---

## 2. 级联覆盖优先级 (Cascading Priority)

当同一个变量在多处被定义时，RuPost 严格遵循以下优先级进行覆盖合并（高优先级覆盖低优先级）：
1. **最高优先级 (Level 1)**：命令行 `--var` 传参 (如 `--var key=val`)
2. **第二优先级 (Level 2)**：终端进程中的**系统环境变量** (仅覆盖已有同名变量，不污染全局命名空间)
3. **第三优先级 (Level 3)**：本地局部环境变量文件 (`.env` 或 `.env.<env_name>`)
4. **最低优先级 (Level 4)**：共享 `rupost.toml` 环境配置中的默认变量

---

## 3. 使用变量

在 `.http` 或 `.md` 代码块中，使用 `{{var_name}}` 语法引用变量：
```http
GET {{base_url}}/users/1
Authorization: Bearer {{token}}
```

---

## 4. 环境与局部变量文件切换

执行测试时，通过 `-e` / `--env` 指定环境名，并可以通过 `--env-file` 显式指定本地局部变量文件路径：
```bash
# 自动合并加载共享 dev 配置和本地局部默认 .env
rupost t examples/basic.http -e dev

# 显式指定加载本地特定的局部配置文件
rupost t examples/basic.http -e dev --env-file .env.staging
```
