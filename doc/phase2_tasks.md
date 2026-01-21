# Phase 2: 文档驱动测试 - 任务列表

## 阶段目标
实现 RuPost 的核心差异化功能：基于文档的 API 测试能力，支持从文件中读取、解析和批量执行 HTTP 请求。

---

## Stage 1: 请求文件格式设计 (Design & Spec)
> 目标：确定文件格式规范，为解析器提供明确的契约

- [ ] **Task 1.1**: 定义 `.http` 文件的最小语法规范
  - 支持的 HTTP 方法 (GET, POST, PUT, DELETE, PATCH)
  - Headers 格式 (key: value)
  - Body 格式 (JSON, plain text, form-data)
  - 请求分隔符 (`###`)
  - 注释语法 (`#`, `//`)
  
- [ ] **Task 1.2**: 设计元数据语法
  - `@name` - 请求名称
  - `@skip` - 跳过该请求
  - `@timeout` - 超时时间（可选）
  
- [ ] **Task 1.3**: 设计 Markdown 文件格式规范
  - 定义如何从 `.md` 文件中提取 HTTP 请求
  - 代码块语言标识符 (```http, ```rest)
  - 支持 Markdown 文档与可执行请求混合
  
- [ ] **Task 1.4**: 创建示例文件集
  - 创建 `examples/basic.http` - 基础 GET/POST 示例
  - 创建 `examples/advanced.http` - 包含 headers, body 的完整示例
  - 创建 `examples/multiple.http` - 多请求批量执行示例
  - 创建 `examples/api-docs.md` - Markdown 格式示例（文档 + 请求）

---

## Stage 2: 解析器核心实现 (Parser Module)
> 目标：实现将 `.http` 文件解析为内部数据结构的能力

- [ ] **Task 2.1**: 创建解析器模块结构
  - 创建 `src/parser/mod.rs`
  - 创建 `src/parser/http_file.rs` - `.http` 文件解析器
  - 创建 `src/parser/markdown_file.rs` - `.md` 文件解析器
  - 创建 `src/parser/types.rs` - 解析器数据结构定义
  
- [ ] **Task 2.2**: 定义解析器数据结构
  - `ParsedRequest` - 单个解析后的请求
  - `ParsedFile` - 整个文件的解析结果
  - `ParseError` - 解析错误类型
  
- [ ] **Task 2.3**: 实现基础解析逻辑
  - 按 `###` 分割多个请求块
  - 解析请求行 (方法 + URL)
  - 解析 Headers (key: value)
  - 解析 Body（支持空行后的 JSON/文本）
  
- [ ] **Task 2.4**: 实现元数据解析
  - 解析 `@name` 注释
  - 解析 `@skip` 注释
  - 将元数据附加到 `ParsedRequest`
  
- [ ] **Task 2.5**: 实现 Markdown 文件解析器
  - 提取 Markdown 中的代码块（```http, ```rest）
  - 将每个代码块视为独立的 HTTP 请求
  - 保留代码块前的 Markdown 标题作为请求名称
  - 处理嵌套代码块和转义字符
  
- [ ] **Task 2.6**: 添加解析器单元测试
  - 测试 `.http` 文件单个/多请求解析
  - 测试 `.md` 文件代码块提取
  - 测试边界情况 (空文件, 格式错误)
  - 测试元数据解析

---

## Stage 3: 请求构建器 (Request Builder)
> 目标：将解析结果转换为可执行的 `Request` 对象

- [ ] **Task 3.1**: 实现 `ParsedRequest` → `Request` 转换
  - 创建 `src/parser/builder.rs`
  - 实现 `RequestBuilder::from_parsed()` 方法
  - 处理默认值 (如缺失的 method 默认为 GET)
  
- [ ] **Task 3.2**: 实现 Body 类型推断
  - 自动检测 JSON body
  - 支持 plain text body
  - 支持 form-urlencoded (可选)
  
- [ ] **Task 3.3**: 添加构建器测试
  - 测试各种 HTTP 方法的转换
  - 测试 Headers 转换
  - 测试 Body 转换

---

## Stage 4: 批量执行引擎 (Test Runner)
> 目标：实现 `rupost test <file>` 命令，批量运行请求

- [ ] **Task 4.1**: 创建测试运行器模块
  - 创建 `src/runner/mod.rs`
  - 创建 `src/runner/executor.rs` - 批量执行逻辑
  - 创建 `src/runner/result.rs` - 执行结果类型
  
- [ ] **Task 4.2**: 实现文件加载与解析
  - 读取文件内容
  - 调用解析器
  - 错误处理 (文件不存在, 解析失败)
  
- [ ] **Task 4.3**: 实现顺序执行逻辑
  - 遍历 `ParsedFile` 中的所有请求
  - 跳过标记为 `@skip` 的请求
  - 为每个请求构建 `Request` 对象并执行
  
- [ ] **Task 4.4**: 实现执行结果收集
  - 定义 `TestResult` 结构 (name, status, duration, error)
  - 收集所有请求的执行结果
  - 统计成功/失败数量
  
- [ ] **Task 4.5**: 添加 CLI 命令集成
  - 在 `src/cli.rs` 中添加 `test` 子命令
  - 实现 `CliRunner::run_test_file()` 方法
  - 添加命令行参数 (如 `--verbose`, `--stop-on-error`)

---

## Stage 5: 断言系统 (Assertion Framework)
> 目标：支持对响应结果进行简单断言验证

- [ ] **Task 5.1**: 设计断言语法
  - 状态码断言: `@assert status == 200`
  - Body 包含断言: `@assert body contains "success"`
  - Header 断言: `@assert header Content-Type == "application/json"`
  
- [ ] **Task 5.2**: 实现断言解析器
  - 创建 `src/parser/assertion.rs`
  - 定义 `Assertion` 枚举类型
  - 实现断言语法解析逻辑
  
- [ ] **Task 5.3**: 实现断言执行器
  - 创建 `src/runner/assertion.rs`
  - 实现 `AssertionExecutor::check()` 方法
  - 支持状态码断言
  - 支持 Body 文本包含断言
  
- [ ] **Task 5.4**: 集成断言到测试运行器
  - 在执行请求后运行断言
  - 收集断言失败信息
  - 在 `TestResult` 中记录断言结果
  
- [ ] **Task 5.5**: 添加断言测试
  - 测试各种断言类型的解析
  - 测试断言执行逻辑
  - 测试断言失败场景

---

## Stage 6: 输出与报告 (Output & Reporting)
> 目标：提供清晰友好的测试结果输出

- [ ] **Task 6.1**: 实现测试结果格式化
  - 创建 `src/runner/reporter.rs`
  - 实现成功/失败的彩色输出
  - 显示每个请求的执行时间
  
- [ ] **Task 6.2**: 实现摘要报告
  - 显示总请求数、成功数、失败数
  - 显示总执行时间
  - 失败时显示详细错误信息
  
- [ ] **Task 6.3**: 添加详细模式
  - 实现 `--verbose` 模式
  - 详细模式下显示请求/响应内容
  - 显示断言检查详情
  
- [ ] **Task 6.4**: 设置正确的退出码
  - 所有测试成功: 退出码 0
  - 有测试失败: 退出码 1
  - 解析/执行错误: 退出码 2

---

## Stage 7: 端到端测试与验证
> 目标：确保整个功能链路正常工作

- [ ] **Task 7.1**: 创建集成测试
  - 创建 `tests/file_test.rs`
  - 准备测试用的 `.http` 文件
  - 测试完整的解析→执行→断言流程
  
- [ ] **Task 7.2**: 手动测试验证
  - 使用真实 API 测试 (如 httpbin.org)
  - 验证多请求批量执行
  - 验证断言成功/失败场景
  
- [ ] **Task 7.3**: 性能测试
  - 测试大文件解析性能
  - 测试批量请求执行性能
  
- [ ] **Task 7.4**: 文档更新
  - 更新 README.md 添加 `test` 命令使用说明
  - 创建 `.http` 文件格式文档
  - 添加示例和最佳实践

---

## 任务执行策略

### 优先级
1. **P0 (必须)**: Stage 1-4 (核心解析和执行能力)
2. **P1 (重要)**: Stage 5 (断言系统，差异化关键)
3. **P2 (优化)**: Stage 6-7 (用户体验和质量保证)

### 每个阶段的完成标准
- ✅ 代码实现完成
- ✅ 单元测试通过
- ✅ 功能可手动验证
- ✅ 代码通过 `cargo clippy` 检查

### 迭代方式
按 Stage 顺序推进，每完成一个 Stage 进行验证，确保可运行后再进入下一个 Stage。
