#!/usr/bin/env bash
# =============================================================================
# examples/run_all.sh  —  一键执行所有 rupost 示例
# 用法：bash examples/run_all.sh
# =============================================================================
set -euo pipefail

# --- 颜色 ---
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
EXAMPLES_DIR="$ROOT_DIR/examples"
RUPOST_BIN="$ROOT_DIR/target/debug/rupost"
# iteration_scenarios 和 batch 示例硬编码了 9000 端口，Mock 使用相同端口
MOCK_PORT=9000
MOCK_PID=""
PASS=0
FAIL=0
SKIP_COUNT=0

cleanup() {
    if [ -n "$MOCK_PID" ] && kill -0 "$MOCK_PID" 2>/dev/null; then
        echo -e "\n${BLUE}[*] 关闭 Mock 服务 (PID: $MOCK_PID)...${NC}"
        kill "$MOCK_PID" 2>/dev/null || true
        wait "$MOCK_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

# run_test label <cmd...>
# 失败不会终止脚本，但记录失败计数
run_test() {
    local label="$1"
    shift
    echo -e "\n${BLUE}[>] $label${NC}"
    if "$@" 2>&1; then
        echo -e "${GREEN}[✓] $label${NC}"
        PASS=$((PASS + 1))
    else
        echo -e "${RED}[✗] $label${NC}"
        FAIL=$((FAIL + 1))
    fi
}

# run_test_skip label — 仅展示输出，不计入失败（含故意错误响应的演示文件）
run_demo() {
    local label="$1"
    shift
    echo -e "\n${BLUE}[>] $label ${YELLOW}[演示模式，忽略失败]${NC}"
    "$@" 2>&1 || true
    echo -e "${YELLOW}[~] $label (演示模式)${NC}"
    SKIP_COUNT=$((SKIP_COUNT + 1))
}

echo -e "${BOLD}=====================================================${NC}"
echo -e "${BOLD}         RuPost Examples — 全量执行脚本               ${NC}"
echo -e "${BOLD}=====================================================${NC}"

# --- 1. 构建最新二进制 ---
echo -e "\n${BLUE}[*] 构建最新 rupost 二进制...${NC}"
cd "$ROOT_DIR"
cargo build --bin rupost -q
echo -e "${GREEN}[✓] 构建完成${NC}"

# --- 2. 确认 rupost.toml 已在根目录（提供 dev 环境的 base_url） ---
if [ ! -f "$ROOT_DIR/rupost.toml" ]; then
    cp "$EXAMPLES_DIR/rupost.toml" "$ROOT_DIR/rupost.toml"
    echo -e "${YELLOW}[!] 已将 examples/rupost.toml 复制至项目根目录${NC}"
fi

# ===========================================================================
# Part A  基础 HTTP 示例（直接对 httpbingo.org 发请求）
# ===========================================================================
echo -e "\n${BOLD}--- Part A: 基础与断言示例 ---${NC}"

run_test "basic.http — 基础 GET/POST/DELETE" \
    "$RUPOST_BIN" test "$EXAMPLES_DIR/basic.http" --env dev

run_test "assertions.http — 全类型断言验证" \
    "$RUPOST_BIN" test "$EXAMPLES_DIR/assertions.http" --env dev

run_test "assertions.md — Markdown 断言验证" \
    "$RUPOST_BIN" test "$EXAMPLES_DIR/assertions.md" --env dev

run_test "multiple.http — 多请求批量（含 # @skip 验证）" \
    "$RUPOST_BIN" test "$EXAMPLES_DIR/multiple.http" --env dev

run_test "nested-blocks.md — Markdown 嵌套代码块提取" \
    "$RUPOST_BIN" test "$EXAMPLES_DIR/nested-blocks.md" --env dev

run_test "simple-api.md — 简单 API 文档格式" \
    "$RUPOST_BIN" test "$EXAMPLES_DIR/simple-api.md" --env dev

run_test "advanced.http — 高级请求（多 Header、PUT/PATCH）" \
    "$RUPOST_BIN" test "$EXAMPLES_DIR/advanced.http" --env dev

echo -e "\n${BOLD}--- Part B: 变量系统示例 ---${NC}"

run_test "variables.md — 变量替换场景" \
    "$RUPOST_BIN" test "$EXAMPLES_DIR/variables.md" --env dev

run_test "metadata.http — 元数据注解" \
    "$RUPOST_BIN" test "$EXAMPLES_DIR/metadata.http" --env dev

echo -e "\n${BOLD}--- Part C: Cookie 会话管理示例 ---${NC}"

COOKIE_FILE_A="$(mktemp /tmp/rupost_cookie_XXXXXX.json)"
run_test "cookie_demo.http — Cookie 罐链路验证" \
    "$RUPOST_BIN" test "$EXAMPLES_DIR/cookie_demo.http" --env dev \
    --cookie-file "$COOKIE_FILE_A"
rm -f "$COOKIE_FILE_A"

COOKIE_FILE_B="$(mktemp /tmp/rupost_cookie_XXXXXX.json)"
run_test "cookie_demo.md — Cookie Markdown 文档格式" \
    "$RUPOST_BIN" test "$EXAMPLES_DIR/cookie_demo.md" --env dev \
    --cookie-file "$COOKIE_FILE_B"
rm -f "$COOKIE_FILE_B"

echo -e "\n${BOLD}--- Part D: 诊断与时序示例 ---${NC}"

run_test "sprint1_diagnostics_demo.http — 时序诊断 HTTP 文件" \
    "$RUPOST_BIN" test "$EXAMPLES_DIR/sprint1_diagnostics_demo.http" --env dev

run_test "sprint1_diagnostics_demo.md — 时序诊断 Markdown" \
    "$RUPOST_BIN" test "$EXAMPLES_DIR/sprint1_diagnostics_demo.md" --env dev

echo -e "\n${BOLD}--- Part E: API 文档演示示例（含故意失败的演示请求）---${NC}"

# api-docs.md 和 auth-examples.md 是纯演示文件，包含故意发出 404/500 的请求
# 不添加 @skip 注解，作为演示场景保留，这里以演示模式运行（忽略失败）
run_demo "api-docs.md — API 文档演示（含预期 404/500 响应）" \
    "$RUPOST_BIN" test "$EXAMPLES_DIR/api-docs.md" --env dev

run_test "auth-examples.md — 认证示例文档" \
    "$RUPOST_BIN" test "$EXAMPLES_DIR/auth-examples.md" --env dev

# ===========================================================================
# Part F  需要本地 Mock 服务的场景（iteration_scenarios 硬编码 localhost:9000）
# ===========================================================================
echo -e "\n${BOLD}--- Part F: Mock 服务依赖示例 (localhost:$MOCK_PORT) ---${NC}"

# 检查端口是否已占用
if lsof -ti:$MOCK_PORT >/dev/null 2>&1; then
    echo -e "${YELLOW}[!] 端口 $MOCK_PORT 已被占用，尝试终止占用进程...${NC}"
    lsof -ti:$MOCK_PORT | xargs kill -9 2>/dev/null || true
    sleep 0.5
fi

# 生成覆盖 batch 和 iteration_scenarios 所需路由的 Mock 配置
MOCK_CONFIG="$(mktemp /tmp/rupost_mock_XXXXXX.json)"
cat > "$MOCK_CONFIG" << 'MOCK_EOF'
[
  {
    "method": "POST",
    "path": "/api/auth/login",
    "variants": [
      {
        "condition": null,
        "status": 200,
        "headers": { "Content-Type": "application/json" },
        "response_body": "{\"token\":\"jwt_session_token_abc123\"}"
      }
    ]
  },
  {
    "method": "GET",
    "path": "/api/users/profile",
    "variants": [
      {
        "condition": null,
        "status": 200,
        "headers": { "Content-Type": "application/json" },
        "response_body": "{\"id\":1,\"name\":\"Alice\",\"status\":\"profile-loaded\"}"
      }
    ]
  },
  {
    "method": "GET",
    "path": "/api/v1/orders/:id",
    "variants": [
      {
        "condition": null,
        "status": 200,
        "headers": { "Content-Type": "application/json" },
        "response_body": "{\"order_id\":\"{{id}}\",\"status\":\"completed\",\"compat_mode\":true,\"payment_method\":\"legacy\"}"
      }
    ]
  },
  {
    "method": "POST",
    "path": "/api/v2/orders",
    "variants": [
      {
        "condition": {
          "source": "Header",
          "key": "X-App-Version",
          "operator": "Equals",
          "expected_value": "2.0.0"
        },
        "status": 201,
        "headers": { "Content-Type": "application/json" },
        "response_body": "{\"order_id\":\"ORD-NEW-001\",\"status\":\"created\"}"
      },
      {
        "condition": null,
        "status": 400,
        "headers": { "Content-Type": "application/json" },
        "response_body": "{\"error\":\"Upgrade required: X-App-Version header missing or outdated\"}"
      }
    ]
  },
  {
    "method": "GET",
    "path": "/health",
    "variants": [
      {
        "condition": null,
        "status": 200,
        "headers": { "Content-Type": "application/json" },
        "response_body": "{\"status\":\"ok\"}"
      }
    ]
  },
  {
    "method": "GET",
    "path": "/version",
    "variants": [
      {
        "condition": null,
        "status": 200,
        "headers": { "Content-Type": "application/json" },
        "response_body": "{\"version\":\"2.0.0\"}"
      }
    ]
  },
  {
    "method": "GET",
    "path": "/v1/users",
    "variants": [
      {
        "condition": null,
        "status": 200,
        "headers": { "Content-Type": "application/json" },
        "response_body": "{\"users\":[{\"id\":1,\"name\":\"Alice\"},{\"id\":2,\"name\":\"Bob\"}],\"total\":2}"
      }
    ]
  },
  {
    "method": "POST",
    "path": "/v1/users",
    "variants": [
      {
        "condition": null,
        "status": 201,
        "headers": { "Content-Type": "application/json" },
        "response_body": "{\"id\":3,\"status\":\"created\"}"
      }
    ]
  },
  {
    "method": "GET",
    "path": "/v1/users/:id",
    "variants": [
      {
        "condition": null,
        "status": 200,
        "headers": { "Content-Type": "application/json" },
        "response_body": "{\"id\":\"{{id}}\",\"name\":\"Test User\",\"email\":\"test@example.com\"}"
      }
    ]
  },
  {
    "method": "PUT",
    "path": "/v1/users/:id",
    "variants": [
      { "condition": null, "status": 200, "headers": { "Content-Type": "application/json" }, "response_body": "{\"status\":\"updated\"}" }
    ]
  },
  {
    "method": "DELETE",
    "path": "/v1/users/:id",
    "variants": [
      { "condition": null, "status": 204, "headers": {}, "response_body": "" }
    ]
  },
  {
    "method": "POST",
    "path": "/v1/auth/login",
    "variants": [
      { "condition": null, "status": 200, "headers": { "Content-Type": "application/json" }, "response_body": "{\"token\":\"api_token_xyz\"}" }
    ]
  },
  {
    "method": "GET",
    "path": "/v1/auth/profile",
    "variants": [
      { "condition": null, "status": 200, "headers": { "Content-Type": "application/json" }, "response_body": "{\"id\":1,\"email\":\"test@example.com\"}" }
    ]
  },
  {
    "method": "GET",
    "path": "/v1/posts",
    "variants": [
      { "condition": null, "status": 200, "headers": { "Content-Type": "application/json" }, "response_body": "{\"posts\":[]}" }
    ]
  },
  {
    "method": "GET",
    "path": "/v1/search",
    "variants": [
      { "condition": null, "status": 200, "headers": { "Content-Type": "application/json" }, "response_body": "{\"results\":[]}" }
    ]
  },
  {
    "method": "POST",
    "path": "/v1/webhooks",
    "variants": [
      { "condition": null, "status": 201, "headers": { "Content-Type": "application/json" }, "response_body": "{\"id\":\"wh_123\",\"status\":\"active\"}" }
    ]
  },
  {
    "method": "POST",
    "path": "/v1/webhooks/:id/test",
    "variants": [
      { "condition": null, "status": 200, "headers": { "Content-Type": "application/json" }, "response_body": "{\"success\":true}" }
    ]
  },
  {
    "method": "GET",
    "path": "/v1/export/users",
    "variants": [
      { "condition": null, "status": 200, "headers": { "Content-Type": "text/csv" }, "response_body": "id,name,email\n1,Alice,alice@example.com" }
    ]
  },
  {
    "method": "GET",
    "path": "/v1/export/stats",
    "variants": [
      { "condition": null, "status": 200, "headers": { "Content-Type": "application/json" }, "response_body": "{\"stats\":{}}" }
    ]
  }
]
MOCK_EOF

echo -e "${BLUE}[*] 启动本地 Mock 服务（第一阶段：API 文档测试），端口 $MOCK_PORT...${NC}"
"$RUPOST_BIN" mock "$MOCK_CONFIG" --port "$MOCK_PORT" > /tmp/rupost_mock.log 2>&1 &
MOCK_PID=$!
sleep 1.5

if ! kill -0 "$MOCK_PID" 2>/dev/null; then
    echo -e "${RED}[✗] Mock 服务启动失败（查看日志: /tmp/rupost_mock.log），跳过相关示例${NC}"
    SKIP_COUNT=$((SKIP_COUNT + 2))
else
    echo -e "${GREEN}[✓] Mock 服务启动成功 (PID: $MOCK_PID)${NC}"

    # api-testing.md 请求本地 Mock 服务（通过 mock 环境：base_url=http://127.0.0.1:9000）
    run_test "api-testing.md — 完整 API 文档测试（Mock 服务）" \
        "$RUPOST_BIN" test "$EXAMPLES_DIR/api-testing.md" \
        --var "base_url=http://127.0.0.1:$MOCK_PORT" \
        --var "api_version=v1" \
        --var "api_key=test-token" \
        --var "test_user_email=test@example.com" \
        --var "test_user_password=pass123" \
        --no-cookies

    # batch 示例使用 httpbingo.org，以并行模式运行依赖链
    run_test "batch/ — 批量 DAG 依赖执行（httpbingo.org）" \
        "$RUPOST_BIN" test "$EXAMPLES_DIR/batch/" \
        --mode parallel --no-cookies

    kill "$MOCK_PID" 2>/dev/null || true
    MOCK_PID=""
fi

rm -f "$MOCK_CONFIG"

# 启动第二阶段 Mock 服务（契约驱动演进场景）
echo -e "\n${BLUE}[*] 启动本地 Mock 服务（第二阶段：契约驱动演进），端口 $MOCK_PORT...${NC}"
"$RUPOST_BIN" mock "$EXAMPLES_DIR/iteration_scenarios/migration_test.md" --port "$MOCK_PORT" > /tmp/rupost_mock_migration.log 2>&1 &
MOCK_PID=$!
sleep 1.5

if ! kill -0 "$MOCK_PID" 2>/dev/null; then
    echo -e "${RED}[✗] 契约 Mock 服务启动失败（查看日志: /tmp/rupost_mock_migration.log），跳过演进相关测试${NC}"
    SKIP_COUNT=$((SKIP_COUNT + 1))
else
    echo -e "${GREEN}[✓] 契约 Mock 服务启动成功 (PID: $MOCK_PID)${NC}"

    # iteration_scenarios/migration_test.md 会通过 @depends-on 运行关联的所有 test 块
    run_test "iteration_scenarios/migration_test.md — API 版本迁移兼容" \
        "$RUPOST_BIN" test "$EXAMPLES_DIR/iteration_scenarios/migration_test.md" \
        --no-cookies

    kill "$MOCK_PID" 2>/dev/null || true
    MOCK_PID=""
fi

# ===========================================================================
# 汇总报告
# ===========================================================================
TOTAL=$((PASS + FAIL + SKIP_COUNT))
echo -e "\n${BOLD}=====================================================${NC}"
echo -e "${BOLD}              示例执行汇总报告                        ${NC}"
echo -e "${BOLD}=====================================================${NC}"
echo -e "  总计示例组:  ${BOLD}$TOTAL${NC}"
echo -e "  ${GREEN}通过: $PASS${NC}"
if [ "$FAIL" -gt 0 ]; then
    echo -e "  ${RED}失败: $FAIL${NC}"
fi
if [ "$SKIP_COUNT" -gt 0 ]; then
    echo -e "  ${YELLOW}演示模式: $SKIP_COUNT (含故意失败的演示请求，不计入结果)${NC}"
fi

if [ "$FAIL" -eq 0 ]; then
    echo -e "\n${GREEN}${BOLD}  ✓ 所有示例执行成功！${NC}"
    exit 0
else
    echo -e "\n${RED}${BOLD}  ✗ 有 $FAIL 个示例组执行失败，请检查上方输出。${NC}"
    exit 1
fi
