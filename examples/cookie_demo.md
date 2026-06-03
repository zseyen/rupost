# Rupost Cookie 自动管理示例 (Cookie Management Demo)

本文件演示了 RuPost 在执行 Markdown API 测试文件时的 **Cookie 自动传递和状态保持机制**。

在同一个文件的串联执行流中，RuPost 默认会开启一个内存会话（Ephemeral Cookie Store）。当前一个请求遇到服务端的 `Set-Cookie` 头时，后续请求会自动携带该 Cookie，无须任何手动操作。

---

## 1. 模拟用户登录（下发第一个 Cookie）

我们向服务端的 `set` 接口发起请求，这会返回一个名为 `session_token` 的 Cookie。

```http
@name set-first-cookie
@assert status == 200
GET {{base_url}}/cookies/set?session_token=secret_val_123
```

## 2. 验证会话（自动携带）

接下来，向 `/cookies` 接口发送请求，服务端会反射返回接收到的所有 Cookie。我们使用断言（`@assert`）来验证 `session_token` 已被自动携带：

```http
@name verify-first-cookie
@assert status == 200
@assert body.cookies.session_token == "secret_val_123"
GET {{base_url}}/cookies
```

## 3. 追加第二个 Cookie（模拟购物车或多因子认证流程）

我们在不清除已有会话的情况下，向接口设置另一个 Cookie：`user_id`。

```http
@name set-second-cookie
@assert status == 200
GET {{base_url}}/cookies/set?user_id=usr_999
```

## 4. 终极验证（多 Cookie 共存）

最后，我们再次验证 `/cookies`。预期中，因为是在同一个会话期内，新旧两个 Cookie 应同时存在且互不冲突：

```http
@name verify-all-cookies
@assert status == 200
@assert body.cookies.session_token == "secret_val_123"
@assert body.cookies.user_id == "usr_999"
GET {{base_url}}/cookies
```

---

## 5. 如何执行此测试？

您可以使用 CLI 直接执行此 Markdown 测试文件，默认会开启 Ephemeral 内存隔离模式：

```bash
# 默认内存模式执行
rupost test examples/cookie_demo.md

# 如果需要将登录状态保存到本地，以便下一次运行，可以指定 cookie 文件：
rupost test examples/cookie_demo.md --cookie-file my_session.json

# 如果想要完全无 Cookie 执行（无状态请求验证）：
rupost test examples/cookie_demo.md --no-cookies

```
