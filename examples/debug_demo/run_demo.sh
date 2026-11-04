#!/bin/bash
# 调试链全流程演示脚本：按 画像×阶段 实跑全部调试能力
# 前置：仓库根目录已构建二进制（cargo build --release）；无 AI 配置（演示 B1 降级路径）
# 用法：./run_demo.sh
set -e
cd "$(dirname "$0")"
killall rupost 2>/dev/null || true
BIN="$(cd ../.. && pwd)/target/release/rupost"
export RUPOST_HISTORY_DIR="$PWD/.demo-history"
rm -rf .demo-history hook.log report.json snap.json snap_hist.json regression.http

echo "═══ [0] 启动 mock 双实例：9500 正常态 / 9501 升级故障态 ═══"
"$BIN" mock mock_backend.json --port 9500 > /dev/null 2>&1 &
MOCK_A=$!
"$BIN" mock mock_upgraded.json --port 9501 > /dev/null 2>&1 &
MOCK_B=$!
sleep 2
trap 'kill $MOCK_A $MOCK_B 2>/dev/null' EXIT

echo "═══ [1] 小李·开发期：401 → 就地归因 → 修复 → 定点重跑 ═══"
echo "--- 1a) 错误密码批量跑（登录 401 级联 + 报表 500 + 配额断言）---"
"$BIN" t suite.http --var base_url=http://127.0.0.1:9500 --var password=wrongpw | grep -E "✗|Tests:" || true
echo "--- 1b) --only login --explain（规则版归因 + AI 降级提示）---"
"$BIN" t suite.http --only login --explain --var base_url=http://127.0.0.1:9500 --var password=wrongpw | grep -E "单请求模式|Possible|401 鉴权|AI 归因" || true
echo "--- 1c) 修复密码 → 定点重跑 ---"
"$BIN" t suite.http --only login --var base_url=http://127.0.0.1:9500 --var password=hunter2 | grep -E "✓ \[1\]|Tests:" || true

echo "═══ [2] 小赵·测试期：批量报告 → ai triage 双簇分诊（无 AI 降级）═══"
"$BIN" t suite.http --report-file report.json --var base_url=http://127.0.0.1:9500 --var password=hunter2 | grep -E "✗ \[|Tests:" || true
"$BIN" ai triage report.json | grep -E "### |Affected|failure" || true

echo "═══ [3] 小张·联调期：录制全绿基线 → 对升级故障态重放（定位到条 + exit 1）═══"
"$BIN" t green.http --save-snapshot snap.json --var base_url=http://127.0.0.1:9500 --var password=hunter2 | grep -E "✓ \[" || true
set +e
"$BIN" replay snap.json -t http://127.0.0.1:9501 | grep -E "mismatch|Total:|Failed entries:| - #" || true
"$BIN" replay snap.json -t http://127.0.0.1:9501 > /dev/null 2>&1
echo "故障态重放退出码 = $?（CI 门禁语义：1）"
"$BIN" replay snap.json -t http://127.0.0.1:9500 > /dev/null 2>&1
echo "正常态重放退出码 = $?（0）"
set -e

echo "═══ [4] 小李·取证：失败钩子捕获 trace 锚点 ═══"
"$BIN" t suite.http --on-failure "sh -c 'echo \"\$RUPOST_FAILED_STATUS|\$RUPOST_FAILED_URL|\$RUPOST_FAILED_TRACE_ID\" >> $PWD/hook.log'" \
    --var base_url=http://127.0.0.1:9500 --var password=hunter2 > /dev/null 2>&1 || true
echo "--- hook.log（状态码|URL|trace；报表 500 携带 mock 注入的 X-Request-Id）---"
cat hook.log

echo "═══ [5] 沉淀：history export → replay 供给链 + bootstrap 机械基线 ═══"
"$BIN" history export --last 5 -o snap_hist.json
"$BIN" replay snap_hist.json -t http://127.0.0.1:9500 | grep -E "Total:" || true
rm -f regression.http
"$BIN" ai bootstrap --from history --last 5 -o regression.http --no-ai | head -3 || true

echo "═══ 演示完成：调试链 D0–D7 全链路实跑 ═══"
