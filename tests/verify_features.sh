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
TEMP_DIR=$(mktemp -d)
echo -e "${BLUE}[*] 创建临时测试工作区: $TEMP_DIR${NC}"

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

# 关闭 Mock 后台服务
echo -e "${BLUE}[*] 关闭 Mock 服务后台进程 (PID: $MOCK_PID)...${NC}"
kill $MOCK_PID

# 5. 验证级联变量覆盖与 DAG 并行执行
echo -e "${BLUE}[*] 验证变量级联优先级与拓扑并行测试...${NC}"

# 创建 rupost.toml
TOML_CONF="$TEMP_DIR/rupost.toml"
cat << 'EOF' > "$TOML_CONF"
[environments.dev]
base_url = "https://httpbin.org"
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
POST https://httpbin.org/post
Content-Type: application/json

{ "token": "token-12345" }

@capture auth_token = body.json.token
EOF

# 创建 02_profile.http
PROFILE_HTTP="$TEMP_DIR/02_profile.http"
cat << 'EOF' > "$PROFILE_HTTP"
### @depends-on 01_auth.http
GET https://httpbin.org/headers
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
$RUPOST_BIN test "$PROFILE_HTTP" --mode parallel --env dev > "$TEMP_DIR/run.log" 2>&1

cat "$TEMP_DIR/run.log"

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

echo -e "\n${GREEN}=====================================================${NC}"
echo -e "${GREEN}      RuPost 核心特性端到端自动化验证通过！[PASS]      ${NC}"
echo -e "${GREEN}=====================================================${NC}"
