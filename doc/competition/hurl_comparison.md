# Hurl vs RuPost 功能对比分析

## 概述

本文档对比分析 **Hurl** 和 **RuPost** 的功能特性，识别差异化优势和可借鉴之处，为 RuPost 的发展提供参考。

---

## Hurl 核心特性总结

### 基本信息
- **官网**: https://hurl.dev
- **语言**: Rust
- **定位**: 基于 libcurl 的 HTTP 测试工具
- **文件格式**: `.hurl` 纯文本格式

### 核心功能

#### 1. 文件格式
```hurl
# 注释以 # 开头
GET https://example.org/api
x-api-key: secret
HTTP 200  # 隐式断言状态码

[Asserts]  # 显式断言块
header "Content-Type" contains "application/json"
jsonpath "$.user.name" == "John"
```

**特点**:
- 使用 `#` 作为注释
- 请求和响应在同一文件中定义
- 明确的请求行 + Headers + Body 结构
- 使用 `[Asserts]` 块组织断言

#### 2. 断言系统（非常强大）

**隐式断言**:
- HTTP 版本和状态码（必须）
- Headers（可选）
- Body（可选）

**显式断言**（`[Asserts]` 块）:
支持非常丰富的查询和断言类型：

| 查询类型 | 用途 | 示例 |
|---------|------|------|
| `status` | 状态码 | `status == 200` |
| `header` | 响应头 | `header "Content-Type" contains "json"` |
| `cookie` | Cookie | `cookie "session" exists` |
| `body` | 响应体 | `body contains "success"` |
| `jsonpath` | JSON 查询 | `jsonpath "$.user.id" == 123` |
| `xpath` | XML/HTML 查询 | `xpath "//h1" exists` |
| `regex` | 正则匹配 | `regex "\d{4}" exists` |
| `bytes` | 字节流 | `bytes startsWith hex,efbbbf;` |
| `duration` | 性能测试 | `duration < 1000` |
| `certificate` | SSL 证书 | `certificate "Subject" contains "example.org"` |

**谓词函数**:
- 比较: `==`, `!=`, `>`, `>=`, `<`, `<=`
- 字符串: `startsWith`, `endsWith`, `contains`, `matches`
- 存在性: `exists`, `not exists`
- 类型检查: `isBoolean`, `isString`, `isInteger`, `isFloat`, `isList`, `isObject`
- 特殊: `isEmpty`, `isIpv4`, `isIpv6`, `isIsoDate`, `isUuid`

#### 3. 变量与模板化
- 支持变量定义和使用
- 支持变量捕获（从响应中提取值用于后续请求）
- 模板替换

#### 4. 请求链
- 单个文件中顺序执行多个请求
- 请求间可以共享变量（捕获-传递）

#### 5. 其他高级功能
- Cookie 管理
- 重定向跟随
- 请求重试
- 并行执行多个 .hurl 文件
- CI/CD 集成友好
- 性能检查（duration 断言）
- 基于 libcurl（支持 HTTP/3, IPv6）

---

## RuPost 当前计划

### 核心定位
- **基于文档的 API 测试工具**
- 兼具 curl/httpie 即时性与 Postman 文档管理能力
- **差异化**: 支持 `.http` 和 `.md` 双格式

### Phase 2 计划功能

#### 1. 文件格式
- **`.http` 格式**: 类似 IntelliJ HTTP Client
- **`.md` 格式**: Markdown 文档 + 可执行请求（Literate Programming）

#### 2. 断言系统（初期）
- 状态码断言: `@assert status == 200`
- Body 包含断言: `@assert body contains "success"`
- Header 断言: `@assert header Content-Type == "application/json"`

#### 3. 元数据
- `@name` - 请求名称
- `@skip` - 跳过请求
- `@timeout` - 超时设置

#### 4. 批量执行
- `rupost test <file>` 命令
- 顺序执行，初期不共享上下文

---

## 功能对比矩阵

| 功能维度 | Hurl | RuPost (Phase 2 计划) | 差距分析 |
|---------|------|-------------------|---------|
| **文件格式** | `.hurl` 专用格式 | `.http` + `.md` 双格式 | ✅ RuPost 更灵活，支持文档化 |
| **注释语法** | `#` | `#` / `//` | ≈ 相似 |
| **请求分隔** | 默认（无需显式分隔符） | `###` 分隔符 | ⚠️ Hurl 更简洁 |
| **断言块** | `[Asserts]` 专用块 | `@assert` 注释形式 | ⚠️ Hurl 更结构化 |
| **状态码断言** | ✅ 隐式 + 显式 | ✅ 显式 | ≈ 相似 |
| **Header 断言** | ✅ 强大（exists, contains, ==） | ✅ 基础（==） | ⚠️ Hurl 更强 |
| **Body 断言** | ✅ 多种（contains, matches, ==） | ✅ 基础（contains） | ⚠️ Hurl 更强 |
| **JSONPath 查询** | ✅ 完整支持 | ❌ 未规划 | 🚨 **重要缺失** |
| **XPath 查询** | ✅ 完整支持 | ❌ 未规划 | ⚠️ 对 HTML/XML 测试重要 |
| **正则表达式** | ✅ 完整支持 | ❌ 未规划 | ⚠️ 通用性强 |
| **类型检查断言** | ✅ 丰富（isString, isInteger 等） | ❌ 未规划 | ⚠️ 有助于数据验证 |
| **变量捕获** | ✅ 支持 | ❌ Phase 3 规划 | ⚠️ 延后实现 |
| **变量替换** | ✅ 支持 | ✅ Phase 3 规划 | ⚠️ 延后实现 |
| **性能断言** | ✅ duration 断言 | ❌ 未规划 | ⚠️ 有价值 |
| **Cookie 管理** | ✅ 自动处理 | ❌ 未规划 | ⚠️ 会话测试需要 |
| **请求链上下文** | ✅ 变量传递 | ❌ 初期不支持 | ⚠️ 延后实现 |
| **并行执行** | ✅ 支持 | ❌ 未规划 | ⚠️ 性能优化需要 |
| **Markdown 支持** | ❌ 不支持 | ✅ **核心差异化** | ✅ RuPost 独有 |
| **文档化能力** | ⚠️ 注释为主 | ✅ Markdown 原生支持 | ✅ RuPost 更强 |

---

## 优劣势分析

### Hurl 的优势

#### ✅ 强大的断言系统
- **JSONPath/XPath 查询**: 可以精确提取和验证复杂数据结构
- **丰富的谓词函数**: 18+ 种类型检查和匹配函数
- **性能测试**: duration 断言可以做简单的性能监控

#### ✅ 成熟的请求链支持
- 变量捕获和传递机制
- Cookie 自动管理
- 适合复杂的工作流测试

#### ✅ 生产就绪
- 基于 libcurl，稳定可靠
- CI/CD 集成友好
- 并行执行支持

#### ✅ 简洁的语法
- 无需显式请求分隔符
- 隐式断言减少冗余

### Hurl 的劣势

#### ❌ 文档化能力弱
- 只能通过注释添加说明
- 不适合作为 API 文档
- 无法嵌入图片、链接等富文本内容

#### ❌ 单一格式
- 只支持 `.hurl` 格式
- 无法利用现有 Markdown 生态

---

### RuPost 的优势

#### ✅ 文档即测试（Literate Programming）
- **Markdown 支持**: 真正的文档与测试融合
- 可以嵌入说明、示例、图表
- 适合 API 文档场景

#### ✅ 双格式支持
- `.http`: 适合纯测试场景
- `.md`: 适合文档化场景
- 用户可以根据需求选择

#### ✅ 兼容主流生态
- `.http` 格式被 JetBrains、VS Code 支持
- 易于在现有工具链中集成

### RuPost 的劣势（Phase 2 计划）

#### ⚠️ 断言系统较弱
- 缺少 JSONPath/XPath 查询
- 缺少类型检查断言
- 缺少正则表达式匹配

#### ⚠️ 缺少高级功能
- 变量捕获延后到 Phase 3
- 无性能测试能力
- 无 Cookie 管理

#### ⚠️ 请求链能力有限
- 初期不支持请求间变量传递
- 不适合复杂工作流测试

---

## 建议与改进方向

### 🎯 优先级 P0（必须在 Phase 2 考虑）

#### 1. 增强断言系统
**建议**: 在 Stage 5（断言系统）中增加以下功能：

```markdown
- [ ] **Task 5.6**: 支持 JSONPath 查询断言
  - 依赖: `serde_json_path` crate
  - 语法: `@assert jsonpath "$.user.id" == 123`
  - 类型检查: `@assert jsonpath "$.items" isList`
  
- [ ] **Task 5.7**: 支持正则表达式断言
  - 依赖: `regex` crate（已规划）
  - 语法: `@assert body matches "\d{4}-\d{2}-\d{2}"`
  
- [ ] **Task 5.8**: 支持类型检查断言
  - `isString`, `isInteger`, `isBoolean`, `isList`, `isObject`
  - 语法: `@assert jsonpath "$.age" isInteger`
```

**影响**: 
- 大幅提升断言能力，与 Hurl 接近
- JSONPath 对 JSON API 测试至关重要
- 实现难度中等（利用现有库）

---

### 🎯 优先级 P1（Phase 2 后期或 Phase 3）

#### 2. 性能断言
```markdown
- [ ] **Task X.1**: 支持响应时间断言
  - 记录每个请求的执行时间
  - 语法: `@assert duration < 1000`  # 毫秒
```

#### 3. Header 断言增强
```markdown
- [ ] **Task X.2**: 丰富 Header 断言谓词
  - 支持 `contains`, `startsWith`, `endsWith`, `matches`
  - 语法: `@assert header Content-Type contains "json"`
```

---

### 🎯 优先级 P2（Phase 3+，可选）

#### 4. Cookie 管理
```markdown
- [ ] 自动处理 Set-Cookie 和 Cookie header
- [ ] Cookie 断言: `@assert cookie "session" exists`
```

#### 5. 并行执行
```markdown
- [ ] 支持 `rupost test --parallel` 并行运行多个文件
```

#### 6. XPath 支持
```markdown
- [ ] 对 HTML/XML 响应支持 XPath 查询
- [ ] 依赖: `xpath_reader` 或类似 crate
```

---

## 差异化定位建议

### 核心定位调整

**当前**: "基于文档的 API 测试工具"

**建议**: **"API 文档即测试平台"**

**核心差异化**:
1. **Markdown 原生支持** - Hurl 不具备
2. **文档与测试融合** - 真正的 Literate Programming
3. **双模式适配** - `.http` 纯测试 + `.md` 文档化

### 目标用户区分

| 用户类型 | 推荐工具 | 原因 |
|---------|---------|------|
| 纯后端测试，复杂工作流 | Hurl | 成熟的变量捕获、请求链 |
| API 文档编写 + 测试验证 | **RuPost** | Markdown 支持，文档即测试 |
| 快速临时测试 | curl/httpie | 无需文件 |
| 前端开发者，需要文档 | **RuPost** | Markdown 友好，易读易写 |
| CI/CD，性能要求高 | Hurl | 并行执行，duration 断言 |

---

## 实施建议

### 短期（Phase 2）

1. ✅ **保持计划中的核心功能**（文件解析、批量执行、基础断言）
2. ✅ **增加 JSONPath 断言支持**（P0，关键差距）
3. ✅ **增加正则表达式断言**（P0，已有依赖规划）
4. ⚠️ **考虑调整断言语法为块形式**（可选，长期维护性更好）

### 中期（Phase 3）

1. 实现变量捕获和替换（已规划）
2. 增加性能断言（duration）
3. Cookie 自动管理
4. 丰富 Header 断言谓词

### 长期（Phase 4+）

1. XPath 支持（HTML/XML 测试）
2. 并行执行
3. 更多类型检查断言
4. 高级报告（JUnit XML、HTML 报告）

---

## 结论

### RuPost 的独特价值

RuPost 不应该成为"另一个 Hurl"，而应该专注于 **文档即测试** 的差异化定位：

✅ **保持优势**:
- Markdown 原生支持
- 双格式灵活性
- 更好的文档化能力

✅ **补齐短板**:
- JSONPath 查询（必须）
- 正则表达式断言（必须）
- 类型检查断言（重要）
- 性能断言（有价值）

✅ **不必跟随**:
- XPath（非核心场景）
- 并行执行（初期不重要）
- SSL 证书断言（高级场景）

### 最终定位

> **RuPost: 让 API 文档变得可测试，让 API 测试变得可阅读**
> 
> - 给前端开发者：用 Markdown 写 API 文档，顺便跑测试
> - 给后端开发者：用 `.http` 文件快速测试，支持断言验证
> - 给团队：文档即代码，测试即文档，版本控制友好

---

## 附录：推荐依赖

```toml
[dependencies]
# 现有
pulldown-cmark = "0.9"  # Markdown 解析
regex = "1.10"           # 正则表达式

# 新增建议
serde_json_path = "0.6"  # JSONPath 查询（轻量级）
# 或者
jsonpath-rust = "0.3"    # 另一个 JSONPath 实现

# 可选（Phase 3+）
cookie_store = "0.20"    # Cookie 管理
```
