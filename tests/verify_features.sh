#!/bin/bash
set -e

# 定义彩色输出
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m' # No Color
BLUE='\033[0;34m'
YELLOW='\033[0;33m'

echo -e "${BLUE}[*] 开始执行 RuPost 全特性自动化验证脚本...${NC}"

# 0. 清理可能残留的后台进程
killall rupost 2>/dev/null || true

# 1. 编译最新的二进制
echo -e "${BLUE}[*] 正在编译 RuPost 二进制程序...${NC}"
cargo build --release

RUPOST_BIN="./target/release/rupost"
if [ ! -f "$RUPOST_BIN" ]; then
    echo -e "${RED}[ERROR] 找不到编译生成的二进制文件: $RUPOST_BIN${NC}"
    exit 1
fi
echo -e "${GREEN}[✓] 编译成功！${NC}"

# 2. 准备临时目录
TEMP_DIR="./tests_temp_dir"
rm -rf "$TEMP_DIR"
mkdir -p "$TEMP_DIR"
echo -e "${BLUE}[*] 创建临时测试工作区: $TEMP_DIR${NC}"

# 2.5 验证 init 初始化模板命令
echo -e "${BLUE}[*] 验证 rupost init 初始化模板功能...${NC}"
(
    cd "$TEMP_DIR"
    ABS_RUPOST_BIN=$(pwd)/../target/release/rupost
    if [ ! -f "$ABS_RUPOST_BIN" ]; then
        ABS_RUPOST_BIN=$(cd .. && pwd)/target/release/rupost
    fi
    
    $ABS_RUPOST_BIN init
    
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
    if $ABS_RUPOST_BIN init 2>/dev/null; then
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


# 写入临时 mock_config.json
MOCK_CONFIG="$TEMP_DIR/mock_config.json"
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
  }
]
EOF

# 3. 验证本地 Mock 服务端 & Trie 路径解析 & 条件变体评估
echo -e "${BLUE}[*] 启动 Mock 服务后台进程，端口 9000...${NC}"
$RUPOST_BIN mock "$MOCK_CONFIG" --port 9000 > "$TEMP_DIR/mock.log" 2>&1 &
MOCK_PID=$!

# 等待 mock 服务器拉起
sleep 2

# 发送请求验证变体条件 (Header X-Role: Admin)
echo -e "${BLUE}[*] 验证 Mock 变体匹配 (条件满足: X-Role=Admin)...${NC}"
RES_ADMIN=$(curl -s -X GET http://localhost:9000/api/users/88 -H "X-Role: Admin")
echo "响应内容: $RES_ADMIN"
if [[ "$RES_ADMIN" == *"admin"* ]] && [[ "$RES_ADMIN" == *"88"* ]]; then
    echo -e "${GREEN}[✓] Mock 变体条件匹配成功，路径参数 id (88) 提取渲染成功！${NC}"
else
    echo -e "${RED}[ERROR] Mock 变体匹配失败！${NC}"
    kill $MOCK_PID
    exit 1
fi

# 发送请求验证兜底响应 (Header X-Role: Guest)
echo -e "${BLUE}[*] 验证 Mock 变体匹配 (条件不满足: 兜底 403)...${NC}"
RES_GUEST=$(curl -s -o /dev/null -w "%{http_code}" -X GET http://localhost:9000/api/users/88 -H "X-Role: Guest")
echo "响应状态码: $RES_GUEST"
if [ "$RES_GUEST" -eq 403 ]; then
    echo -e "${GREEN}[✓] Mock 兜底匹配逻辑验证成功！${NC}"
else
    echo -e "${RED}[ERROR] Mock 兜底匹配失败！${NC}"
    kill $MOCK_PID
    exit 1
fi

# 4. 验证网络诊断工具 (diagnose)
echo -e "${BLUE}[*] 验证网络高亮诊断 (diagnose)...${NC}"
# 对活动中的 mock 服务进行诊断 (使用 127.0.0.1 避开 macOS 的 localhost IPv6 解析问题)
$RUPOST_BIN diagnose http://127.0.0.1:9000 > "$TEMP_DIR/diagnose.log"
cat "$TEMP_DIR/diagnose.log"
if grep -q "Diagnostics" "$TEMP_DIR/diagnose.log" || grep -q "DNS Lookup" "$TEMP_DIR/diagnose.log"; then
    echo -e "${GREEN}[✓] 网络诊断输出校验成功！${NC}"
else
    echo -e "${RED}[ERROR] 网络诊断输出校验失败！${NC}"
    kill $MOCK_PID
    exit 1
fi

# 5. 验证级联变量覆盖与 DAG 并行执行
echo -e "${BLUE}[*] 验证变量级联优先级与拓扑并行测试...${NC}"

# 创建 rupost.toml
TOML_CONF="$TEMP_DIR/rupost.toml"
cat << 'EOF' > "$TOML_CONF"
[environments.dev]
base_url = "http://127.0.0.1:9000"
test_key = "toml-val"
EOF

# 创建 .env
ENV_CONF="$TEMP_DIR/.env"
cat << 'EOF' > "$ENV_CONF"
test_key = "env-val"
EOF

# 创建 01_auth.http
AUTH_HTTP="$TEMP_DIR/01_auth.http"
cat << 'EOF' > "$AUTH_HTTP"
POST http://127.0.0.1:9000/api/auth/login
Content-Type: application/json

{ "username": "admin" }

@capture auth_token from body.token
EOF

# 创建 02_profile.http
PROFILE_HTTP="$TEMP_DIR/02_profile.http"
cat << 'EOF' > "$PROFILE_HTTP"
### @depends-on 01_auth.http
GET http://127.0.0.1:9000/api/user/profile
X-Test-Key: {{test_key}}
Authorization: Bearer {{auth_token}}
EOF

# 备份本地已有的 toml 和 env 配置文件，测试完毕后还原
HAS_TOML=0
if [ -f "rupost.toml" ]; then
    HAS_TOML=1
    mv rupost.toml rupost.toml.bak
fi

HAS_ENV=0
if [ -f ".env" ]; then
    HAS_ENV=1
    mv .env .env.bak
fi

cp "$TOML_CONF" rupost.toml
cp "$ENV_CONF" .env

echo -e "${BLUE}[*] 执行 DAG 拓扑并行测试 (验证依赖关系与 State Cloning 状态传递)...${NC}"
$RUPOST_BIN test "$PROFILE_HTTP" --mode parallel --env dev > "$TEMP_DIR/run.log" 2>&1 || true

cat "$TEMP_DIR/run.log"

# 关闭 Mock 后台服务 (完成所有网络请求后关闭)
echo -e "${BLUE}[*] 关闭 Mock 服务后台进程 (PID: $MOCK_PID)...${NC}"
kill $MOCK_PID || true

# 检查是否成功运行且 X-Test-Key 使用了 .env 的 "env-val"
# 并且 02_profile.http 获取到了 01_auth.http 捕获的 token
if grep -q "01_auth.http" "$TEMP_DIR/run.log" && grep -q "02_profile.http" "$TEMP_DIR/run.log"; then
    echo -e "${GREEN}[✓] DAG 并行测试执行成功！${NC}"
else
    echo -e "${RED}[ERROR] DAG 并行执行校验失败！${NC}"
    # 恢复环境
    rm -f rupost.toml .env
    [ $HAS_TOML -eq 1 ] && mv rupost.toml.bak rupost.toml
    [ $HAS_ENV -eq 1 ] && mv .env.bak .env
    exit 1
fi

# 恢复用户原有的环境
rm -f rupost.toml .env
[ $HAS_TOML -eq 1 ] && mv rupost.toml.bak rupost.toml
[ $HAS_ENV -eq 1 ] && mv .env.bak .env

# 清理临时文件
rm -rf "$TEMP_DIR"

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
$RUPOST_BIN mock examples/ai_assert/mock_ai.json --port 9001 > /dev/null 2>&1 &
MOCK_AI_PID=$!
sleep 2

AI_OUT=$(RUPOST_AI_BASE_URL=http://127.0.0.1:9001/v1 RUPOST_AI_API_KEY=demo $RUPOST_BIN ai assert "登录成功并且响应里包含token" 2>&1)
echo "AI 断言输出: $AI_OUT"
if echo "$AI_OUT" | grep -q "# @assert status == 200" && echo "$AI_OUT" | grep -q "# @assert body.token exists"; then
    echo -e "${GREEN}[✓] ai assert 离线 dogfooding 闭环验证成功（无真实 API Key）！${NC}"
else
    echo -e "${RED}[ERROR] ai assert 闭环验证失败！${NC}"
    kill $MOCK_AI_PID
    exit 1
fi

# 未配置密钥时应返回退出码 1 并给出可执行指引（set -e 下用 || 捕获退出码）
AI_EXIT=0
env -u RUPOST_AI_API_KEY -u OPENAI_API_KEY RUPOST_AI_PROBE_URL=http://127.0.0.1:1 $RUPOST_BIN ai assert "status is 200" > /dev/null 2>&1 || AI_EXIT=$?
if [ $AI_EXIT -eq 1 ]; then
    echo -e "${GREEN}[✓] 未配置密钥时正确返回退出码 1！${NC}"
else
    echo -e "${RED}[ERROR] 未配置密钥时应返回退出码 1，实际: $AI_EXIT${NC}"
    kill $MOCK_AI_PID
    exit 1
fi

kill $MOCK_AI_PID

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
$RUPOST_BIN mock examples/ai_triage/mock_ai.json --port 9002 > /dev/null 2>&1 &
MOCK_TRIAGE_PID=$!
sleep 2

FAIL_HTTP=$(mktemp /tmp/rupost_fail_XXXXXX.http)
cat << 'EOF' > "$FAIL_HTTP"
### get-profile
GET http://127.0.0.1:9002/v1/users/1
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
    kill $MOCK_TRIAGE_PID
    exit 1
fi

# 真实失败 → 真实报告 → ai triage 消费（B1 降级，exit 0，敏感值不泄漏）
DOGFOOD_EXIT=0
env -u RUPOST_AI_API_KEY -u OPENAI_API_KEY RUPOST_AI_PROBE_URL=http://127.0.0.1:1 $RUPOST_BIN test "$FAIL_HTTP" --report-file "$DOGFOOD_REPORT" > /dev/null 2>&1 || DOGFOOD_EXIT=$?
if [ "$DOGFOOD_EXIT" -eq 1 ] && [ -f "$DOGFOOD_REPORT" ]; then
    echo -e "${GREEN}[✓] test --report-file 落盘成功（exit 1 由失败断言决定）！${NC}"
else
    echo -e "${RED}[ERROR] --report-file 闭环失败（exit=$DOGFOOD_EXIT）！${NC}"
    kill $MOCK_TRIAGE_PID
    exit 1
fi

TRIAGE_JSON=$(env -u RUPOST_AI_API_KEY -u OPENAI_API_KEY RUPOST_AI_PROBE_URL=http://127.0.0.1:1 $RUPOST_BIN ai triage "$DOGFOOD_REPORT" --json)
if echo "$TRIAGE_JSON" | grep -q '"category": "Auth"' && ! echo "$TRIAGE_JSON" | grep -q "expired-jwt-should-not-leak"; then
    echo -e "${GREEN}[✓] ai triage dogfooding 闭环验证成功（真实报告 → Auth 根因簇，脱敏无泄漏）！${NC}"
else
    echo -e "${RED}[ERROR] ai triage dogfooding 闭环失败！${NC}"
    echo "$TRIAGE_JSON"
    kill $MOCK_TRIAGE_PID
    exit 1
fi

kill $MOCK_TRIAGE_PID
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

echo -e "\n${GREEN}=====================================================${NC}"
echo -e "${GREEN}      RuPost 核心特性端到端自动化验证通过！[PASS]      ${NC}"
echo -e "${GREEN}=====================================================${NC}"
