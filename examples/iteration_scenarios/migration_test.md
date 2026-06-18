# @depends-on v1_api.md
# @depends-on v2_api_evolution.md

# API 迭代迁移与兼容性集成测试用例

本文件用于对运行在 Localhost 9000 端口上的订单 Mock 服务（编译自上述两份契约）进行跨版本集成验证。

## @test 验证 V1 老客户端平滑兼容性
老设备仍然调用 v1 接口，确保能获取数据且带有兼容字段。

```http
@name test-v1-compatibility-flow
@test
GET http://localhost:9000/api/v1/orders/202?status=completed
@assert status == 200
@assert body.compat_mode == true
@assert body.payment_method == legacy
```

## @test 验证 V2 客户端版本不匹配时被拒绝
新设备如果缺少 X-App-Version 头，或者版本不对，应该被 Mock 服务拒绝。

```http
@name test-v2-missing-version-header
@test
POST http://localhost:9000/api/v2/orders
Content-Type: application/json

{
  "amount": 150.0,
  "payment_method": "wechat"
}

@assert status == 400
@assert body.error contains Upgrade required
```

## @test 验证 V2 正常创建流程
当携带正确的 X-App-Version 时，允许创建订单。

```http
@name test-v2-create-success
@test
POST http://localhost:9000/api/v2/orders
X-App-Version: 2.0.0
Content-Type: application/json

{
  "amount": 150.0,
  "payment_method": "wechat"
}

@assert status == 201
@assert body.x_app_validated == true
@assert body.status == pending
```

---

## 📖 拓扑依赖合并与场景测试指南

本文件展示了 Rupost 强大的**多文件拓扑级联依赖合并 (`@depends-on`)** 特性。当进行 API 多版本并存演进与跨版本联调时，通过在文件首行声明依赖项，系统会自动构造依赖有向图并按照正确的顺序加载。

### 1. 声明级联依赖
在文件头部使用注释声明所需的基础契约：
```markdown
# @depends-on v1_api.md
# @depends-on v2_api_evolution.md
```
这样，不管是启动 Mock 服务还是运行自动化用例，Rupost 都会将这三份文件作为一个整体来解析和调度。

### 2. 启动联合 Mock 服务
您只需以 `migration_test.md` 作为入口文件，Rupost 的 Mock 引擎便会自动顺着依赖连带加载所有关联的 API 契约块：
```bash
# 传入这一个文件，自动拉起包含 V1 与 V2 完整接口的 Mock 拓扑路由树
rupost mock examples/iteration_scenarios/migration_test.md --port 9000
```
启动后终端会打印出 `initialized with 4 routes`。这证明虽然 `migration_test.md` 本身没有声明 `@mock`，但其依赖的 `v1_api.md` 与 `v2_api_evolution.md` 的所有 Mock 变体已经被完整联合编译并拉起。

### 3. 一键回归测试链 (拓扑运行)
同样，您可以使用下述命令，让测试运行器按照正确的拓扑顺序一键回归运行这一系列级联契约中的全部测试：
```bash
rupost test examples/iteration_scenarios/migration_test.md
```
Rupost 会自动按依赖图计算出执行序列（通常是：`v1_api.md` -> `v2_api_evolution.md` -> `migration_test.md`），执行这三个文件中的所有断言并输出最终批处理报告。
```
