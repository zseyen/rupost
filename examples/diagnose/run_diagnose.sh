#!/usr/bin/env bash

# 定义 ANSI 颜色转义码
BLUE='\033[1;34m'
GREEN='\033[1;32m'
YELLOW='\033[1;33m'
RED='\033[1;31m'
PURPLE='\033[1;35m'
CYAN='\033[1;36m'
WHITE='\033[1;37m'
RESET='\033[0m'

# 打印华丽标题
echo -e "${CYAN}================================================================${RESET}"
echo -e "${CYAN}          RuPost Network Diagnostics Showcase & Tutorial        ${RESET}"
echo -e "${CYAN}================================================================${RESET}"
echo -e "本脚本将通过四个核心实验为您展示 RuPost 的网络高亮诊断功能，"
echo -e "并结合步骤说明对各个网络诊断指标和结果进行解构。"
echo ""

# 1. 自动编译
echo -e "${BLUE}[步骤 1/4] 编译最新的 RuPost 二进制程序...${RESET}"
cargo build
if [ $? -ne 0 ]; then
    echo -e "${RED}[错误] Cargo 编译失败！${RESET}"
    exit 1
fi
RUPOST_BIN="./target/debug/rupost"
echo -e "${GREEN}[成功] 编译完成，程序路径为: ${RUPOST_BIN}${RESET}"
echo ""

# 启动本地 Mock 服务的清理机制
MOCK_PID=""
cleanup() {
    if [ -n "$MOCK_PID" ]; then
        echo -e "\n${YELLOW}[清理] 正在停止后台 Mock 服务端 (PID: $MOCK_PID)...${RESET}"
        kill -9 $MOCK_PID 2>/dev/null
    fi
}
trap cleanup EXIT

# 2. 启动本地 Mock 服务
echo -e "${BLUE}[准备] 启动本地 Mock 服务器...${RESET}"
# 在后台启动 mock 端口 9000
$RUPOST_BIN mock ./examples/diagnose/diagnose_mock.json --port 9000 > /tmp/rupost_diagnose_mock.log 2>&1 &
MOCK_PID=$!

# 轮询探测本地 9000 端口是否已就绪，最多等待 5 秒
echo -n "正在等待 Mock 服务就绪"
PORT_READY=0
for i in {1..25}; do
    if nc -z 127.0.0.1 9000 >/dev/null 2>&1; then
        PORT_READY=1
        break
    fi
    echo -n "."
    sleep 0.2
done

if [ $PORT_READY -eq 1 ]; then
    echo -e " ${GREEN}[已就绪]${RESET}"
else
    echo -e " ${RED}[超时] Mock 服务器未能及时在端口 9000 启动，将尝试继续执行。${RESET}"
fi
echo ""

# 3. 实验一：诊断本地 HTTP 接口
echo -e "${CYAN}----------------------------------------------------------------${RESET}"
echo -e "${PURPLE}🧪 实验 1: 本地 HTTP 服务诊断 (Local HTTP Connectivity)${RESET}"
echo -e "${CYAN}----------------------------------------------------------------${RESET}"
echo -e "${YELLOW}【测试背景】${RESET} 很多时候我们在本地开发一个微服务（例如端口 9000）。通过诊断命令，"
echo -e "我们可以迅速了解本地网络的建连基准值。"
echo -e "${YELLOW}【执行命令】${RESET} ${WHITE}\$ rupost diagnose http://127.0.0.1:9000/${RESET}"
echo ""

# 运行本地 HTTP 诊断
$RUPOST_BIN diagnose http://127.0.0.1:9000/

echo -e "\n${GREEN}【结果分析讲解】${RESET}"
echo -e "1. ${BLUE}DNS Lookup (DNS 解析):${RESET} 耗时通常极短（趋近于 0.0 ms），因为解析的是 '127.0.0.1' 这一本地硬编码 IP，无需向局域网或公网 DNS 服务器发送查询报文。"
echo -e "2. ${BLUE}TCP Connect (TCP 连接):${RESET} 耗时通常在 0.5-2.0 ms 之间，是内核本地回环网卡（loopback）建连的极限响应时延。"
echo -e "3. ${BLUE}TLS Handshake (TLS 握手):${RESET} 诊断结果不包含 TLS 项。因为我们请求的是 http:// 协议，不需要发起安全通道握手。"
echo -e "4. ${BLUE}HTTP TTFB (首字节时间):${RESET} 衡量的是 Mock 服务器从解析请求到生成响应（'Hello Diagnose'）的内部处理时长。它体现了服务端的实际开销。"
echo -e "5. ${BLUE}HTTP Protocol Info (HTTP协议状态):${RESET} 返回 ${GREEN}Status Code: 200${RESET} 且 ${WHITE}HTTP Version: HTTP/1.1${RESET}，说明接口响应完全正常。"
echo ""
read -p "按回车键 [Enter] 继续到实验 2..."

# 4. 实验二：诊断本地 WebSocket 接口
echo -e "\n${CYAN}----------------------------------------------------------------${RESET}"
echo -e "${PURPLE}🧪 实验 2: WebSocket 升级通道诊断 (WebSocket Upgrade)${RESET}"
echo -e "${CYAN}----------------------------------------------------------------${RESET}"
echo -e "${YELLOW}【测试背景】${RESET} 在生产环境中，客户端和微服务之间经常需要建立长连接（WS/WSS）。"
echo -e "如果遭遇反向代理（如 Nginx、API 网关）配置缺失，握手往往会被拦截。"
echo -e "我们通过传入 ws:// 协议，可以让 RuPost 模拟发送标准的 HTTP Upgrade 报文，用来检验服务端是否能正确升级协议。"
echo -e "${YELLOW}【执行命令】${RESET} ${WHITE}\$ rupost d ws://127.0.0.1:9000/ws${RESET}"
echo ""

# 运行本地 WS 诊断
$RUPOST_BIN d ws://127.0.0.1:9000/ws

echo -e "\n${GREEN}【结果分析讲解】${RESET}"
echo -e "1. ${BLUE}ws_upgrade_success:${RESET} 返回为 ${GREEN}true${RESET}，终端高亮显示 ${GREEN}[WS UPGRADE SUCCESS]${RESET}。"
echo -e "2. ${BLUE}Status Code:${RESET} 返回为 ${GREEN}101${RESET}，这是 RFC 6455 规定的 '101 Switching Protocols'。代表协议成功从 HTTP 升级为 WebSocket 长连接。"
echo -e "3. ${YELLOW}提示：${RESET} 如果在此处返回了 200、404 或 502 等非 101 状态，RuPost 会判定升级失败，并给出引导信息（检查 Nginx 的 Upgrade 和 Connection 头部是否正确设置）。"
echo ""
read -p "按回车键 [Enter] 继续到实验 3..."

# 关闭后台 mock，开始公网诊断
cleanup
MOCK_PID=""

# 5. 实验三：诊断公网 HTTPS 接口 (带 TLS 握手及证书解构)
echo -e "\n${CYAN}----------------------------------------------------------------${RESET}"
echo -e "${PURPLE}🧪 实验 3: 公网 HTTPS 性能与安全证书诊断 (Public HTTPS & Certs)${RESET}"
echo -e "${CYAN}----------------------------------------------------------------${RESET}"
echo -e "${YELLOW}【测试背景】${RESET} 在真实的 API 测试场景下，网络链路上往往会产生较大的波动。"
echo -e "网络诊断工具在这里能够提供完整的‘延迟瀑布图’以及‘X.509 证书深度分析’。"
echo -e "${YELLOW}【执行命令】${RESET} ${WHITE}\$ rupost diagnose https://httpbingo.org/get${RESET}"
echo -e "${YELLOW}【诊断进行中，请稍候...】${RESET}"
echo ""

# 运行公网 HTTPS 诊断
$RUPOST_BIN diagnose https://httpbingo.org/get
DIAGNOSE_STATUS=$?

if [ $DIAGNOSE_STATUS -ne 0 ]; then
    echo -e "${RED}[提示] 公网 HTTPS 探测未成功，可能因为当前网络受限。跳过此部分讲解。${RESET}"
else
    echo -e "\n${GREEN}【结果分析讲解】${RESET}"
    echo -e "1. ${BLUE}⏱️ Latency Breakdown 细粒度时延瀑布图:${RESET}"
    echo -e "   - ${BLUE}DNS Lookup:${RESET} 指将 httpbingo.org 通过本地域名解析出公网 IP 的开销（如 30ms ~ 100ms）。"
    echo -e "   - ${GREEN}TCP Connect:${RESET} 发发起目标公网 IP:443 端口的三次握手往返耗时（体现了物理距离和网络 RTT）。"
    echo -e "   - ${PURPLE}TLS Handshake:${RESET} HTTPS 所必需的。通过 ring 密码库与服务器完成密钥交换与安全信道初始化。由于涉及大量非对称加解密和证书传输，通常占比很高。"
    echo -e "   - ${YELLOW}HTTP TTFB:${RESET} 当发送完 HTTP GET 后，等待公网链路将首个字节数据返回给客户端的时间，代表了服务器的应用响应处理延迟。"
    echo -e "   - 终端柱状图直观地展示了哪一个阶段是性能瓶颈。比如，若 TCP 长但 TTFB 短，说明网络差，服务器快；若 TCP 短但 TTFB 极长，说明网络好，但服务器后端接口代码执行缓慢。"
    echo -e "2. ${BLUE}🛡️ TLS Certificate Info (证书状态诊断):${RESET}"
    echo -e "   - 自动获取证书链并调用 x509-parser 解析出公网证书的发行机构 (Issuer) 及域名信息 (Subject SAN)。"
    echo -e "   - ${GREEN}Status:${RESET} 诊断会自动计算证书过期时间与当前 UTC 时间的差值（剩余天数），若天数 > 30，显示 [VALID]；"
    echo -e "     若剩余天数 <= 30，显示黄色 [WARNING] 警告，提前避免线上证书突然过期导致的“雪崩”事故。"
fi
echo ""
read -p "按回车键 [Enter] 继续到实验 4..."

# 6. 实验四：演示异常网络连通性诊断 (错误处理)
echo -e "\n${CYAN}----------------------------------------------------------------${RESET}"
echo -e "${PURPLE}🧪 实验 4: 故意制造的连接拒绝异常诊断 (Connection Refused)${RESET}"
echo -e "${CYAN}----------------------------------------------------------------${RESET}"
echo -e "${YELLOW}【测试背景】${RESET} 当我们访问一个未启动服务的端口时，我们需要了解 RuPost 报告的精准性。"
echo -e "${YELLOW}【执行命令】${RESET} ${WHITE}\$ rupost diagnose http://127.0.0.1:9999/${RESET}"
echo ""

# 运行异常网络诊断
$RUPOST_BIN diagnose http://127.0.0.1:9999/
ERR_STATUS=$?

echo -e "\n${GREEN}【结果分析讲解】${RESET}"
echo -e "1. 诊断命令在遭遇拒绝连接（即目标端口未被任何程序监听）时，会立刻输出精准的底层错误原因："
echo -e "   ${RED}Error: TCP connection failed: Connection refused (os error 61)${RESET}"
echo -e "2. 这种明确的报错可以帮助开发者在几秒钟内断定故障根源为“端口未开启”或“服务挂掉”，而无需通过应用日志层层排查。"
echo ""

# 退出提示
echo -e "${CYAN}================================================================${RESET}"
echo -e "${GREEN}演示完毕！网络诊断功能的测试用例与说明文件已就绪。${RESET}"
echo -e "您可以随时查看说明文档: ${WHITE}examples/diagnose/README.md${RESET}"
echo -e "或者直接在您的命令行里运行: ${WHITE}rupost diagnose <url>${RESET}"
echo -e "${CYAN}================================================================${RESET}"
