# Stage 4 改进建议

## 设计改进

### 1. 使用更语义化的模块名
**当前计划**: `src/runner/`
**建议**: `src/executor/` 或 `src/batch/`

**理由**:
- `runner` 在 Rust 生态中通常指测试框架的运行器
- `executor` 更准确描述"批量执行请求"的职责
- `batch` 强调批量处理的特性

**推荐**: 保持 `runner`，因为：
- 符合原始设计文档
- 与 "Test Runner" 概念一致
- 未来可扩展为完整的测试框架

---

### 2. 执行结果的详细程度
**建议增强 `TestResult` 结构**:

```rust
pub struct TestResult {
    pub name: Option<String>,           // 请求名称（来自 @name 或自动生成）
    pub method: String,                 // HTTP 方法
    pub url: String,                    // 请求 URL
    pub status: u16,                    // 响应状态码
    pub duration: Duration,             // 执行耗时
    pub success: bool,                  // 是否成功（2xx 状态码）
    pub error: Option<String>,          // 错误信息（如果失败）
    pub request_number: usize,          // 请求序号
}
```

**优势**:
- 提供更丰富的测试报告信息
- 便于后续添加断言验证
- 支持生成详细的测试报告

---

### 3. 执行策略的可配置性
**建议添加执行选项**:

```rust
pub struct ExecutionOptions {
    pub stop_on_error: bool,           // 遇到错误是否停止
    pub parallel: bool,                // 是否并行执行（未来）
    pub max_concurrent: usize,         // 最大并发数（未来）
    pub timeout: Option<Duration>,     // 全局超时
    pub verbose: bool,                 // 详细输出模式
}
```

**当前实现**: 先实现基础版本（顺序执行，不停止）
**未来扩展**: 根据需要添加选项

---

### 4. 输出格式的优化
**建议**:
- 使用 `colored` crate 提供彩色输出
- 参考 Jest/Mocha 等测试框架的输出格式
- 区分成功（绿色）和失败（红色）

**示例输出**:
```
Running 4 requests from basic.http...

 ✓ [1/4] GET https://httpbin.org/get (234ms)
 ✓ [2/4] POST https://httpbin.org/post (456ms)
 ✓ [3/4] GET https://httpbin.org/get?name=Alice (123ms)
 ✓ [4/4] DELETE https://httpbin.org/delete (189ms)

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Summary
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Tests:     4 passed, 4 total
  Duration:  1.002s
```

---

### 5. 错误处理的细粒度
**建议区分错误类型**:

```rust
pub enum ExecutionError {
    ParseError(ParseError),          // 文件解析失败
    ConversionError(RupostError),    // 请求转换失败
    NetworkError(RupostError),       // 网络请求失败
    Timeout,                         // 请求超时
    Cancelled,                       // 用户取消
}
```

**优势**:
- 提供更准确的错误信息
- 便于调试和问题定位
- 支持不同的错误处理策略

---

### 6. 进度显示
**建议添加进度条**（可选）:
- 使用 `indicatif` crate
- 显示当前执行进度
- 实时更新执行状态

**优先级**: P2（优化项）
**原因**: 先实现核心功能，进度条可后续添加

---

## 实现优先级

| 功能 | 优先级 | 阶段 |
|------|--------|------|
| 基础批量执行 | P0 | Stage 4（本次） |
| 结果收集和统计 | P0 | Stage 4（本次） |
| 彩色输出 | P0 | Stage 4（本次） |
| CLI test 子命令 | P0 | Stage 4（本次） |
| `--stop-on-error` 选项 | P1 | 后续优化 |
| `--verbose` 选项 | P1 | 后续优化 |
| 进度条 | P2 | 后续优化 |
| 并行执行 | P2 | 未来扩展 |

---

## 待优化清单

### 当前阶段不实现的功能
1. **元数据解析**（@name, @skip）
   - 原因: 需要先完善解析器
   - 计划: Stage 2 第二迭代

2. **断言验证**（@assert）
   - 原因: 需要设计断言 DSL
   - 计划: Stage 5

3. **并行执行**
   - 原因: 增加复杂度，当前不是必需
   - 计划: 性能优化阶段

4. **请求依赖**（变量传递）
   - 原因: 需要设计变量系统
   - 计划: 高级功能阶段

5. **生成测试报告**（JSON/HTML）
   - 原因: 当前控制台输出已足够
   - 计划: Stage 6

---

## 技术债务记录

### 需要重构的地方
1. **`HttpFileParser` 空结构体**
   - 建议: 改为模块级函数
   - 影响: API 变更，需要更新调用处
   - 文档: `doc/parser_optimization_suggestions.md`

2. **错误类型的统一**
   - 当前: 多种错误类型混用
   - 建议: 建立统一的错误层次结构
   - 优先级: P2

3. **测试覆盖率**
   - 当前: 单元测试覆盖良好
   - 缺失: 集成测试较少
   - 建议: 增加端到端集成测试

---

## 下一步行动

1. ✅ 记录改进建议
2. ⬜ 创建 Stage 4 实现计划
3. ⬜ 实现批量执行引擎
4. ⬜ 添加 CLI test 子命令
5. ⬜ 测试验证
