# RuPost 手动测试指引（二进制实测版）

> **用途**：用编译产物 `target/release/rupost` 对全部功能做人工逐项验证。
> **约定**：`$RUPOST` = `./target/release/rupost`；测试一律在**临时工作区**进行（`mktemp -d` 后 `cd`），并用 `RUPOST_HISTORY_DIR` 隔离历史——不污染仓库与家目录。
> **退出码契约**（全文档通用）：全绿 `0` / 测试失败 `1` / 配置·解析·工件 I/O 错误 `2`。exit 2 三来源鉴别见 M18。
> **来源可信度**：全部命令与预期输出取自自动验证脚本（`tests/verify_features.sh`、`examples/run_all.sh`）与集成测试的实测行为；标注 ⚠ 的项为文档化行为、未在本轮实测，结果若有出入以 `--help` 为准。

---

## M0 环境准备与冒烟

```bash
cargo build --release
export RUPOST=./target/release/rupost
WS=$(mktemp -d "${TMPDIR:-/tmp}/manual_XXXXXX") && cd "$WS"
export RUPOST_HISTORY_DIR="$WS/history" && mkdir -p "$RUPOST_HISTORY_DIR"
$RUPOST --help          # 预期:显示用法;若装了插件命令,末尾出现 Extensions: 区块
$RUPOST --version       # 预期:版本号
```

---

## M1 请求客户端（内联模式）

### M1.1 HTTPie 风格 GET
```bash
python3 -m http.server 18800 --bind 127.0.0.1 &   # 任意本地 HTTP 服务即可
$RUPOST GET http://127.0.0.1:18800/
```
**预期**：高亮输出含 `HTTP/1.0 200`、响应头表、响应体、耗时瀑布（DNS/TCP/TTFB/传输分段）。
**⚠ 实测边界**：内联模式**不要混入 `-v/--verbose` 等旗标**——旗标存在会切换到 curl 解析器（方法 token 被当 URL,见 M1.3）。

### M1.2 curl 风格 POST + Header
```bash
echo 'hello rupost' > body.txt
$RUPOST POST http://127.0.0.1:18800/up -H 'X-Tag: manual' --raw 'hello'
```
**预期**：200；服务端（http.server 日志）可见 POST 与 `X-Tag` 头。

### M1.3 内联双风格边界（WP-P2c 修复后实测锁定）
```bash
$RUPOST GET http://127.0.0.1:18800/                # ✅ httpie 纯形式（无旗标）→ 200
$RUPOST GET http://127.0.0.1:18800/ --verbose      # ✅ 尾部旗标被容忍 → 200（WP-P2c 修复）
$RUPOST GET --verbose http://127.0.0.1:18800/      # ✅ 旗标夹中间 → 200（WP-P2c 修复）
$RUPOST --verbose GET http://127.0.0.1:18800/      # exit 2:"未知命令 '--verbose'"(根路由裁决序,旗标前置=非 rupost 形态)
```
**讲解（WP-P2c 修复）**：此前任何旗标存在都会误切 curl 解析器（方法 token 被当 URL 吞掉）。修复后解析器选择按**方法前置**判定——首个裸 token 为标准 HTTP 方法即走 httpie 解析器，尾部/中间旗标被容忍（verbose 输出未实现，忽略不阻断）；旗标**前置**（`--verbose GET …`）仍走根路由裁决（exit 2"未知命令"）——那是"非 rupost 形态"的诚实拒绝，AGENTS §5 的"全局参数位置契约"（全局旗标须在子命令前）维持不变。

---

## M2 文档测试引擎（.http/.md/DAG）

### M2.1 基础套件与断言
```bash
cat > suite.http <<'EOF'
### 登录
# @name login
# @assert status == 200
POST http://127.0.0.1:18800/login
Content-Type: application/json

{"user":"admin","pass":"x"}

@capture auth_token from body.token
EOF
# 配一个真登录端点(任选):用 python 起个带 token 的 JSON 服务,或直接用 M10 的 mock
$RUPOST t suite.http ; echo "exit=$?"
```
**预期**：`1 passed`，exit 0；失败时终端逐条列出断言差异且 exit 1。

### M2.2 DAG 依赖与并行
```bash
cat > 01_auth.http <<'EOF'
### login
POST http://127.0.0.1:18800/login
Content-Type: application/json

{"user":"admin"}

@capture auth_token from body.token
EOF
cat > 02_profile.http <<'EOF'
### @depends-on 01_auth.http
GET http://127.0.0.1:18800/profile
Authorization: Bearer {{auth_token}}
EOF
$RUPOST t 01_auth.http 02_profile.http --mode parallel
```
**预期**：DAG 先登录后 profile；01 的捕获变量单向克隆给 02。

### M2.3 单请求选择器 / fail-fast / 跳过
```bash
$RUPOST t suite.http --only login          # 只跑 @name=login(或 #1 序号)
$RUPOST t suite.http --only login --with-deps   # 连带 @depends-on 闭包
$RUPOST t suite.http --fail-fast           # 首个失败文件即停
$RUPOST t suite.http --only 999            # 预期:越界可读错误,exit 非 0
```

### M2.4 ⚠ 未求值契约（手动注意点）
请求**连接失败/超时**时断言不参与求值（M8 契约基线中表现为该条目缺失）——这是登记过的语义，不是缺陷。

---

## M3 变量与环境级联

### M3.1 五层级联优先级（从低到高）
`rupost.toml [environments.X]` → `.env` 文件 → 系统环境变量 → `--var-state` 落盘值 → **`--var` 显式传参（最高）**

```bash
cat > rupost.toml <<'EOF'
[environments.dev]
base_url = "http://127.0.0.1:18800"
test_key = "toml-val"
EOF
printf 'test_key = "env-val"\n' > .env
cat > probe.http <<'EOF'
### probe
# @assert status == 200
GET {{base_url}}/echo?key={{test_key}}
X-Test-Key: {{test_key}}
EOF
$RUPOST t probe.http --env dev ; echo "exit=$?"   # 需要能区分 key 的端点,见 M10
$RUPOST t probe.http --env dev --var test_key=cli-val   # 显式传参恒最高
```
**实测预期**（配合 M10 的语义判别 mock——变体只认 `X-Test-Key: env-val`，优先级失守即 403，不靠肉眼）：
- `--env dev`：**exit 0**（header=env-val 命中变体，证明 .env 真实压过 toml）；
- `--env dev --var test_key=cli-val`：**exit 1 + 403**（header 变为 cli-val——证明 `--var` 恒最高；403 即优先级生效的判别证据）。

### M3.2 未解析变量与 strict-vars
```bash
$RUPOST t probe.http --var base_url=http://127.0.0.1:18800 --strict-vars
# 若文档含未定义 {{nope}} → 拒绝外发 exit 非 0 并指名变量;默认(不加)则警告+字面量发送
```

---

## M4 断言系统

```bash
cat > asserts.http <<'EOF'
### 断言全家福
# @assert status == 200
# @assert headers.content-type contains "json"
# @assert body.items #0.name == "alice"
GET http://127.0.0.1:18800/api
EOF
$RUPOST t asserts.http
```
**预期**：逐条 PASS/FAIL 与 expected/actual 对照；改坏任一断言 → 该条 FAIL、整体 exit 1。
**注意**：`@assert` 在**响应已收到**时才求值；连接失败不求值（M2.4）。

---

## M5 报告

```bash
$RUPOST t suite.http --report json                       # stdout 纯 JSON(数组)
$RUPOST t suite.http --report json --report-file r.json  # 落盘
$RUPOST t suite.http --report junit --report-file r.xml  # JUnit(CI 消费)
$RUPOST t suite.http --report html --report-file r.html  # HTML
$RUPOST t suite.http --report-file r.json --report-file r.json   # 第二次:已存在 → 覆盖警告(stderr)仍写入
```
**预期**：json 报告含 `file_path/results[]`，每条含 `request_number/source_line/assertions/timing/warnings`；junit 含 `<testsuites>`；terminal 模式下失败块后输出 `path:line` 稳定定位。

---

## M6 历史与快照

```bash
$RUPOST h                          # 列表
$RUPOST h show <序号>              # 单条详情
$RUPOST h prune --days 1           # 清理
$RUPOST history export --last 1 -o snap.json    # 历史 → 快照(⚠ 不脱敏,勿外传)
```
**快照与脱敏**：
```bash
$RUPOST t suite.http --save-snapshot snap.json --mask-secrets --mask-secret-value "我的密码"
# 打开 snap.json:命中词表/显式值的字段应为 [REDACTED];掩码后 body_sha256 为掩码形态的验真摘要
```
**预期**：`masking: "secrets"`；重放该快照时出现“数据不完整”黄色警告（M7）。

---

## M7 故障复现包（incident）+ 完整性 + 重放闭环

### M7.1 产出
```bash
# 先起一个必失败的 mock(500):
$RUPOST mock <(echo '[{"method":"GET","path":"/boom","variants":[{"condition":null,"status":500}]}]') --port 18095 &
cat > pay.http <<'EOF'
### charge
# @assert status == 200
POST http://127.0.0.1:18095/charge
Authorization: Bearer demo-secret-token-xyz
Content-Type: application/json

{"amount":1}
EOF
$RUPOST t pay.http --incident-out ./inc1 --mask-secret-value demo-secret-token-xyz ; echo "exit=$?"
```
**预期**：exit 1；`./inc1/` 出现三文件；`manifest.json` 含 `artifact_digests`（恰 report/snapshot 两条，sha256 64 位小写 hex）；**三个文件 grep 不到明文 token**；目录权限 0700/文件 0600。
**完整性校验（接收方视角）**：
```bash
shasum -a 256 inc1/snapshot.json    # 与 manifest.artifact_digests 中 file=snapshot.json 的 sha256 一致
```
### M7.2 消费
```bash
$RUPOST r inc1/snapshot.json --target http://127.0.0.1:18095
# 预期:加载 1 条 → 掩码形态警告("数据不完整") → 重放(token 已掩码,命中 401 兜底也属正常语义)
$RUPOST ai triage --snapshot inc1/snapshot.json    # 规则版分诊,无密钥可用
```
### M7.3 边界
```bash
$RUPOST t pay.http --incident-out ./inc1     # 目录已存在 → exit 2,原目录原样
$RUPOST t ok.http --incident-out ./inc2      # 全绿 → 不建目录,stderr 说明
```

---

## M8 契约漂移基线（--contract-baseline）

```bash
$RUPOST t ok.http --contract-baseline base.json ; echo "exit=$?"    # 首跑:建基线
cat base.json        # 顶层两键 schema_version/contracts;条目恰 id/assertion/passed 三键;无任何响应值
# 修改后端使其 500 后复跑:
$RUPOST t ok.http --contract-baseline base.json ; echo "exit=$?"
# 预期:stderr 出现 "⚠ 契约漂移: 1 条…" 且基线随跑更新;exit 仍由测试结果决定(漂移不改退出码)
# 服务端宕机(全错运行)再跑:预期 "⚠ … 缺失(removed)" 且 **基线不被空结果覆写**
```
**反噬防护**：断言原文改变视同新增；teardown 不入基线；损坏 JSON 基线 → `[ERROR] 契约基线工件错误: …` + **exit 2**（见 M18）。

---

## M9 网络诊断

```bash
$RUPOST diagnose http://127.0.0.1:18800              # 条形瀑布 + 证书健康
$RUPOST diagnose http://127.0.0.1:18800 --report json | python3 -m json.tool
```
**预期**：DNS/TCP/TLS/TTFB 分段毫秒图；`--report json` 输出 DiagnosticsReport 结构。

---

## M10 Mock 服务器（HTTP 变体/SSE/WS 路由）

```bash
cat > mock.json <<'EOF'
[
 {"method":"GET","path":"/echo","variants":[
   {"condition":{"source":"Header","key":"X-Test-Key","operator":"Equals","expected_value":"env-val"},
    "status":200,"headers":{"Content-Type":"application/json"},"response_body":"{\"ok\":true}"},
   {"condition":null,"status":403,"headers":{},"response_body":"{\"err\":\"wrong key\"}"}]},
 {"method":"POST","path":"/api/auth/login","variants":[
   {"condition":null,"status":200,"headers":{"Content-Type":"application/json"},"response_body":"{\"token\":\"token-12345\"}"}]},
 {"method":"GET","path":"/api/user/profile","variants":[
   {"condition":{"source":"Header","key":"Authorization","operator":"Equals","expected_value":"Bearer token-12345"},
    "status":200,"headers":{"Content-Type":"application/json"},"response_body":"{\"status\":\"profile-loaded\"}"},
   {"condition":null,"status":401,"headers":{},"response_body":"{\"error\":\"Unauthorized\"}"}]},
 {"method":"GET","path":"/api/env-probe","variants":[
   {"condition":{"source":"Header","key":"X-Test-Key","operator":"Equals","expected_value":"env-val"},
    "status":200,"headers":{},"response_body":"{\"status\":\"env-ok\"}"},
   {"condition":null,"status":403,"headers":{},"response_body":"{\"error\":\"wrong test key\"}"}]}
]
EOF
$RUPOST mock mock.json --port 18800        # 前台保活;另开终端执行以下请求
curl -s http://127.0.0.1:18800/api/users/88 -H 'X-Role: Admin'     # 路径参数 :id 渲染
curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:18800/api/env-probe   # 兜底 403
```
**预期**：Trie 模糊匹配 + 变体条件评估 + 兜底；控制台有访问日志。M3 的优先级探针直接复用本 mock（`/api/env-probe` 只认 env-val）。

---

## M11 WebSocket

⚠ 建议直接跑仓库自带示例（已内置服务端剧本）：`examples/websocket.http` / `websocket.md`（`run_all.sh` 有对应条目）。
```bash
$RUPOST t examples/websocket.http        # SEND/EXPECT 剧本、软匹配断言
$RUPOST t examples/websocket.md
```
**预期**：WS 握手、逐帧断言、心跳保活正常退出；断言失败 → exit 1。

---

## M12 SSE

```bash
$RUPOST t examples/llm_and_sse/sse_demo.md --var base_url=http://127.0.0.1:18800
```
**预期**：`@sse` 流式断言（stream.llm.content contains …）、`@stream_to` 物理落盘文件生成；SSE 请求的失败定位（path:line）与普通请求一致。

---

## M13 插件体系

### M13.1 离线体检与列表
```bash
$RUPOST plugin validate examples/plugins/rupost-plugin-cmd-hello.json   # ✓ ready: cmd-hello v0.1.0
$RUPOST plugin list          # 三态:✓ ready / ⚠ pending(解释器缺失) / ✗ failed(处方)
$RUPOST plugin schema        # 四段 JSON(供 IDE 挂接,见指南 §3.0)
```
### M13.2 hook 型（请求管线）
```bash
mkdir -p ./.rupost/plugins
cp "$OLDPWD/examples/plugins/rupost-plugin-echo-sign"* ./.rupost/plugins/   # 路径按实际调整
$RUPOST plugin --allow-project-plugins list
$RUPOST t "$OLDPWD/examples/plugins/plugin_pipeline.http" --allow-project-plugins --var base_url=http://127.0.0.1:18800
```
**预期**：@sign 签名头真实注入（mock 精确变体 200）；缺插件默认阻断 exit 1；`?optional`+`--allow-missing-plugins` 降级放行；`--strict-plugins` 判败。
### M13.3 command 型（CLI 工具扩展）
```bash
cp examples/plugins/rupost-plugin-cmd-hello* ./.rupost/plugins/ && chmod 755 ./.rupost/plugins/rupost-plugin-cmd-hello
$RUPOST plugin --allow-project-plugins list      # cmd-hello ready
$RUPOST --allow-project-plugins cmd-hello a b    # 首跑 TOFU 交互确认 y;输出 args/workspace/rupost bin 三行
```
### M13.4 AST 桥
```bash
$RUPOST ast suite.http | python3 -m json.tool   # 顶层 4 键;请求级 method/url(原文)/headers(保序)/assertions/captures/source_line
$RUPOST ast doc.md | python3 -c "import json,sys; print(json.load(sys.stdin)['requests'][0]['source_line'])"
```

---

## M14 Web 工作台

```bash
$RUPOST web --host 127.0.0.1 --port 18900 --dir "$WS" &
curl -s http://127.0.0.1:18900/api/v1/meta                     # {"success":true,"data":{"readonly":false,...}}
curl -s -X POST http://127.0.0.1:18900/api/v1/run/block -H 'Content-Type: application/json' \
  -d '{"file_path":"login.http","raw_block":"### login\nPOST http://127.0.0.1:18800/api/auth/login\nContent-Type: application/json\n\n{}\n\n@capture auth_token from body.token\n"}' \
  -c jarA.txt      # 会话 A:登录+捕获;响应 Set-Cookie: rupost_session=…
curl -s -b jarA.txt -X POST http://127.0.0.1:18900/api/v1/run/block -H 'Content-Type: application/json' \
  -d '{"file_path":"p.http","raw_block":"### p\nGET http://127.0.0.1:18800/api/user/profile\nAuthorization: Bearer {{auth_token}}\n"}'   # ✅ A 延续成功
curl -s -c jarB.txt -X POST http://127.0.0.1:18900/api/v1/run/block -H 'Content-Type: application/json' \
  -d '{"file_path":"p.http","raw_block":"### p\nGET http://127.0.0.1:18800/api/user/profile\nAuthorization: Bearer {{auth_token}}\n"}'   # ❌ B 诚实 401(隔离)
```
**会话隔离（A1）**：A/B Cookie 互不串扰；首跑下发 Set-Cookie；30 分钟滑动续期。
**incident 接口**：套件面板对应 `POST /api/v1/run/suite` 带 `"incident": true` → 失败时响应 `incident_dir`（服务端管辖 `.rupost/incidents/`，32 上限最旧裁剪）。
**边界**：`--readonly` 禁执行与保存；`--dev` 仅回环绑定且警告；项目级插件在 Extensions 区块需 `RUPOST_ALLOW_PROJECT_PLUGINS=1` 环境通道。

---

## M15 TUI 冒烟

```bash
$RUPOST tui suite.http      # 打开后:q 退出;Tab/方向键切焦;Enter 执行;X 导出 curl
```
**预期**：正常启动退出无 "Cannot start a runtime from within a runtime"；响应面板四段耗时；历史 Tab 可反向还原。

---

## M16 导入与生成

```bash
OUT=$(mktemp /tmp/imp_XXXX); rm -f $OUT
$RUPOST import curl "curl -X POST 'https://api.example.com/login?next=/home' -H 'Content-Type: application/json' -H 'Authorization: Bearer eyJdemo' --data-raw '{\"user\":\"admin\"}'" -o $OUT
cat $OUT     # POST https://api.example.com/login?next=/home;authorization 模板化 {{authorization}};# @assert status == 200
$RUPOST import curl "curl -X POST 'https://api.example.com/login' -o $OUT"    # 已存在 → 拒绝(防覆盖)
$RUPOST g --last 1 -o gen.http && head gen.http      # 历史 → 用例;默认凭据模板化
```

---

## M17 AI 能力（离线降级路径；无密钥可测）

```bash
# ai assert:无密钥 → exit 1 + 可执行指引(自建网关/ai.toml/Ollama)
env -u RUPOST_AI_API_KEY -u OPENAI_API_KEY RUPOST_AI_PROBE_URL=http://127.0.0.1:1 $RUPOST ai assert "登录成功" ; echo "exit=$?"
# ai triage:规则版确定性降级(无密钥可用)
$RUPOST ai triage <失败报告.json> --json | grep '"category": "Auth"'
# ai resolve:B2 降级——文件原样 + exit 1
printf '### login\nPOST {{base_url}}/login\n\n# @ai-assert 登录成功\n' > r.http
env -u RUPOST_AI_API_KEY -u OPENAI_API_KEY RUPOST_AI_PROBE_URL=http://127.0.0.1:1 $RUPOST ai resolve r.http ; echo "exit=$?"
# ai bootstrap:机械基线降级,exit 0
RUPOST_HISTORY_DIR=$RUPOST_HISTORY_DIR $RUPOST ai bootstrap --from history --last 1 -o boot.http --no-ai
```
**完整闭环（可选）**：`$RUPOST mock examples/ai_assert/mock_ai.json --port 18901 &` 后 `RUPOST_AI_BASE_URL=http://127.0.0.1:18901/v1 RUPOST_AI_API_KEY=demo $RUPOST ai assert "登录成功并且响应里包含token"`——离线 dogfooding。

---

## M18 退出码与安全红线（横向）

### 退出码
| 场景 | exit | stderr 鉴别 |
|---|---|---|
| 全绿 | 0 | — |
| 测试/断言失败 | 1 | 报告正文 |
| clap 参数错误 | 2 | `error: unexpected argument ...` |
| 解析错误 | 2 | `[ERROR] 解析错误: ...` |
| 复现包/契约基线 I/O | 2 | `[ERROR] 故障复现包创建失败: ...` / `[ERROR] 契约基线工件错误: ...` |
### 安全红线抽查
- 掩码三表面：快照/报告/终端均无明文凭据（M6/M7 的 grep 抽查）；
- 历史文件 0600；快照 0600、复现包目录 0700；
- 插件 TOFU：非交互首跑 fail-closed；`~/.rupost/tofu-commands.txt` 0600；
- Web：Host 守卫（非白名单 403）、readonly 禁执行、`--dev` 强制回环。

---

## 附：测试矩阵勾选表

| 模块 | 关键验证点 | 通过 |
|---|---|---|
| M1 | GET/POST/-v/参数位置契约 | ☐ |
| M2 | 断言/捕获/DAG/并行/--only/--fail-fast | ☐ |
| M3 | 五层级联/strict-vars | ☐ |
| M4 | 断言全家福逐条 | ☐ |
| M5 | json/junit/html/覆盖警告/path:line | ☐ |
| M6 | 历史四命令/快照/双掩码 | ☐ |
| M7 | incident 三文件/digest 校验/replay/目录保护/exit 2 | ☐ |
| M8 | 基线两连跑/漂移三态/空覆写守卫/exit 2 | ☐ |
| M9 | diagnose 瀑布/json | ☐ |
| M10 | Trie/变体/兜底 | ☐ |
| M11-M12 | WS/SSE 剧本 | ☐ |
| M13 | validate/list/schema/hook/command/TOFU/降级/ast | ☐ |
| M14 | meta/会话隔离/incident 接口/readonly | ☐ |
| M15 | TUI 启动退出 | ☐ |
| M16 | import 防覆盖/g 模板化 | ☐ |
| M17 | 四条 AI 降级路径 | ☐ |
| M18 | 退出码表/安全红线抽查 | ☐ |
