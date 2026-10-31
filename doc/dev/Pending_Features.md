# Pending Features (Unimplemented)

> **最后同步**: 2026-07-19 | 已实现项见 `doc/plans/progress_summary.md`

Extracted from `next_phase_plan.md.resolved` as of 2026-01-24，已根据实际进度更新。

## ✅ 已实现（原标记为未实现 / Pending）

| 原编号 | 功能 | 实际状态 | 交付物 |
|--------|------|----------|--------|
| P0-1 | Detailed Debug Mode (`--debug`, `--debug-on-failure`) | ✅ 已实现（部分） | `rupost diagnose <url>` — DNS/TCP/TLS/TTFB 瀑布图 + X.509 证书分析；`--debug` 全局标志已接入 |
| P1-4 | Request Dependency Control (`@depends-on`) | ✅ 已实现 | DAG 拓扑排序（Kahn 算法）+ 循环检测 + 状态克隆传递 |
| P1-5 (部分) | Output Format Enhancement — JSON Report | ✅ 已实现 | `--report json` + 结构化控制台摘要 |
| P2-7 (部分) | Parallel Execution Optimization | ✅ 已实现 | `--parallel` 模式 + `JoinSet` + 并发限制器 |

## 🔴 P1 — 仍未实现（高优先级）

### 1. HTML 可视化报告
- Sprint 6 遗留项
- 基于 `handlebars-rust` / `tera` 对 `BatchExecutor` JSON 结果渲染单页 HTML
- 用于 Jenkins / GitHub Pages / 团队展示

### ~~2. 动态变量~~ 已实现
- 已内置于 `src/variable/resolver.rs`：`$uuid` / `$timestamp` / `$random_int`（2026-10 勘误：旧文档的 `{{$randomInt}}` 写法有误，实现是下划线风格）

### 3. JUnit XML 报告
- `--format junit` 输出
- 企业 CI/CD 流水线集成必备（GitHub Actions, Jenkins）

## 🟠 P2 — 仍未实现（中优先级）

### 4. 条件执行 (`@skip-if`)
- 语法已预研
- 根据环境变量 / 用例执行状态动态跳过

### 5. 循环执行 (`@loop`)
- 语法已预研
- 对特定接口重复触发 N 次（压测 / 制造测试数据）

### 6. 并行执行进度条
- 并行模式已存在，缺终端进度反馈

### 7. `rupost history export`
- 事后快照创建（CLI 已定义，实现待完成）

## 🟡 P3 — 仍未实现（低优先级）

### 8. 变量加密
- SOPS-like `ENC[...]` 语法
- 敏感配置值（API keys）的加密存储与解析

### 9. 插件系统
- 自定义变量 / 断言 / Mock 生成器

### 10. 分布式追踪
- 自动 Trace ID 提取与 propagated
