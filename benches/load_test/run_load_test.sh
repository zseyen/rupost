#!/bin/bash
set -e

# 定义彩色输出
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m' # No Color
BLUE='\033[0;34m'
YELLOW='\033[0;33m'

echo -e "${BLUE}[*] 开始执行 RuPost 宏观并发性能压力测试...${NC}"

# 0. 清理残留进程
killall rupost 2>/dev/null || true
killall load_generator 2>/dev/null || true

# 1. 编译最新的 release 二进制
echo -e "${BLUE}[*] 正在编译发布版 binaries (rupost & load_generator)...${NC}"
cargo build --release

RUPOST_BIN="./target/release/rupost"
LOAD_GEN_BIN="./target/release/load_generator"

if [ ! -f "$RUPOST_BIN" ] || [ ! -f "$LOAD_GEN_BIN" ]; then
    echo -e "${RED}[ERROR] 找不到编译生成的发布版二进制文件！${NC}"
    exit 1
fi
echo -e "${GREEN}[✓] 二进制程序编译成功！${NC}"

# 2. 启动本地 Mock 服务端压测
TEMP_DIR="./benches_temp_dir"
rm -rf "$TEMP_DIR"
mkdir -p "$TEMP_DIR"

MOCK_CONFIG="benches/load_test/mock_perf_config.json"

echo -e "${BLUE}[*] 后台拉起本地 Mock 服务器 (端口 9000)...${NC}"
$RUPOST_BIN mock "$MOCK_CONFIG" --port 9000 > "$TEMP_DIR/mock_run.log" 2>&1 &
MOCK_PID=$!
sleep 2

# 并发发包测试 (50连接并发，持续5秒)
echo -e "${BLUE}[*] 执行 Mock 高并发压力测试 (50 并发, 5 秒)...${NC}"
$LOAD_GEN_BIN --url http://127.0.0.1:9000/api/users/88 -c 50 -d 5

# 3. 压测批处理 DAG 拓扑调度性能
echo -e "${BLUE}[*] 正在动态生成具有多叉拓扑依赖的并发用例集 (150个用例)...${NC}"

# 写入首个 auth 用例 (用于捕获全局共享变量)
cat << 'EOF' > "$TEMP_DIR/000_auth.http"
POST http://127.0.0.1:9000/api/users/88
X-Role: Admin

@capture global.perf_token from body.role
EOF

# 写入 149 个并发子用例 (全部依赖 000_auth.http 并从全局变量中提取鉴权)
for i in $(seq -f "%03g" 1 149); do
    cat << EOF > "$TEMP_DIR/${i}_req.http"
### @depends-on 000_auth.http
GET http://127.0.0.1:9000/api/users/${i}
X-Role: {{global.perf_token}}

@assert status == 200
EOF
done

echo -e "${BLUE}[*] 开始压测批处理拓扑调度引擎 (150个依赖用例, 并发度 50)...${NC}"

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

# 写入临时 rupost.toml
cat << 'EOF' > rupost.toml
[environments.dev]
base_url = "http://127.0.0.1:9000"
EOF

# 运行批测试并使用 time 度量总耗时
START_SCHED=$(python3 -c "import time; print(int(time.time() * 1000))")
$RUPOST_BIN test "$TEMP_DIR/" --mode parallel --concurrency 50 --env dev > "$TEMP_DIR/run_perf.log" 2>&1
TEST_EXIT_CODE=$?
END_SCHED=$(python3 -c "import time; print(int(time.time() * 1000))")
ELAPSED_SCHED=$((END_SCHED - START_SCHED))

cat "$TEMP_DIR/run_perf.log"

if [ $TEST_EXIT_CODE -eq 0 ]; then
    echo -e "${GREEN}[✓] 150 个拓扑并发测试用例全部执行通过！${NC}"
    echo -e "${GREEN}[✓] 调度器 150 用例总耗时 (含 150 次 HTTP I/O): ${ELAPSED_SCHED} 毫秒。${NC}"
else
    echo -e "${RED}[ERROR] 拓扑并发测试执行失败或结果不完整！${NC}"
    # 关闭 Mock 服务并恢复
    kill $MOCK_PID || true
    rm -f rupost.toml
    [ $HAS_TOML -eq 1 ] && mv rupost.toml.bak rupost.toml
    [ $HAS_ENV -eq 1 ] && mv .env.bak .env
    exit 1
fi

# 4. 资源清理
echo -e "${BLUE}[*] 关闭 Mock 服务后台进程 (PID: $MOCK_PID)...${NC}"
kill $MOCK_PID || true

rm -f rupost.toml
[ $HAS_TOML -eq 1 ] && mv rupost.toml.bak rupost.toml
[ $HAS_ENV -eq 1 ] && mv .env.bak .env

rm -rf "$TEMP_DIR"

echo -e "\n${GREEN}=====================================================${NC}"
echo -e "${GREEN}      RuPost 宏观并发性能压力测试全部完成！[PASS]      ${NC}"
echo -e "${GREEN}=====================================================${NC}"
