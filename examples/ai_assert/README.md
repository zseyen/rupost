# AI 断言生成 (`rupost ai assert`)

用一句自然语言生成可直接粘贴进 `.http`/`.md` 文件的标准 `@assert` 断言行。
AI 只在创作期参与，生成的断言是普通文本，`rupost t` 离线确定性执行——运行时零 AI 依赖。

## 快速开始

### 1. 配置任意 OpenAI 兼容后端

```bash
export RUPOST_AI_API_KEY=sk-...                           # 必填（或 OPENAI_API_KEY）
export RUPOST_AI_BASE_URL=https://api.siliconflow.cn/v1   # 可选，默认 https://api.openai.com/v1
export RUPOST_AI_MODEL=deepseek-ai/DeepSeek-V3            # 可选，默认 gpt-4o-mini
```

> 密钥仅从环境变量读取，不落盘、不进日志。本地 Ollama 用户设置
> `RUPOST_AI_BASE_URL=http://localhost:11434/v1` 即可。

### 2. 生成断言

```bash
rupost ai assert "登录成功并且响应里包含token"
# [✓] 已生成 2 条标准断言（粘贴到请求块末尾即可生效）:
# # @assert status == 200
# # @assert body.token exists

rupost ai assert "status is 200 and token exists" --raw
# 机器可读：仅输出断言行，可直接 >> 追加进测试文件
```

### 3. 零 Key 离线体验（dogfooding）

本目录的 `mock_ai.json` 让 rupost 自身的 mock 服务器充当 OpenAI 兼容后端：

```bash
# 终端 1：起一个假 AI 服务（返回固定的 chat completion 响应）
rupost mock examples/ai_assert/mock_ai.json --port 9001

# 终端 2：把 base_url 指向它
RUPOST_AI_BASE_URL=http://127.0.0.1:9001/v1 RUPOST_AI_API_KEY=demo \
  rupost ai assert "登录成功并且响应里包含token"
```

全程不需要任何真实 API Key——这正是 `tests/ai_assert_test.rs` 与
`tests/verify_features.sh` 步骤 7 的自动化验证方式。

## 上下文感知（P0）：让 AI 看见真实响应

### `--last N`：取最近 N 条历史作上下文

```bash
# 先真实跑一次请求（产生 history）
rupost test login.http

# AI 看着真实响应写断言（默认脱敏：token/password 等整体 [REDACTED]，4KB/条截断）
rupost ai assert --last 1 "验证登录成功且返回管理员身份"
```

### `--response <FILE>`：显式响应文件

```bash
rupost ai assert --response resp.json "验证分页字段齐全"
```

### `--no-mask`：显式关闭上下文脱敏（自查环境）

```bash
rupost ai assert --last 1 "..." --no-mask
# ⚠ --no-mask 已显式关闭脱敏：上下文中的敏感值将原样发送给 AI 端点，请确认合规
```

`--last` 与 `--response` 互斥；无上下文时行为与 P0 之前完全一致。

## B4 零配置链（环境变量 > ~/.rupost/ai.toml > 本地探测）

```bash
# ① 环境变量（最高优先，远程端点必填 Key）
export RUPOST_AI_API_KEY=sk-... RUPOST_AI_BASE_URL=https://api.siliconflow.cn/v1

# ② 用户级配置 ~/.rupost/ai.toml（本地端点免 Key；⚠ api_key 字段写入即被拒绝）
#    base_url = "http://localhost:11434/v1"
#    model    = "qwen2.5:7b"

# ③ 什么都没配：自动探测本地 Ollama（GET /v1/models，2s 超时）
rupost ai assert "status is 200"
# ℹ 未检测到显式 AI 配置，已使用本地探测端点 http://localhost:11434/v1（模型: qwen2.5:7b）
```

全部失败时输出三选一可执行指引；`RUPOST_AI_PROBE_URL` 为测试钩子（覆盖探测地址，不入用户文档）。

## 断言语法速查

AI 的输出被 system prompt 固化在 rupost 的 8 运算符 × 4 目标语法内：

- **目标**：`status`、`headers.<name>`、`body.<json_path>`、`response.time`
- **运算符**：`==` `!=` `>` `<` `>=` `<=` `contains` `exists`

AI 返回的内容会被严格按 JSON 数组解析，非法输出直接报错（引擎不猜、不执行）。
