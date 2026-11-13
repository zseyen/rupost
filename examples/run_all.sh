#!/bin/bash
# ==============================================================================
# RuPost 示例运行脚本 - 一键执行 examples 目录下所有的 HTTP & Markdown 测试用例
# ==============================================================================

set -e

# 注册全局清理 Trap，确保脚本因 error 意外退出或被中断时后台 Mock 进程一定能被清理
trap 'echo -e "\n${YELLOW}[*] 正在释放后台仿真 Mock 服务进程...${NC}"; kill $LLM_MOCK_PID $SSE_MOCK_PID $PLUGIN_MOCK_PID $INCIDENT_MOCK_PID 2>/dev/null || true' EXIT

# 定义终端色彩
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # 无颜色

echo -e "${BLUE}======================================================================${NC}"
echo -e "${BLUE}                    RuPost 一键执行全部内置示例                      ${NC}"
echo -e "${BLUE}======================================================================${NC}"

# 1. 确保最新版二进制已就绪
echo -e "${CYAN}[*] 正在编译最新的发布版二进制 (rupost)...${NC}"
cargo build --release --bin rupost
RUPOST_BIN="./target/release/rupost"

if [ ! -f "$RUPOST_BIN" ]; then
    echo -e "${RED}[ERROR] 未能找到编译出的二进制文件！${NC}"
    exit 1
fi
echo -e "${GREEN}[✓] 编译就绪。${NC}"

# 统计计数器
PASSED=0
FAILED=0
TOTAL=0

# 记录执行结果的辅助函数
run_case() {
    local case_name=$1
    local command_args=$2
    TOTAL=$((TOTAL + 1))

    echo -e "\n${CYAN}----------------------------------------------------------------------${NC}"
    echo -e "${YELLOW}[${TOTAL}] 正在运行示例: ${case_name}${NC}"
    echo -e "${CYAN}执行命令: ${RUPOST_BIN} ${command_args}${NC}"
    echo -e "${CYAN}----------------------------------------------------------------------${NC}"

    # 运行用例并捕获返回值
    if $RUPOST_BIN $command_args; then
        echo -e "${GREEN}[✓] 示例 ${case_name} 执行通过 (PASS)${NC}"
        PASSED=$((PASSED + 1))
    else
        echo -e "${RED}[✗] 示例 ${case_name} 执行失败 (FAIL)${NC}"
        FAILED=$((FAILED + 1))
    fi
}

# 2. 运行免配置开箱即用示例 (直连公共测试源)
echo -e "\n${BLUE}>>> 第一阶段：运行免配置基础用例${NC}"
run_case "基础 HTTP 请求 (basic.http)" "test examples/basic.http"
run_case "断言语法展示 (assertions.http)" "test examples/assertions.http"
run_case "Markdown断言展示 (assertions.md)" "test examples/assertions.md"
run_case "多请求链式执行 (multiple.http)" "test examples/multiple.http"
run_case "Markdown嵌套代码块 (nested-blocks.md)" "test examples/nested-blocks.md"

# 3. 运行多环境变量用例 (基于 examples/rupost.toml)
echo -e "\n${BLUE}>>> 第二阶段：运行带变量与环境覆盖的用例${NC}"
run_case "基础 API 变量替换 (basic-api.http)" "test examples/basic-api.http --env mock"
run_case "Markdown完整测试文档 (api-testing.md)" "test examples/api-testing.md --env mock"
run_case "Cookie 会话管理与隔离 (cookie_demo.md)" "test examples/cookie_demo.md --env dev"
run_case "CRUD 数据管理流 (crud-operations.http)" "test examples/crud-operations.http --env mock"
run_case "用户鉴权登录流 (auth-flow.http)" "test examples/auth-flow.http --env mock"

# 4. 运行批处理 DAG 有向依赖用例 (批处理与并发隔离)
echo -e "\n${BLUE}>>> 第三阶段：运行有依赖关系的并行批测试${NC}"
run_case "DAG 并行状态捕获 (batch/)" "test examples/batch/ --env dev --mode parallel --concurrency 3"
run_case "多叉 DAG 拓扑网 (batch_complex/)" "test examples/batch_complex/ --env dev --mode parallel --concurrency 4"

# 5. 运行大模型与 Server-Sent Events (SSE) 本地闭环联调示例
echo -e "\n${BLUE}>>> 第四阶段：运行大模型与 Server-Sent Events (SSE) 闭环联调${NC}"

# (1) 启动大模型仿真 Mock 网关 (llm_demo.md)
echo -e "${CYAN}[*] 启动大模型仿真 Mock 网关在端口 8080...${NC}"
$RUPOST_BIN mock examples/llm_and_sse/llm_demo.md --port 8080 > /tmp/llm_mock_run.log 2>&1 &
LLM_MOCK_PID=$!
sleep 2

# 运行 LLM 用例
run_case "大模型流式 SSE 用例 (llm_demo.md)" "test examples/llm_and_sse/llm_demo.md --var base_url=http://127.0.0.1:8080"
run_case "大模型流式 HTTP 用例 (llm_demo.http)" "test examples/llm_and_sse/llm_demo.http --var base_url=http://127.0.0.1:8080"

# 关闭 LLM Mock
kill $LLM_MOCK_PID || true
LLM_MOCK_PID=""

# (2) 启动通用 SSE Mock 网关 (sse_demo.md)
echo -e "${CYAN}[*] 启动通用 SSE Mock 网关在端口 8080...${NC}"
$RUPOST_BIN mock examples/llm_and_sse/sse_demo.md --port 8080 > /tmp/sse_mock_run.log 2>&1 &
SSE_MOCK_PID=$!
sleep 2

# 运行通用 SSE 用例
run_case "通用 SSE 接口流用例 (sse_demo.md)" "test examples/llm_and_sse/sse_demo.md --var base_url=http://127.0.0.1:8080"

# 关闭 SSE Mock
kill $SSE_MOCK_PID || true
SSE_MOCK_PID=""

# 5.5 插件体系闭环（Stdio IPC 管线 @auth/@sign + 声明式自愈降级；需 python3）
if command -v python3 &>/dev/null; then
    PLUGIN_EX_WS=$(mktemp -d /tmp/rupost_plugin_example_XXXXXX)
    mkdir -p "$PLUGIN_EX_WS/.rupost/plugins"
    cp examples/plugins/rupost-plugin-echo-sign "$PLUGIN_EX_WS/.rupost/plugins/"
    cp examples/plugins/rupost-plugin-echo-sign.json "$PLUGIN_EX_WS/.rupost/plugins/"
    chmod 755 "$PLUGIN_EX_WS/.rupost/plugins/rupost-plugin-echo-sign"
    echo -e "\n${BLUE}>>> 第五阶段(续)：插件体系闭环联调${NC}"
    # 插件工作区闸门以 cwd 为根：cd 进临时工作区后相对路径失效，先解析绝对路径
    RUPOST_BIN="$(cd "$(dirname "$RUPOST_BIN")" && pwd)/$(basename "$RUPOST_BIN")"
    $RUPOST_BIN mock examples/plugins/mock.json --port 8090 > /tmp/plugin_mock_run.log 2>&1 &
    PLUGIN_MOCK_PID=$!
    sleep 2
    EX_PWD=$PWD
    cd "$PLUGIN_EX_WS"
    # 防假守护：必选插件形态——插件环境损坏时本用例必须 FAIL
    run_case "插件 Stdio IPC 管线 @auth/@sign (plugins)" "test $EX_PWD/examples/plugins/plugin_pipeline.http --allow-project-plugins --var base_url=http://127.0.0.1:8090"
    run_case "插件缺失自愈降级 (plugins)" "test $EX_PWD/examples/plugins/plugin_degrade.http --allow-project-plugins --allow-missing-plugins --var base_url=http://127.0.0.1:8090"
    cd "$EX_PWD"
    kill $PLUGIN_MOCK_PID || true
    PLUGIN_MOCK_PID=""
    rm -rf "$PLUGIN_EX_WS"
else
    echo -e "\n${YELLOW}[i] 未检测到 python3，跳过插件示例（安装 python3 后可跑通插件 Stdio IPC 闭环）${NC}"
fi

# 5.6 故障复现包闭环（WP-4，本地 loopback Mock，零公网零真实凭据）
echo -e "\n${BLUE}>>> 第五阶段(续)：故障复现包闭环（incident bundle）${NC}"
INCIDENT_WS=$(mktemp -d /tmp/rupost_incident_example_XXXXXX)
INCIDENT_PORT="${RUPOST_INCIDENT_MOCK_PORT:-8095}"
$RUPOST_BIN mock examples/incident_bundle/mock.json --port "$INCIDENT_PORT" > "$INCIDENT_WS/mock.log" 2>&1 &
INCIDENT_MOCK_PID=$!
INCIDENT_READY=0
for _ in $(seq 1 40); do
    if ! kill -0 "$INCIDENT_MOCK_PID" 2>/dev/null; then
        echo -e "${RED}[ERROR] 复现包 Mock 启动即退出（端口 ${INCIDENT_PORT} 被占用或配置错误）${NC}"
        cat "$INCIDENT_WS/mock.log" 2>/dev/null || true
        exit 1
    fi
    if curl -s -o /dev/null "http://127.0.0.1:${INCIDENT_PORT}/ok"; then
        INCIDENT_READY=1
        break
    fi
    sleep 0.2
done
if [ "$INCIDENT_READY" -ne 1 ]; then
    echo -e "${RED}[ERROR] 复现包 Mock 未在超时内就绪（端口 ${INCIDENT_PORT}）${NC}"
    cat "$INCIDENT_WS/mock.log" 2>/dev/null || true
    exit 1
fi
# 期望失败用例：exit 1 + 三文件工件 + 强制脱敏（manifest 声明 secrets 且原始凭据零泄漏）
INCIDENT_OUT="$INCIDENT_WS/payment-failure"
INCIDENT_EXIT=0
$RUPOST_BIN t examples/incident_bundle/payments.http --var "base_url=http://127.0.0.1:${INCIDENT_PORT}" --incident-out "$INCIDENT_OUT" --mask-secret-value demo-secret-token-xyz > /dev/null 2>&1 || INCIDENT_EXIT=$?
if [ "$INCIDENT_EXIT" -eq 1 ] \
    && [ -f "$INCIDENT_OUT/manifest.json" ] && [ -f "$INCIDENT_OUT/report.json" ] && [ -f "$INCIDENT_OUT/snapshot.json" ] \
    && ! grep -q "demo-secret-token-xyz" "$INCIDENT_OUT"/manifest.json "$INCIDENT_OUT"/report.json "$INCIDENT_OUT"/snapshot.json \
    && grep -q '"masking": *"secrets"' "$INCIDENT_OUT"/manifest.json; then
    echo -e "${GREEN}[✓] 故障复现包闭环验证成功（失败 exit 1 + 三文件 + 强制脱敏）(PASS)${NC}"
    TOTAL=$((TOTAL + 1)); PASSED=$((PASSED + 1))
else
    echo -e "${RED}[✗] 故障复现包闭环验证失败 (FAIL)${NC}"
    TOTAL=$((TOTAL + 1)); FAILED=$((FAILED + 1))
fi
# 成功对照：全绿运行即使给了 --incident-out 也不创建目录
GREEN_EXIT=0
$RUPOST_BIN t examples/incident_bundle/green.http --var "base_url=http://127.0.0.1:${INCIDENT_PORT}" --incident-out "$INCIDENT_WS/should-not-exist" > /dev/null 2>&1 || GREEN_EXIT=$?
if [ "$GREEN_EXIT" -eq 0 ] && [ ! -e "$INCIDENT_WS/should-not-exist" ]; then
    echo -e "${GREEN}[✓] 复现包成功对照验证成功（全绿不建目录）(PASS)${NC}"
    TOTAL=$((TOTAL + 1)); PASSED=$((PASSED + 1))
else
    echo -e "${RED}[✗] 复现包成功对照验证失败 (FAIL)${NC}"
    TOTAL=$((TOTAL + 1)); FAILED=$((FAILED + 1))
fi
kill $INCIDENT_MOCK_PID || true
INCIDENT_MOCK_PID=""
rm -rf "$INCIDENT_WS"

# 6. 打印最终执行结果
echo -e "\n${BLUE}======================================================================${NC}"
echo -e "${BLUE}                           示例运行总结报告                           ${NC}"
echo -e "${BLUE}======================================================================${NC}"
echo -e "总运行用例集: ${TOTAL}"
echo -e "通过 (PASS) : ${GREEN}${PASSED}${NC}"
if [ $FAILED -gt 0 ]; then
    echo -e "失败 (FAIL) : ${RED}${FAILED}${NC}"
    echo -e "${RED}部分示例未能完全运行通过，请检查网络（httpbingo.org 可达性）或配置。${NC}"
    exit 1
else
    echo -e "失败 (FAIL) : ${GREEN}0${NC}"
    echo -e "${GREEN}恭喜！所有内置示例全部成功跑通！[ALL PASS]${NC}"
fi
echo -e "${BLUE}======================================================================${NC}"
