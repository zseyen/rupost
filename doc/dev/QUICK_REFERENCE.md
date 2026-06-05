# RuPost 快速参考

**当前版本**: Phase 2 - Stage 4 完成
**更新时间**: 2026-01-22

---

## 🚀 快速开始

### 基本用法

```bash
# 批量测试（默认模式）
rupost test examples/basic.http

# 详细输出（显示完整响应）
rupost test examples/basic.http --verbose
rupost test examples/basic.http -v
```

---

## 📂 项目结构

```
src/
├── parser/          # 解析器（.http 文件 → ParsedRequest）
│   ├── types.rs     # 数据结构
│   ├── http_file.rs # HTTP 文件解析
│   ├── converter.rs # 请求转换器
│   └── mod.rs       # 模块入口
│
├── runner/          # 批量执行引擎
│   ├── types.rs     # TestResult, TestSummary
│   ├── executor.rs  # 批量执行逻辑
│   ├── reporter.rs  # 报告生成
│   └── mod.rs       # 模块入口
│
├── http/            # HTTP 客户端
│   ├── client.rs    # HTTP 客户端
│   ├── request.rs   # 请求封装
│   ├── response.rs  # 响应封装
│   └── types.rs     # HTTP 类型（Method, Url, Status）
│
├── utils/           # 工具模块
│   └── formatter.rs # 响应格式化
│
├── error.rs         # 错误类型
├── logger.rs        # 日志初始化
├── cli.rs           # CLI 参数解析
├── lib.rs           # 库入口
└── main.rs          # 程序入口
```

---

## ✅ 已实现功能

### 解析能力
- [x] `.http` 文件解析
- [x] 多请求支持（`###` 分隔）
- [x] HTTP 方法识别（GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS）
- [x] Headers 解析
- [x] Body 解析
- [x] 注释支持（`#`, `//`）

### 转换能力
- [x] ParsedRequest → Request 转换
- [x] 智能 Body 类型推断（JSON 自动检测）
- [x] 默认值处理（method 默认 GET）

### 执行能力
- [x] 批量执行（`rupost test <file>`）
- [x] 结果收集
- [x] 测试报告生成
- [x] 彩色输出
- [x] Verbose 模式
- [x] 失败自动详细输出

---

## ⚠️ 未实现功能

### 高优先级
- [ ] 元数据解析（`@name`, `@skip`, `@timeout`）
- [ ] 断言系统（`@assert`）
- [ ] 变量系统（`{{variable}}`）

### 中优先级
- [ ] Markdown 文件支持
- [ ] 环境管理（`.env`）
- [ ] 请求依赖

### 低优先级
- [ ] 并行执行
- [ ] JSON/HTML 报告
- [ ] 进度条

---

## 📊 测试状态

- **单元测试**: 33/33 通过 ✅
- **Clippy**: 无警告 ✅
- **端到端验证**: 成功 ✅

---

## 📝 重要文档

| 文档 | 路径 | 用途 |
|------|------|------|
| 进度总结 | `doc/progress_summary.md` | 全面进度概览 |
| 优先级待办 | `doc/todo_prioritized.md` | 技术债务清单 |
| 任务追踪 | `task.md` | 当前任务状态 |
| 完成报告 | `walkthrough.md` | Stage 4 完成总结 |
| 解析器优化 | `doc/parser_optimization_suggestions.md` | 重构建议 |
| Stage 4 改进 | `doc/stage4_improvements.md` | 设计改进 |

---

## 🎯 下一步建议

### 优先顺序

1. **元数据解析** (1-2 天)
   - 实现 `@name`, `@skip`, `@timeout`
   
2. **断言系统** (2-3 天)
   - 设计断言 DSL
   - 实现基础断言
   
3. **变量系统** (2-3 天)
   - 环境变量支持
   - 请求间变量传递

---

## 🔧 开发命令

```bash
# 编译
cargo build

# 运行测试
cargo test

# Clippy 检查
cargo clippy --all-targets -- -D warnings

# 运行示例
cargo run --example parse_and_execute
cargo run -- test examples/basic.http
cargo run -- test examples/basic.http --verbose

# 格式化代码
cargo fmt
```

---

## 📈 成熟度评估

- **核心功能**: ⭐⭐⭐⭐☆ (4/5)
- **测试覆盖**: ⭐⭐⭐⭐⭐ (5/5)
- **代码质量**: ⭐⭐⭐⭐☆ (4/5)
- **用户体验**: ⭐⭐⭐☆☆ (3/5)
- **文档完整**: ⭐⭐⭐☆☆ (3/5)

**总体**: ⭐⭐⭐⭐☆ (4/5) - 优秀的 MVP

---

## 💡 快速提示

- 默认模式适合 CI/CD（简洁输出）
- Verbose 模式适合调试（完整响应）
- 失败的请求会自动显示详细信息
- 测试失败时退出码为 1（CI 友好）

---

**Happy Testing! 🚀**
