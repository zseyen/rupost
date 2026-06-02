# RuPost Sprint 1 实战功能测试用例与接口文档

本 Markdown 文档既是一份接口参考，也是可被 RuPost 直接运行的测试脚本！
在终端中执行以下命令，开启高精度的细粒度网络诊断：

```bash
# 执行本 MD 里的所有测试，并强制打印网络诊断时序条形图
rupost test examples/sprint1_diagnostics_demo.md --debug

# 或者在执行时配合多环境，观察本地 Cookie 物理文件的跨环境隔离：
rupost test examples/sprint1_diagnostics_demo.md -e dev --cookie-file my_cookies.json
rupost test examples/sprint1_diagnostics_demo.md -e prod --cookie-file my_cookies.json
```

---

## 1. 模拟登录并写入 Session Cookie

本请求通过 GET 方法向客户端下发一个 `session_id` Cookie。
在配合不同的 `--env` (如 `dev`、`prod`) 执行时，您可以检查本地生成的 Cookie 文件：
*   `-e dev` 会生成 `my_cookies_dev.json`
*   `-e prod` 会生成 `my_cookies_prod.json`

```http
GET {{base_url}}/cookies/set?session_id=sprint1_token
Accept: application/json

@assert status == 200
@assert body.cookies.session_id == "sprint1_token"
```

---

## 2. 模拟高延迟后端服务 (时序诊断测试)

本请求会触发后端服务器强制延迟 1.5 秒后响应。
开启 `--debug` 参数后，您将看到系统细化拆分出的时序条形图，轻松找出性能瓶颈究竟是在 DNS、TCP 握手，还是在后端处理（TTFB）阶段：

```http
GET {{base_url}}/delay/1.5
Accept: application/json

@assert status == 200
@assert response.time >= 1500
@assert response.time < 3000
```
