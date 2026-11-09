# 场景 B：多租户企业级权限与组织服务 (IAM & Tenant RBAC Scenario)

本场景作为 RuPost **复杂 Header 级联（租户上下文）、身份权限矩阵与细粒度状态码（400/403/409）**的黄金验证用例。

---

## 包含文件清单

1. **`iam_spec.md`（设计态 SSOT）**：
   - 包含 Frontmatter 规范（`title`、`version`、`host`、`rules`）。
   - **单块双角色主契约 (`GET /api/v1/tenants/roles`)**：
     - 测试用例标记 `@test`，验证角色权限查询；
     - Mock 多条件分支：
       - `header.X-Tenant-ID == suspended_corp` → 403 Forbidden 租户冻结拦截
       - `query.role == admin` → 200 OK 返回管理员完整权限列表
       - `query.role == member` → 200 OK 返回普通成员权限列表
       - `@mock-default` → 200 OK 默认访客角色
   - **纯契约定义 (`POST /api/v1/tenants/users`)**：
     - 无 `@test` 标记，用于验证 `rupost test` 模式下**自动跳过执行**，而在 `rupost mock` 模式下提供服务。
     - 包含缺失 Header（400）与邮箱冲突（409）分支。

2. **`iam_test_suite.http`（执行态测试工件）**：
   - 覆盖管理员查询、普通成员查询、冻结拦截、默认角色、成员创建、409 冲突与 400 校验。

3. **`openapi_iam.json`（参考规范输入源）**：
   - 标准 OpenAPI 3.0.3 格式定义，为后续双向引导生成（`import openapi`）提供黄金对标基准。

---

## 联调与回归验证操作

### 1. 启动 Mock 契约服务
```bash
cargo run -- mock examples/scenario_iam_tenant/iam_spec.md --port 9001
```

### 2. 运行自动化测试套件
在另一终端运行：
```bash
cargo run -- test examples/scenario_iam_tenant/iam_test_suite.http
```

### 3. 验证设计态文档（验证纯契约跳过机制）
```bash
cargo run -- test examples/scenario_iam_tenant/iam_spec.md
```
