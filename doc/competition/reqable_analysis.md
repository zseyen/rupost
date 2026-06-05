# Reqable 工具分析

## 概述

**Reqable** 是一个新一代的跨平台 HTTP API 调试和测试工具，定位为"all-in-one"解决方案，试图整合 Fiddler、Charles Proxy 和 Postman 的功能。

**官网**: https://reqable.com  
**定位**: API 调试代理 + REST 客户端  
**平台**: Windows, macOS, Linux, Android, iOS  
**技术栈**: Flutter + C++  
**价格**: 免费使用，无需登录  

---

## 核心功能分析

### 1. API 调试（MITM Proxy）

Reqable 的核心功能是作为一个中间人代理（MITM Proxy），这是它的最大亮点。

#### 协议支持
- ✅ HTTP/1.x
- ✅ HTTP/2
- ⚠️ HTTP/3 (QUIC) - 部分支持
- ✅ WebSocket
- ✅ HTTPS/TLS (TLSv1.1, TLSv1.2, TLSv1.3)

#### 代理模式
- HTTP/HTTPS/Socks4/Socks4a/Socks5
- 支持二级代理
- 系统代理集成

#### 流量操作能力
1. **Rewriting（重写）**
   - URL 重定向
   - 本地/远程映射
   - 修改请求/响应

2. **Scripting（脚本）**
   - 支持 Python 脚本
   - 自动化处理请求/响应

3. **Breakpoints（断点）**
   - 实时拦截请求/响应
   - 手动修改后再发送

4. **Replay（重放）**
   - 单个或批量请求重放

5. **Gateway（网关）**
   - 屏蔽/暂停特定请求

6. **Mirroring（镜像）**
   - 镜像映射配置

#### 移动设备调试
- ✅ Android 设备抓包
- ✅ iOS 设备抓包
- ✅ Remote Devices 功能
- ✅ Collaborative 模式（流量路由到PC）

---

### 2. API 测试

Reqable 也提供了类似 Postman 的 API 测试功能：

#### 核心测试功能
- ✅ 从抓包记录直接转为 API 请求
- ✅ 多标签页测试
- ✅ 批量编辑（query、headers、forms）
- ✅ 认证支持（API Key、Basic Auth、Bearer Token）
- ✅ 自动保存历史记录
- ✅ API 集合管理
- ✅ cURL 导入/导出
- ✅ Cookie 自动管理

#### 协作功能（v3.0+）
- ☁️ 云数据存储
- 🔄 多设备同步
- 👥 团队实时协作
- 📦 API Collections 共享
- 🌍 环境变量共享

---

### 3. 工具箱

内置多种实用工具：
- Base64 编解码
- URL 编解码
- MD5 计算器
- 时间戳工具
- JSON Viewer
- XML Viewer
- HEX Viewer
- Image Viewer
- Color Picker
- QR Code Generator

---

## 与其他工具对比

### Reqable vs Charles Proxy

| 维度 | Reqable | Charles Proxy |
|------|---------|--------------|
| **MITM 代理** | ✅ 完整支持 | ✅ 完整支持（经典） |
| **协议支持** | HTTP/1.x, HTTP/2, HTTP/3(部分) | HTTP/1.x, HTTP/2 |
| **SSL/TLS** | TLSv1.1-1.3 | TLSv1.0-1.3 |
| **断点调试** | ✅ | ✅ |
| **流量重写** | ✅ | ✅ |
| **脚本支持** | ✅ Python | ❌ 无 |
| **带宽限流** | ❓ 未提及 | ✅ 强大 |
| **API 测试集成** | ✅ 内置 | ❌ 需要其他工具 |
| **跨平台** | ✅ 全平台 | ⚠️ Win/Mac/Linux |
| **移动端** | ✅ Android/iOS App | ❌ 仅PC |
| **价格** | ✅ 免费 | ❌ 付费（$50+ 买断） |
| **UI** | 现代化（Flutter） | 传统 Java UI |

**总结**: Reqable = 现代化免费版的 Charles + API 测试功能

---

### Reqable vs Postman

| 维度 | Reqable | Postman |
|------|---------|---------|
| **API 测试** | ✅ 支持 | ✅ 核心功能 |
| **Collection** | ✅ 支持 | ✅ 强大 |
| **环境变量** | ✅ 支持 | ✅ 强大 |
| **脚本** | Python（代理层） | JavaScript（测试脚本） |
| **MITM 代理** | ✅ 核心功能 | ⚠️ 有限 |
| **抓包调试** | ✅ 强大 | ❌ 弱 |
| **Mock Server** | ❓ | ✅ |
| **监控** | ❌ | ✅ |
| **团队协作** | ✅ (v3.0+) | ✅ 强大 |
| **云同步** | ✅ (v3.0+) | ✅ 默认 |
| **离线使用** | ✅ | ⚠️ 需要账号 |
| **价格** | ✅ 免费 | ⚠️ 免费版功能受限 |

**总结**: Reqable = 免费的 Postman + 强大的代理抓包

---

### Reqable vs Hurl

| 维度 | Reqable | Hurl |
|------|---------|------|
| **类型** | GUI + 代理 | CLI + 文件 |
| **文件驱动** | ❌ | ✅ `.hurl` |
| **MITM 代理** | ✅ 核心 | ❌ |
| **断言测试** | ✅ GUI | ✅ 强大 |
| **CI/CD** | ⚠️ 有限 | ✅ 完美 |
| **版本控制** | ⚠️ 云同步为主 | ✅ 文件驱动 |
| **移动调试** | ✅ 强大 | ❌ |
| **学习曲线** | 低（GUI） | 中（DSL） |

**总结**: 完全不同的使用场景，Hurl 专注于测试，Reqable 专注于调试

---

## 与 RuPost 的对比

### Reqable 的优势

#### ✅ 1. 强大的 MITM 代理能力
- 实时抓包和流量分析
- 移动设备调试（Android/iOS）
- 流量重写、断点、脚本
- **这是 RuPost 不具备的**

#### ✅ 2. GUI 优先，用户友好
- 现代化界面（Flutter）
- 可视化操作
- 适合移动开发者和测试工程师

#### ✅ 3. 跨平台完整
- 包括移动端 App（Android/iOS）
- 真正的全平台覆盖

#### ✅ 4. 免费且功能完整
- 无需登录即可使用
- 核心功能免费

---

### RuPost 的优势

#### ✅ 1. 文档驱动测试（Reqable 不具备）
- **Markdown 原生支持**
- **文档即测试**
- 版本控制友好
- **这是 Reqable 的空白**

#### ✅ 2. CLI 优先，自动化友好
- 适合 CI/CD 集成
- 脚本化测试
- 无 GUI 依赖

#### ✅ 3. 文件基础架构
- `.http` 和 `.md` 文件
- Git 友好
- 可读性强

#### ✅ 4. 生产环境调试能力（规划中）
- **Trace ID 自动追踪**
- **Kubernetes 日志集成**
- **分布式追踪集成**
- **这是 Reqable 没有的**

---

## 市场定位差异

### Reqable 的目标用户

1. **移动应用开发者**
   - 需要抓包调试 App 流量
   - iOS/Android 开发

2. **前端开发者**
   - 调试浏览器请求
   - 分析网络性能

3. **测试工程师**
   - 需要 GUI 工具
   - 手动测试为主

4. **安全研究人员**
   - 流量分析
   - 漏洞挖掘

### RuPost 的目标用户

1. **后端开发者**
   - API 文档编写
   - 自动化测试

2. **DevOps/SRE**
   - 生产环境调试
   - Kubernetes 环境
   - 日志追踪

3. **开源项目维护者**
   - API 文档维护
   - 示例代码验证

4. **小型团队**
   - 偏好轻量级工具
   - 需要版本控制

---

## 功能对比矩阵

| 功能 | Reqable | RuPost | 说明 |
|------|---------|--------|------|
| **MITM 代理** | ✅ 强 | ❌ | Reqable 核心优势 |
| **移动设备调试** | ✅ | ❌ | Reqable 独有 |
| **流量重写** | ✅ | ❌ | Reqable 独有 |
| **Python 脚本** | ✅ | ❌ | Reqable 独有 |
| **Markdown 支持** | ❌ | ✅ | RuPost 独有 |
| **文件驱动测试** | ❌ | ✅ | RuPost 独有 |
| **CI/CD 集成** | ⚠️ 弱 | ✅ | RuPost 优势 |
| **版本控制** | ⚠️ 云同步 | ✅ 文件 | RuPost 优势 |
| **生产日志集成** | ❌ | ✅ | RuPost 规划独有 |
| **K8s 集成** | ❌ | ✅ | RuPost 规划独有 |
| **Trace ID 追踪** | ❌ | ✅ | RuPost 规划独有 |
| **GUI** | ✅ 强 | ❌ | Reqable 优势 |
| **CLI** | ⚠️ 有限 | ✅ | RuPost 优势 |
| **API 集合** | ✅ | ✅ | 都支持 |
| **环境变量** | ✅ | ✅ | 都支持（规划） |
| **断言** | ✅ GUI | ✅ 代码 | 方式不同 |
| **团队协作** | ✅ 云 | ✅ Git | 方式不同 |
| **免费使用** | ✅ | ✅ | 都免费 |

---

## 可借鉴之处

### 1. 从抓包转为测试

**Reqable 的创新**: 可以直接从抓包记录中生成 API 测试请求

**RuPost 可以借鉴**（Phase 4+）:
```bash
# 提供一个记录模式
rupost record --proxy 0.0.0.0:8888

# 自动生成 .http 文件
rupost export-recorded --output api-requests.http
```

---

### 2. 移动设备支持

**Reqable 的优势**: 原生移动端 App

**RuPost 可以考虑**:
- 提供 Web UI（可选）
- 通过浏览器访问（适合移动设备）

---

### 3. 工具箱集成

**Reqable** 提供了很多实用工具（Base64、MD5、JSON Viewer等）

**RuPost 可以考虑**:
```bash
rupost utils base64-encode "hello"
rupost utils json-format < response.json
rupost utils uuid
```

---

## 结论

### 市场定位

**Reqable 和 RuPost 服务不同的场景**:

| 场景 | 推荐工具 | 原因 |
|------|---------|------|
| 移动应用调试 | **Reqable** | MITM 代理、移动端 App |
| 前端网络调试 | **Reqable** | 流量分析、重写 |
| API 文档编写 | **RuPost** | Markdown、文档即测试 |
| CI/CD 自动化测试 | **RuPost** | 文件驱动、CLI |
| 生产环境调试 | **RuPost** | 日志集成、K8s 支持 |
| 版本控制 API 测试 | **RuPost** | `.http` 文件、Git 友好 |
| 团队协作（GUI） | **Reqable** | 云同步、实时协作 |
| 开源项目文档 | **RuPost** | Markdown、可执行示例 |

---

### RuPost 的差异化策略

Reqable 的存在**进一步验证了 RuPost 的差异化定位**:

1. ✅ **Reqable 擅长调试，RuPost 擅长测试和文档**
2. ✅ **Reqable 需要 GUI，RuPost 纯 CLI**
3. ✅ **Reqable 云同步，RuPost 文件驱动**
4. ✅ **Reqable 移动优先，RuPost 后端优先**

**两者可以互补**:
- 开发时用 Reqable 抓包和调试
- 测试时用 RuPost 文件化测试
- 文档时用 RuPost Markdown

---

### 市场空间

Reqable 填补了 "免费的 Charles + Postman" 的市场空间。

RuPost 填补了 "文档即测试 + 生产调试" 的市场空间。

**两者不冲突，市场足够大！**

---

## 参考资料

- [Reqable 官网](https://reqable.com)
- [Reqable GitHub](https://github.com/reqable)
- [Reqable vs Charles Proxy](https://www.browserstack.com/guide/charles-proxy-alternatives)
- [Reqable vs Postman](https://medium.com/@reqable)

---

**文档版本**: 1.0.0  
**最后更新**: 2026-01-22  
**作者**: RuPost Team
