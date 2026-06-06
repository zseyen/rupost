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
