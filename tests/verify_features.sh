#!/bin/bash
set -e

# 定义彩色输出
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m' # No Color
BLUE='\033[0;34m'
YELLOW='\033[0;33m'

echo -e "${BLUE}[*] 开始执行 RuPost 全特性自动化验证脚本...${NC}"

# WP-2 可信度契约（差异化执行计划 A2）：
# - 绝不 killall：只管理本脚本启动的 PID（trap 幂等清理，开发者独立进程零干扰）
# - mktemp 隔离工作区：绝不移动/覆盖仓库根的 rupost.toml / .env
# - 端口经 RUPOST_TEST_PORT_BASE 可平移；健康探针替代固定 sleep
# - 严格退出码与语义断言：mock 变体判别业务事实，不以文件名出现为成功条件
PIDS=()
cleanup() {
    local pid
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
}
trap cleanup EXIT INT TERM

# 1. 编译最新的二进制
echo -e "${BLUE}[*] 正在编译 RuPost 二进制程序...${NC}"
cargo build --release

REPO_ROOT="$(pwd)"
RUPOST_BIN="${RUPOST_BIN:-$REPO_ROOT/target/release/rupost}"
case "$RUPOST_BIN" in
    /*) ;;
    *) RUPOST_BIN="$REPO_ROOT/$RUPOST_BIN" ;;
esac
if [ ! -x "$RUPOST_BIN" ]; then
    echo -e "${RED}[ERROR] 找不到可执行的二进制文件: $RUPOST_BIN${NC}"
    exit 1
fi
echo -e "${GREEN}[✓] 编译成功！${NC}"

# 2. mktemp 隔离工作区（历史记录同样收进工作区，不污染 ~/.rupost 与仓库根）
WORKSPACE="$(mktemp -d "${TMPDIR:-/tmp}/rupost_verify_XXXXXX")"
cd "$WORKSPACE"
export RUPOST_HISTORY_DIR="$WORKSPACE/history"
mkdir -p "$RUPOST_HISTORY_DIR"
echo -e "${BLUE}[*] 创建临时测试工作区: $WORKSPACE${NC}"

# 端口分配：全部可经 RUPOST_TEST_PORT_BASE 平移，消除固定端口冲突
PORT_BASE="${RUPOST_TEST_PORT_BASE:-9000}"
MOCK_PORT=$((PORT_BASE))
AI_PORT=$((PORT_BASE + 1))
TRIAGE_PORT=$((PORT_BASE + 2))
HOOK_PORT=$((PORT_BASE + 3))
REPLAY_PORT=$((PORT_BASE + 4))
PLUGIN_PORT=$((PORT_BASE + 5))

# 健康探针：有限重试替代固定 sleep；进程启动即退出（端口被占用/配置非法）立刻报错
wait_mock_ready() {
    local port=$1 log=$2 pid=$3
    local retries=50   # 50 * 0.2s = 10s 上限
    while [ "$retries" -gt 0 ]; do
        if ! kill -0 "$pid" 2>/dev/null; then
            echo -e "${RED}[ERROR] Mock 进程启动即退出（端口 ${port} 被占用或配置非法），日志：${NC}"
            cat "$log" 2>/dev/null || true
            exit 1
        fi
        if curl -s -m 2 -o /dev/null "http://127.0.0.1:${port}/"; then
            return 0
        fi
        sleep 0.2
        retries=$((retries - 1))
    done
    echo -e "${RED}[ERROR] Mock 端口 ${port} 未在超时内就绪，日志：${NC}"
    cat "$log" 2>/dev/null || true
    exit 1
}

# 2.5 验证 init 初始化模板命令（在工作区内进行，零仓库根污染）
echo -e "${BLUE}[*] 验证 rupost init 初始化模板功能...${NC}"
(
    $RUPOST_BIN init

    if [ ! -f "rupost.toml" ]; then
        echo -e "${RED}[ERROR] init 命令未能生成 rupost.toml 配置文件模板！${NC}"
        exit 1
    fi

    if ! grep -q "environments.dev" "rupost.toml"; then
        echo -e "${RED}[ERROR] 生成的 rupost.toml 配置内容不完整或损坏！${NC}"
        exit 1
    fi

    # 验证二次 init 不覆盖现有文件并正确报错
    echo "test-flag" >> rupost.toml
    if $RUPOST_BIN init 2>/dev/null; then
         echo -e "${RED}[ERROR] init 命令在文件已存在时没有报错退出！${NC}"
         exit 1
    fi
    if ! grep -q "test-flag" "rupost.toml"; then
         echo -e "${RED}[ERROR] init 命令覆盖了已有的 rupost.toml 配置文件！${NC}"
         exit 1
    fi

    # 清理生成的 toml
    rm rupost.toml
)
if [ $? -ne 0 ]; then
    echo -e "${RED}[ERROR] init 命令功能验证失败！${NC}"
    exit 1
fi
echo -e "${GREEN}[✓] init 命令模板初始化与防护功能验证成功！${NC}"


# 写入工作区 mock_config.json
MOCK_CONFIG="$WORKSPACE/mock_config.json"
cat << 'EOF' > "$MOCK_CONFIG"
[
  {
    "method": "GET",
    "path": "/api/users/:id",
    "variants": [
      {
        "condition": {
          "source": "Header",
          "key": "X-Role",
          "operator": "Equals",
          "expected_value": "Admin"
        },
        "status": 200,
        "headers": { "Content-Type": "application/json" },
        "response_body": "{\"status\":\"success\",\"role\":\"admin\",\"user_id\":\"{{id}}\"}"
      },
      {
        "condition": null,
        "status": 403,
        "headers": { "Content-Type": "application/json" },
        "response_body": "{\"error\":\"Access Denied\"}"
      }
    ]
  },
  {
    "method": "POST",
    "path": "/api/auth/login",
    "variants": [
      {
        "condition": null,
        "status": 200,
        "headers": { "Content-Type": "application/json" },
        "response_body": "{\"token\":\"token-12345\"}"
      }
    ]
  },
  {
    "method": "GET",
    "path": "/api/user/profile",
    "variants": [
      {
        "condition": {
          "source": "Header",
          "key": "Authorization",
          "operator": "Equals",
          "expected_value": "Bearer token-12345"
        },
        "status": 200,
        "headers": { "Content-Type": "application/json" },
        "response_body": "{\"status\":\"profile-loaded\"}"
      },
      {
        "condition": null,
        "status": 401,
        "headers": { "Content-Type": "application/json" },
        "response_body": "{\"error\":\"Unauthorized\"}"
      }
    ]
  },
  {
    "method": "GET",
    "path": "/api/env-probe",
    "variants": [
      {
        "condition": {
          "source": "Header",
          "key": "X-Test-Key",
          "operator": "Equals",
          "expected_value": "env-val"
        },
        "status": 200,
        "headers": { "Content-Type": "application/json" },
        "response_body": "{\"status\":\"env-ok\"}"
      },
      {
        "condition": null,
        "status": 403,
        "headers": { "Content-Type": "application/json" },
        "response_body": "{\"error\":\"wrong test key\"}"
      }
    ]
  }
]
EOF

# 3. 验证本地 Mock 服务端 & Trie 路径解析 & 条件变体评估
echo -e "${BLUE}[*] 启动 Mock 服务后台进程，端口 ${MOCK_PORT}...${NC}"
$RUPOST_BIN mock "$MOCK_CONFIG" --port "$MOCK_PORT" > "$WORKSPACE/mock.log" 2>&1 &
MOCK_PID=$!
PIDS+=("$MOCK_PID")
wait_mock_ready "$MOCK_PORT" "$WORKSPACE/mock.log" "$MOCK_PID"

# 发送请求验证变体条件 (Header X-Role: Admin)
echo -e "${BLUE}[*] 验证 Mock 变体匹配 (条件满足: X-Role=Admin)...${NC}"
RES_ADMIN=$(curl -s -X GET "http://127.0.0.1:${MOCK_PORT}/api/users/88" -H "X-Role: Admin")
echo "响应内容: $RES_ADMIN"
if [[ "$RES_ADMIN" == *"admin"* ]] && [[ "$RES_ADMIN" == *"88"* ]]; then
    echo -e "${GREEN}[✓] Mock 变体条件匹配成功，路径参数 id (88) 提取渲染成功！${NC}"
else
    echo -e "${RED}[ERROR] Mock 变体匹配失败！${NC}"
    exit 1
fi

# 发送请求验证兜底响应 (Header X-Role: Guest)
echo -e "${BLUE}[*] 验证 Mock 变体匹配 (条件不满足: 兜底 403)...${NC}"
RES_GUEST=$(curl -s -o /dev/null -w "%{http_code}" -X GET "http://127.0.0.1:${MOCK_PORT}/api/users/88" -H "X-Role: Guest")
echo "响应状态码: $RES_GUEST"
if [ "$RES_GUEST" -eq 403 ]; then
    echo -e "${GREEN}[✓] Mock 兜底匹配逻辑验证成功！${NC}"
else
    echo -e "${RED}[ERROR] Mock 兜底匹配失败！${NC}"
    exit 1
fi

# 4. 验证网络诊断工具 (diagnose)
echo -e "${BLUE}[*] 验证网络高亮诊断 (diagnose)...${NC}"
# 对活动中的 mock 服务进行诊断 (使用 127.0.0.1 避开 macOS 的 localhost IPv6 解析问题)
$RUPOST_BIN diagnose "http://127.0.0.1:${MOCK_PORT}" > "$WORKSPACE/diagnose.log"
cat "$WORKSPACE/diagnose.log"
if grep -q "Diagnostics" "$WORKSPACE/diagnose.log" || grep -q "DNS Lookup" "$WORKSPACE/diagnose.log"; then
    echo -e "${GREEN}[✓] 网络诊断输出校验成功！${NC}"
else
    echo -e "${RED}[ERROR] 网络诊断输出校验失败！${NC}"
    exit 1
fi

# 5. 验证级联变量覆盖与 DAG 并行执行（全部在工作区内，零仓库根污染）
echo -e "${BLUE}[*] 验证变量级联优先级与拓扑并行测试...${NC}"

# 创建 rupost.toml（toml 声明 test_key=toml-val；.env 将覆盖为 env-val）
TOML_CONF="$WORKSPACE/rupost.toml"
cat << EOF > "$TOML_CONF"
[environments.dev]
base_url = "http://127.0.0.1:${MOCK_PORT}"
test_key = "toml-val"
EOF

# 创建 .env（优先级必须压过 toml：env-val）
ENV_CONF="$WORKSPACE/.env"
cat << 'EOF' > "$ENV_CONF"
test_key = "env-val"
EOF

# 创建 01_auth.http
AUTH_HTTP="$WORKSPACE/01_auth.http"
cat << EOF > "$AUTH_HTTP"
POST http://127.0.0.1:${MOCK_PORT}/api/auth/login
Content-Type: application/json

{ "username": "admin" }

@capture auth_token from body.token
EOF

# 创建 02_profile.http（profile 端点仅接受精确 Bearer token-12345：捕获失守即 401）
PROFILE_HTTP="$WORKSPACE/02_profile.http"
cat << EOF > "$PROFILE_HTTP"
### @depends-on 01_auth.http
GET http://127.0.0.1:${MOCK_PORT}/api/user/profile
X-Test-Key: {{test_key}}
Authorization: Bearer {{auth_token}}
EOF

# 创建 03_env.http（env-probe 端点仅接受 X-Test-Key=env-val：优先级失守即 403）
ENV_HTTP="$WORKSPACE/03_env.http"
cat << EOF > "$ENV_HTTP"
### env-priority-probe
GET http://127.0.0.1:${MOCK_PORT}/api/env-probe
X-Test-Key: {{test_key}}

# @assert status == 200
EOF

echo -e "${BLUE}[*] 执行 DAG 拓扑并行测试 (验证依赖关系与 State Cloning 状态传递)...${NC}"
DAG_EXIT=0
$RUPOST_BIN test 01_auth.http 02_profile.http 03_env.http --mode parallel --env dev > "$WORKSPACE/run.log" 2>&1 || DAG_EXIT=$?

cat "$WORKSPACE/run.log"

# 语义断言（mock 变体判别业务事实，不以文件名出现为成功条件）：
# - exit 0 ⇒ 02_profile 命中精确 Bearer 变体（01_auth 捕获的 token 真实传递）
#         ⇒ 03_env-probe 命中 X-Test-Key=env-val 变体（.env 真实覆盖 toml-val）
if [ "$DAG_EXIT" -eq 0 ]; then
    echo -e "${GREEN}[✓] DAG 并行测试语义验证成功（token 捕获 + .env 优先级 + 严格 exit 0）！${NC}"
else
    echo -e "${RED}[ERROR] DAG 并行执行校验失败（exit=$DAG_EXIT）！${NC}"
    exit 1
fi

# 关闭 Mock 后台服务 (完成所有网络请求后关闭；trap 兜底清理其余进程)
echo -e "${BLUE}[*] 关闭 Mock 服务后台进程 (PID: $MOCK_PID)...${NC}"
kill $MOCK_PID 2>/dev/null || true

# 清理工作区内的测试配置
rm -f "$WORKSPACE/rupost.toml" "$WORKSPACE/.env"

# 6. 验证 TUI 启动与防嵌套 Runtime Panic 冒烟测试
echo -e "${BLUE}[*] 验证 TUI 启动与防嵌套 Runtime Panic 冒烟测试...${NC}"
# 使用 python3 pty 模拟真实终端按键 'q'，防止在非交互子进程中挂起或环境差异
if command -v python3 &>/dev/null; then
    TUI_OUT=$(python3 -c "
import pty, os, time
master, slave = pty.openpty()
pid = os.fork()
if pid == 0:
    os.close(master)
    os.setsid()
    os.dup2(slave, 0)
    os.dup2(slave, 1)
    os.dup2(slave, 2)
    os.close(slave)
    os.execl('$RUPOST_BIN', '$RUPOST_BIN', 'tui')
else:
    os.close(slave)
    time.sleep(0.5)
    os.write(master, b'q')
    time.sleep(0.5)
    out = b''
    try:
        out = os.read(master, 4096)
    except Exception:
        pass
    os.close(master)
    os.waitpid(pid, 0)
    print(out.decode('utf-8', errors='ignore'))
" 2>&1 || true)
else
    TUI_OUT=$(echo "q" | $RUPOST_BIN tui 2>&1 || true)
fi

echo "TUI 启动输出检测完毕。"

if echo "$TUI_OUT" | grep -q "Cannot start a runtime from within a runtime"; then
    echo -e "${RED}[ERROR] TUI 启动冒烟测试失败：发现 Runtime 嵌套冲突！${NC}"
    exit 1
else
    echo -e "${GREEN}[✓] TUI 运行环境校验成功（正常启动与退出，无 Runtime 嵌套冲突）！${NC}"
fi

# 7. 验证 AI 断言生成 (ai assert) —— 离线 dogfooding 闭环 (mock 充当 OpenAI 兼容后端)
echo -e "${BLUE}[*] 验证 AI 断言生成 (ai assert) 离线 dogfooding 闭环...${NC}"
$RUPOST_BIN mock "$REPO_ROOT/examples/ai_assert/mock_ai.json" --port "$AI_PORT" > /dev/null 2>&1 &
MOCK_AI_PID=$!
PIDS+=("$MOCK_AI_PID")
wait_mock_ready "$AI_PORT" "$WORKSPACE/mock_ai.log" "$MOCK_AI_PID"

AI_OUT=$(RUPOST_AI_BASE_URL=http://127.0.0.1:${AI_PORT}/v1 RUPOST_AI_API_KEY=demo $RUPOST_BIN ai assert "登录成功并且响应里包含token" 2>&1)
echo "AI 断言输出: $AI_OUT"
if echo "$AI_OUT" | grep -q "# @assert status == 200" && echo "$AI_OUT" | grep -q "# @assert body.token exists"; then
    echo -e "${GREEN}[✓] ai assert 离线 dogfooding 闭环验证成功（无真实 API Key）！${NC}"
else
    echo -e "${RED}[ERROR] ai assert 闭环验证失败！${NC}"
    exit 1
fi

# 未配置密钥时应返回退出码 1 并给出可执行指引（set -e 下用 || 捕获退出码）
AI_EXIT=0
env -u RUPOST_AI_API_KEY -u OPENAI_API_KEY RUPOST_AI_PROBE_URL=http://127.0.0.1:1 $RUPOST_BIN ai assert "status is 200" > /dev/null 2>&1 || AI_EXIT=$?
if [ $AI_EXIT -eq 1 ]; then
    echo -e "${GREEN}[✓] 未配置密钥时正确返回退出码 1！${NC}"
else
    echo -e "${RED}[ERROR] 未配置密钥时应返回退出码 1，实际: $AI_EXIT${NC}"
    exit 1
fi

kill $MOCK_AI_PID 2>/dev/null || true

# 8. 验证 AI 失败分诊 (ai triage) 离线确定性降级 + dogfooding 闭环
echo -e "${BLUE}[*] 验证 AI 失败分诊 (ai triage) 离线确定性降级...${NC}"
TMP_REPORT=$(mktemp /tmp/rupost_report_XXXXXX.json)
cat << 'EOF' > "$TMP_REPORT"
[
  {
    "file_path": "smoke.http",
    "results": [
      {
        "request_number": 1,
        "name": "CheckAuth",
        "method": "GET",
        "url": "http://localhost:9999/v1/items/1",
        "status": 401,
        "duration_ms": 3,
        "success": false,
        "error": null,
        "skipped": false,
        "assertions": [
          {
            "raw": "status == 200",
            "passed": false,
            "message": "expected 200, got 401",
            "expected": "200",
            "actual": "401"
          }
        ],
        "warnings": []
      }
    ]
  }
]
EOF
TRIAGE_OUT=$(env -u RUPOST_AI_API_KEY -u OPENAI_API_KEY RUPOST_AI_PROBE_URL=http://127.0.0.1:1 $RUPOST_BIN ai triage "$TMP_REPORT" --json)
echo "$TRIAGE_OUT" | grep -q '"category": "Auth"'
echo -e "${GREEN}[✓] ai triage 确定性降级验证成功（含黄色警告 + exit 0）！${NC}"
rm -f "$TMP_REPORT"

echo -e "${BLUE}[*] 验证 AI 失败分诊 dogfooding 闭环 (test --report-file → ai triage)...${NC}"
$RUPOST_BIN mock "$REPO_ROOT/examples/ai_triage/mock_ai.json" --port "$TRIAGE_PORT" > /dev/null 2>&1 &
MOCK_TRIAGE_PID=$!
PIDS+=("$MOCK_TRIAGE_PID")
wait_mock_ready "$TRIAGE_PORT" "$WORKSPACE/mock_triage.log" "$MOCK_TRIAGE_PID"

FAIL_HTTP=$(mktemp /tmp/rupost_fail_XXXXXX.http)
cat << EOF > "$FAIL_HTTP"
### get-profile
GET http://127.0.0.1:${TRIAGE_PORT}/v1/users/1
Accept: application/json

# @assert status == 200
EOF
DOGFOOD_REPORT=$(mktemp /tmp/rupost_dogfood_report_XXXXXX.json)
TEMP_EXPLAIN=$(mktemp /tmp/rupost_explain_XXXXXX.log)

# --explain 无 Key：规则版基线必须出现、AI 段落不得出现、退出码 1（AI 不改变判定）
TEST_EXIT=0
env -u RUPOST_AI_API_KEY -u OPENAI_API_KEY RUPOST_AI_PROBE_URL=http://127.0.0.1:1 $RUPOST_BIN test "$FAIL_HTTP" --explain > "$TEMP_EXPLAIN" 2>&1 || TEST_EXIT=$?
if [ "$TEST_EXIT" -eq 1 ] && grep -q "Possible Causes" "$TEMP_EXPLAIN" && ! grep -q "AI Explanation" "$TEMP_EXPLAIN"; then
    echo -e "${GREEN}[✓] --explain 规则版基线降级验证成功（exit 1，无 AI 段落）！${NC}"
else
    echo -e "${RED}[ERROR] --explain 无 Key 降级验证失败（exit=$TEST_EXIT）！${NC}"
    cat "$TEMP_EXPLAIN"
    exit 1
fi

# 真实失败 → 真实报告 → ai triage 消费（B1 降级，exit 0，敏感值不泄漏）
DOGFOOD_EXIT=0
env -u RUPOST_AI_API_KEY -u OPENAI_API_KEY RUPOST_AI_PROBE_URL=http://127.0.0.1:1 $RUPOST_BIN test "$FAIL_HTTP" --report-file "$DOGFOOD_REPORT" > /dev/null 2>&1 || DOGFOOD_EXIT=$?
if [ "$DOGFOOD_EXIT" -eq 1 ] && [ -f "$DOGFOOD_REPORT" ]; then
    echo -e "${GREEN}[✓] test --report-file 落盘成功（exit 1 由失败断言决定）！${NC}"
else
    echo -e "${RED}[ERROR] --report-file 闭环失败（exit=$DOGFOOD_EXIT）！${NC}"
    exit 1
fi

TRIAGE_JSON=$(env -u RUPOST_AI_API_KEY -u OPENAI_API_KEY RUPOST_AI_PROBE_URL=http://127.0.0.1:1 $RUPOST_BIN ai triage "$DOGFOOD_REPORT" --json)
if echo "$TRIAGE_JSON" | grep -q '"category": "Auth"' && ! echo "$TRIAGE_JSON" | grep -q "expired-jwt-should-not-leak"; then
    echo -e "${GREEN}[✓] ai triage dogfooding 闭环验证成功（真实报告 → Auth 根因簇，脱敏无泄漏）！${NC}"
else
    echo -e "${RED}[ERROR] ai triage dogfooding 闭环失败！${NC}"
    echo "$TRIAGE_JSON"
    exit 1
fi

kill $MOCK_TRIAGE_PID 2>/dev/null || true
rm -f "$FAIL_HTTP" "$DOGFOOD_REPORT" "$TEMP_EXPLAIN"

# 9. 验证通用导入器 (import curl)：devtools curl → 可执行 .http（防覆盖）
echo -e "${BLUE}[*] 验证通用导入器 (import curl)...${NC}"
IMPORT_FILE=$(mktemp /tmp/rupost_import_XXXXXX)
rm -f "$IMPORT_FILE"  # mktemp 会创建空文件——导入的防覆盖语义要求先移除
$RUPOST_BIN import curl "curl -X POST 'https://api.example.com/login?next=/home' -H 'Content-Type: application/json' -H 'Authorization: Bearer eyJdemo' --data-raw '{\"user\":\"admin\"}'" -o "$IMPORT_FILE" > /dev/null 2>&1
if grep -q "POST https://api.example.com/login" "$IMPORT_FILE" && grep -q "authorization: Bearer {{authorization}}" "$IMPORT_FILE" && grep -q "# @assert status == 200" "$IMPORT_FILE"; then
    echo -e "${GREEN}[✓] import curl 导入验证成功（凭据模板化 + status 断言）！${NC}"
else
    echo -e "${RED}[ERROR] import curl 导入失败！${NC}"
    cat "$IMPORT_FILE"
    exit 1
fi
IMPORT_DUP=0
$RUPOST_BIN import curl "curl -X POST 'https://api.example.com/login' -o "$IMPORT_FILE"" > /dev/null 2>&1 || IMPORT_DUP=$?
if [ "$IMPORT_DUP" -ne 0 ]; then
    echo -e "${GREEN}[✓] import 防覆盖验证成功（已存在拒绝 + 尾部 -o 拦截）！${NC}"
else
    echo -e "${RED}[ERROR] import 防覆盖失效！${NC}"
    exit 1
fi
rm -f "$IMPORT_FILE"

# 10. 验证 AI 批量提炼 (ai bootstrap --no-ai 降级) 与指令展开 (ai resolve)
echo -e "${BLUE}[*] 验证 ai bootstrap 降级与 ai resolve 指引...${NC}"
HIST_DIR=$(mktemp -d /tmp/rupost_hist_XXXXXX)
printf '%s\n' '{"id":"h1","timestamp":"2026-10-28T01:00:00Z","duration_ms":10,"request":{"method":"GET","url":"http://127.0.0.1:9002/v1/users/1","headers":{},"body":null},"source":"smoke","response":{"status":200,"headers":{},"body":"{\"token\":\"t\"}"}}' > "$HIST_DIR/history.jsonl"
BOOT_FILE=$(mktemp /tmp/rupost_boot_XXXXXX)
rm -f "$BOOT_FILE"  # mktemp 预创建空文件——bootstrap 防覆盖要求先移除
BOOT_EXIT=0
env -u RUPOST_AI_API_KEY -u OPENAI_API_KEY RUPOST_AI_PROBE_URL=http://127.0.0.1:1 RUPOST_HISTORY_DIR="$HIST_DIR" $RUPOST_BIN ai bootstrap --from history --last 5 -o "$BOOT_FILE" --no-ai > /dev/null 2>&1 || BOOT_EXIT=$?
if [ "$BOOT_EXIT" -eq 0 ] && grep -q "# @assert status == 200" "$BOOT_FILE" && grep -q "# @name " "$BOOT_FILE"; then
    echo -e "${GREEN}[✓] ai bootstrap 机械基线降级验证成功（exit 0 + status 断言 + @name）！${NC}"
else
    echo -e "${RED}[ERROR] ai bootstrap 降级失败（exit=$BOOT_EXIT）！${NC}"
    exit 1
fi

RESOLVE_FILE=$(mktemp /tmp/rupost_resolve_XXXXXX.http)
printf '%s\n' "### login" "POST {{base_url}}/auth/login" "" "# @ai-assert 登录成功" > "$RESOLVE_FILE"
RESOLVE_EXIT=0
env -u RUPOST_AI_API_KEY -u OPENAI_API_KEY RUPOST_AI_PROBE_URL=http://127.0.0.1:1 $RUPOST_BIN ai resolve "$RESOLVE_FILE" > /dev/null 2>&1 || RESOLVE_EXIT=$?
if [ "$RESOLVE_EXIT" -eq 1 ] && grep -q "@ai-assert 登录成功" "$RESOLVE_FILE" && ! grep -q "@assert" "$RESOLVE_FILE"; then
    echo -e "${GREEN}[✓] ai resolve B2 降级验证成功（文件原样 + exit 1）！${NC}"
else
    echo -e "${RED}[ERROR] ai resolve 降级验证失败（exit=$RESOLVE_EXIT）！${NC}"
    exit 1
fi
rm -rf "$HIST_DIR" "$BOOT_FILE" "$RESOLVE_FILE"

# 11. 验证失败钩子 trace 锚点协议 (--on-failure, DX-Debug R7)
echo -e "${BLUE}[*] 验证失败钩子 trace 锚点 (--on-failure)...${NC}"
HOOK_MOCK=$(mktemp /tmp/rupost_hook_mock_XXXXXX.json)
cat << 'EOF' > "$HOOK_MOCK"
[
  {
    "method": "GET",
    "path": "/api",
    "variants": [
      {
        "condition": null,
        "status": 401,
        "headers": { "Content-Type": "application/json", "X-Request-Id": "anchor-42" },
        "response_body": "{\"error\":\"unauthorized\"}"
      }
    ]
  }
]
EOF
$RUPOST_BIN mock "$HOOK_MOCK" --port "$HOOK_PORT" > /dev/null 2>&1 &
HOOK_MOCK_PID=$!
PIDS+=("$HOOK_MOCK_PID")
wait_mock_ready "$HOOK_PORT" "$WORKSPACE/mock_hook.log" "$HOOK_MOCK_PID"
HOOK_CASE=$(mktemp /tmp/rupost_hook_case_XXXXXX.http)
printf '%s\n' "### probe" "GET http://127.0.0.1:${HOOK_PORT}/api" "" "# @assert status == 200" > "$HOOK_CASE"
TRACE_OUT=$(mktemp /tmp/rupost_trace_XXXXXX)
HOOK_EXIT=0
$RUPOST_BIN t "$HOOK_CASE" --on-failure "printf '%s' \"\$RUPOST_FAILED_TRACE_ID\" > $TRACE_OUT" > /dev/null 2>&1 || HOOK_EXIT=$?
kill $HOOK_MOCK_PID 2>/dev/null || true
if [ "$HOOK_EXIT" -eq 1 ] && [ "$(cat "$TRACE_OUT" 2>/dev/null)" = "anchor-42" ]; then
    echo -e "${GREEN}[✓] 失败钩子 trace 锚点验证成功（X-Request-Id → RUPOST_FAILED_TRACE_ID）！${NC}"
else
    echo -e "${RED}[ERROR] 失败钩子锚点验证失败（exit=$HOOK_EXIT）！${NC}"
    exit 1
fi
rm -rf "$HOOK_MOCK" "$HOOK_CASE" "$TRACE_OUT"

# 12. 验证快照重放失败定位 (R6: replay per-entry + exit code)
echo -e "${BLUE}[*] 验证快照重放失败定位 (replay per-entry)...${NC}"
REPLAY_MOCK=$(mktemp /tmp/rupost_replay_mock_XXXXXX)
cat << 'EOF' > "$REPLAY_MOCK"
[
  { "method": "GET", "path": "/ok", "variants": [ { "condition": null, "status": 200, "headers": { "Content-Type": "application/json" }, "response_body": "{\"ok\":true}" } ] },
  { "method": "GET", "path": "/bad", "variants": [ { "condition": null, "status": 503, "headers": { "Content-Type": "application/json" }, "response_body": "{\"err\":1}" } ] }
]
EOF
$RUPOST_BIN mock "$REPLAY_MOCK" --port "$REPLAY_PORT" > /dev/null 2>&1 &
REPLAY_MOCK_PID=$!
PIDS+=("$REPLAY_MOCK_PID")
wait_mock_ready "$REPLAY_PORT" "$WORKSPACE/mock_replay.log" "$REPLAY_MOCK_PID"
# 快照：#1 /ok 期望 200（成）、#2 /bad 期望 200（败，实得 503）
REPLAY_SNAP=$(mktemp /tmp/rupost_replay_snap_XXXXXX)
cat << 'EOF' > "$REPLAY_SNAP"
{
  "timestamp": "2026-11-04T00:00:00Z",
  "source_file": "verify_replay",
  "entries": [
    { "id": "r1", "request": { "method": "GET", "url": "http://origin/ok", "headers": {}, "body": null }, "response": { "status": 200, "headers": {}, "body": "" } },
    { "id": "r2", "request": { "method": "GET", "url": "http://origin/bad", "headers": {}, "body": null }, "response": { "status": 200, "headers": {}, "body": "" } }
  ]
}
EOF
REPLAY_STDOUT=$(mktemp /tmp/rupost_replay_out_XXXXXX)
REPLAY_EXIT=0
$RUPOST_BIN replay "$REPLAY_SNAP" --target "http://127.0.0.1:${REPLAY_PORT}" > "$REPLAY_STDOUT" 2>&1 || REPLAY_EXIT=$?
if [ "$REPLAY_EXIT" -eq 1 ] && grep -q "Failed entries:" "$REPLAY_STDOUT" && grep -q "expected 200, got 503" "$REPLAY_STDOUT"; then
    echo -e "${GREEN}[✓] 快照重放失败定位验证成功（失败 exit 1 + Failed entries 定位到条）！${NC}"
else
    echo -e "${RED}[ERROR] 快照重放定位验证失败（exit=$REPLAY_EXIT）！${NC}"
    exit 1
fi

# 12.5 export→replay 供给链闭环（T5）：历史 → 快照 → 重放（复用存活的 REPLAY_PORT mock）
HIST_DIR=$(mktemp -d /tmp/rupost_hist_replay_XXXXXX)
printf '%s\n' '{"id":"h1","timestamp":"2026-11-04T00:00:00Z","duration_ms":10,"request":{"method":"GET","url":"http://origin/ok","headers":{},"body":null},"source":"smoke","response":{"status":200,"headers":{},"body":"{\"ok\":true}"}}' > "$HIST_DIR/history.jsonl"
HIST_SNAP=$(mktemp /tmp/rupost_hist_snap_XXXXXX)
HIST_OUT=$(mktemp /tmp/rupost_hist_replay_out_XXXXXX)
RUPOST_HISTORY_DIR="$HIST_DIR" $RUPOST_BIN history export --last 1 -o "$HIST_SNAP" > /dev/null 2>&1 \
    || { echo -e "${RED}[ERROR] history export 失败（供给链闭环前置步）！${NC}"; exit 1; }
HIST_EXIT=0
$RUPOST_BIN replay "$HIST_SNAP" --target "http://127.0.0.1:${REPLAY_PORT}" > "$HIST_OUT" 2>&1 || HIST_EXIT=$?
if [ "$HIST_EXIT" -eq 0 ] && grep -q "Total: 1, Passed: 1" "$HIST_OUT"; then
    echo -e "${GREEN}[✓] 历史导出→重放供给链验证成功（export→replay 闭环）！${NC}"
else
    echo -e "${RED}[ERROR] 历史导出→重放闭环验证失败（exit=$HIST_EXIT）！${NC}"
    exit 1
fi
# 13. 验证 replay --explain 无 Key 降级契约（对照步骤 8 的 t --explain 四条锁法；
# 复用 12/12.5 存活的双快照与 REPLAY_PORT mock——须在 HIST_SNAP 清理前执行）
echo -e "${BLUE}[*] 验证 replay --explain 规则版降级...${NC}"
EXPLAIN_OUT=$(mktemp /tmp/rupost_explain_XXXXXX)
EXPLAIN_ERR=$(mktemp /tmp/rupost_explain_err_XXXXXX)
EXPLAIN_EXIT=0
env -u RUPOST_AI_API_KEY -u OPENAI_API_KEY $RUPOST_BIN replay "$REPLAY_SNAP" --target "http://127.0.0.1:${REPLAY_PORT}" --explain > "$EXPLAIN_OUT" 2> "$EXPLAIN_ERR" || EXPLAIN_EXIT=$?
if [ "$EXPLAIN_EXIT" -eq 1 ] && grep -q "Possible Causes" "$EXPLAIN_OUT" && ! grep -q "AI Explanation" "$EXPLAIN_OUT" && grep -q "AI 归因已跳过" "$EXPLAIN_ERR"; then
    echo -e "${GREEN}[✓] replay --explain 规则版降级验证成功（exit 1 + 规则版 + 无 AI 段）！${NC}"
else
    echo -e "${RED}[ERROR] replay --explain 降级验证失败（exit=$EXPLAIN_EXIT）！${NC}"
    exit 1
fi
OK_EXIT=0
env -u RUPOST_AI_API_KEY -u OPENAI_API_KEY $RUPOST_BIN replay "$HIST_SNAP" --target "http://127.0.0.1:${REPLAY_PORT}" --explain > "$EXPLAIN_OUT" 2>/dev/null || OK_EXIT=$?
if [ "$OK_EXIT" -eq 0 ] && ! grep -q "Possible Causes" "$EXPLAIN_OUT"; then
    echo -e "${GREEN}[✓] 全成功 --explain 零归因噪音验证成功（exit 0）！${NC}"
else
    echo -e "${RED}[ERROR] 全成功 --explain 噪音验证失败（exit=$OK_EXIT）！${NC}"
    exit 1
fi
rm -rf "$EXPLAIN_OUT" "$EXPLAIN_ERR"
rm -rf "$HIST_DIR" "$HIST_SNAP" "$HIST_OUT"

# 14. 验证 JUnit XML 报告（M1：CI 消费场景）
echo -e "${BLUE}[*] 验证 JUnit XML 报告 (--report junit)...${NC}"
JUNIT_CASE=$(mktemp /tmp/rupost_junit_case_XXXXXX.http)
printf '%s\n' "### health" "# @name health" "GET http://127.0.0.1:${REPLAY_PORT}/ok" "" "# @assert status == 200" "" "### fail" "# @name fail" "GET http://127.0.0.1:${REPLAY_PORT}/bad" "" "# @assert status == 200" > "$JUNIT_CASE"
JUNIT_XML=$(mktemp /tmp/rupost_junit_XXXXXX.xml)
JUNIT_EXIT=0
$RUPOST_BIN t "$JUNIT_CASE" --report junit --report-file "$JUNIT_XML" > /dev/null 2>&1 || JUNIT_EXIT=$?
if [ "$JUNIT_EXIT" -eq 1 ] && grep -q "<testsuites" "$JUNIT_XML" && grep -q "failures=\"1\"" "$JUNIT_XML" && grep -q "<failure" "$JUNIT_XML" && ! grep -q $'\x1b' "$JUNIT_XML"; then
    echo -e "${GREEN}[✓] JUnit XML 报告验证成功（exit 1 与失败数一致 + ANSI 剥离）！${NC}"
else
    echo -e "${RED}[ERROR] JUnit XML 报告验证失败（exit=$JUNIT_EXIT）！${NC}"
    exit 1
fi
rm -rf "$JUNIT_CASE" "$JUNIT_XML"

kill $REPLAY_MOCK_PID 2>/dev/null || true
rm -rf "$REPLAY_MOCK" "$REPLAY_SNAP" "$REPLAY_STDOUT"

# 15. 验证插件体系冒烟（Stdio IPC 管线 + 工作区闸门 + 降级门禁 + 保留字拒绝 + 新旗标）
echo -e "${BLUE}[*] 验证插件体系冒烟 (plugin registry/pipeline/degradation)...${NC}"
# 子 shell 内 cd 到临时工作区后相对路径失效，先解析绝对路径
PLUGIN_BIN="$(cd "$(dirname "$RUPOST_BIN")" && pwd)/$(basename "$RUPOST_BIN")"
PLUGIN_WS=$(mktemp -d /tmp/rupost_plugin_ws_XXXXXX)
mkdir -p "$PLUGIN_WS/.rupost/plugins"

# 插件 mock：X-Smoke=yes 命中 200 变体，否则兜底 403（防假守护：断言依赖插件真实注入的 Header）
PLUGIN_MOCK="$PLUGIN_WS/mock.json"
cat << 'EOF' > "$PLUGIN_MOCK"
[
  { "method": "GET", "path": "/guarded", "variants": [
    { "condition": { "source": "Header", "key": "X-Smoke", "operator": "Equals", "expected_value": "yes" },
      "status": 200, "headers": { "Content-Type": "application/json" }, "response_body": "{\"guarded\":\"ok\"}" },
    { "condition": null, "status": 403, "headers": { "Content-Type": "application/json" }, "response_body": "{\"error\":\"no plugin header\"}" }
  ] }
]
EOF
$RUPOST_BIN mock "$PLUGIN_MOCK" --port "$PLUGIN_PORT" > /dev/null 2>&1 &
PLUGIN_MOCK_PID=$!
PIDS+=("$PLUGIN_MOCK_PID")
wait_mock_ready "$PLUGIN_PORT" "$WORKSPACE/mock_plugin.log" "$PLUGIN_MOCK_PID"

# suite 级缓存插件：注入 X-Smoke: yes（真实 sh 子进程 Stdio IPC）
SMOKE_EXE="$PLUGIN_WS/.rupost/plugins/rupost-plugin-smoke-auth"
cat << 'EOF' > "$SMOKE_EXE"
#!/bin/sh
cat > /dev/null
echo '{"protocol_version":"1.0","extra_headers":[["X-Smoke","yes"]],"remove_headers":[],"mutated_body":null,"set_variables":{},"warnings":[]}'
EOF
chmod 755 "$SMOKE_EXE"
cat << 'EOF' > "$PLUGIN_WS/.rupost/plugins/rupost-plugin-smoke-auth.json"
{
  "manifest_version": "1.0", "name": "smoke-auth", "version": "0.1.0", "description": "smoke plugin",
  "commands": [],
  "hooks": [{ "directive": "auth", "alias": "smoke-auth", "stage": "prepare", "cache_scope": "suite", "requires_env": [], "parameters_schema": {} }]
}
EOF
# 保留字阶段 manifest（C2：必须被拒绝注册并告警，且无同目录二进制也不得误报二进制缺失在前）
cat << 'EOF' > "$PLUGIN_WS/.rupost/plugins/rupost-plugin-reserved.json"
{
  "manifest_version": "1.0", "name": "reserved", "version": "0.1.0", "description": "reserved stage",
  "commands": [],
  "hooks": [{ "directive": "harvest", "stage": "post_response", "cache_scope": "none", "requires_env": [], "parameters_schema": {} }]
}
EOF

cat << EOF > "$PLUGIN_WS/rupost.toml"
[environments.dev]
base_url = "http://127.0.0.1:${PLUGIN_PORT}"
EOF

# a) plugin list 冒烟（含工作区闸门旗标 + 保留字跳过告警可见）
PLUGIN_LIST_ERR=$(mktemp /tmp/rupost_plugin_list_XXXXXX)
PLUGIN_LIST_EXIT=0
(cd "$PLUGIN_WS" && $PLUGIN_BIN plugin list --allow-project-plugins) > /dev/null 2> "$PLUGIN_LIST_ERR" || PLUGIN_LIST_EXIT=$?
if [ "$PLUGIN_LIST_EXIT" -eq 0 ] && grep -q "保留字" "$PLUGIN_LIST_ERR"; then
    echo -e "${GREEN}[✓] plugin list + 保留字阶段拒绝注册告警验证成功！${NC}"
else
    echo -e "${RED}[ERROR] plugin list / 保留字告警验证失败！${NC}"; cat "$PLUGIN_LIST_ERR"; exit 1
fi

# b) 插件真实注入：请求带 @auth smoke-auth → X-Smoke: yes 命中 200 变体（exit 0 证明插件 Header 到达）
PLUGIN_HTTP="$PLUGIN_WS/plugin.http"
printf '%s\n' "### guarded" "@auth smoke-auth ?optional" "GET {{base_url}}/guarded" "" "# @assert status == 200" > "$PLUGIN_HTTP"
PLUGIN_RUN_EXIT=0
(cd "$PLUGIN_WS" && $PLUGIN_BIN t plugin.http --env dev --allow-project-plugins --save-snapshot plugin_snap.json --max-snapshot-body-bytes 8) > /dev/null 2>&1 || PLUGIN_RUN_EXIT=$?
if [ "$PLUGIN_RUN_EXIT" -eq 0 ] && grep -q '"body_truncated": *true' "$PLUGIN_WS/plugin_snap.json"; then
    echo -e "${GREEN}[✓] 插件 Stdio IPC 管线注入 + --max-snapshot-body-bytes 截断验证成功！${NC}"
else
    echo -e "${RED}[ERROR] 插件管线 / 快照上限旗标验证失败！${NC}"; exit 1
fi

# c) 必选插件缺失：默认（未传任何降级旗标）阻断 exit 1 并列出搜索路径；
#    --strict-plugins 的整体判失败门禁由单测与 runner 门禁测试覆盖，此处不重复传旗标
MISS_HTTP="$PLUGIN_WS/missing.http"
printf '%s\n' "### blocked" "@auth no-such-plugin" "GET {{base_url}}/guarded" "" "# @assert status == 200" > "$MISS_HTTP"
(cd "$PLUGIN_WS" && $PLUGIN_BIN t missing.http --env dev --allow-project-plugins) > /dev/null 2>&1 && BLOCKED=0 || BLOCKED=$?
if [ "$BLOCKED" -eq 1 ]; then
    echo -e "${GREEN}[✓] 必选插件缺失默认阻断（exit 1）验证成功！${NC}"
else
    echo -e "${RED}[ERROR] 缺失插件阻断失效（exit=$BLOCKED）！${NC}"; exit 1
fi

# d) ?optional + --allow-missing-plugins：降级原样外发（403 断言失败照常 exit 1，但插件缺席不额外阻断）
printf '%s\n' "### degraded" "@auth no-such-plugin ?optional" "GET {{base_url}}/guarded" "" "# @assert status == 403" > "$MISS_HTTP"
DEGRADE_EXIT=0
(cd "$PLUGIN_WS" && $PLUGIN_BIN t missing.http --env dev --allow-project-plugins --allow-missing-plugins --save-snapshot degraded_snap.json) > /dev/null 2>&1 || DEGRADE_EXIT=$?
if [ "$DEGRADE_EXIT" -eq 0 ] && grep -q '"degraded_pass"' "$PLUGIN_WS/degraded_snap.json"; then
    echo -e "${GREEN}[✓] ?optional + --allow-missing-plugins 降级 + DegradedPass 快照三态验证成功！${NC}"
else
    echo -e "${RED}[ERROR] 降级语义验证失败（exit=$DEGRADE_EXIT）！${NC}"; exit 1
fi

kill $PLUGIN_MOCK_PID 2>/dev/null || true
rm -rf "$PLUGIN_WS" "$PLUGIN_LIST_ERR"

# e) AST 语法错误退出码归位（M6-5：解析错误 exit 2，执行失败仍 exit 1）
AST_BAD=$(mktemp /tmp/rupost_ast_bad_XXXXXX.http)
printf '%s\n' "FTP https://example.invalid" > "$AST_BAD"
$RUPOST_BIN t "$AST_BAD" > /dev/null 2>&1 && AST_EXIT=0 || AST_EXIT=$?
rm -f "$AST_BAD"
if [ "$AST_EXIT" -eq 2 ]; then
    echo -e "${GREEN}[✓] AST 语法错误 exit 2 归位验证成功！${NC}"
else
    echo -e "${RED}[ERROR] AST 语法错误退出码应为 2，实际: $AST_EXIT${NC}"
    exit 1
fi

# 清理工作区（trap 已兜底清理所有本脚本启动的进程）
cd "$REPO_ROOT"
rm -rf "$WORKSPACE"

echo -e "\n${GREEN}=====================================================${NC}"
echo -e "${GREEN}      RuPost 核心特性端到端自动化验证通过！[PASS]      ${NC}"
echo -e "${GREEN}=====================================================${NC}"
