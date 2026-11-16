---
trigger: always_on
---

# Rust 智能体开发守则 (Rules)

1. **会话归档纪律**：每完成 7 次对话，必须总结当前状态与下一步计划，并持久化保存到 `checkpoint.md`。
2. **架构与编码标准**：遵循 Clean Architecture 架构与 Rust 最佳实践，严格做到数据与渲染分离、领域模型充血，杜绝外层闭包解构内部字段。
3. **单一事实来源 (DRY SSOT)**：系统核心知识（HTTP 标准方法、RFC 3986 URL Scheme、POSIX 路径归一化、实时文档扩展名、UTF-8 字符截断）全库仅保留一个权威定义，严禁重复书写字面量。
4. **TDD 驱动重构 (Red-Green-Refactor)**：重构或增加功能前必须先写测试锁定行为契约；守护测试必须经受反转生产代码必然变红的验证，禁止假守护。
5. **跨平台路径与安全性**：暴露至外部的相对路径必须统一经过 `normalize_posix_path`；严禁在沙箱之外使用未经校验的裸路径。
6. **JJ 原子化提交**：遵循原子化提交纪律，每次提交只包含当次变更，提交信息中的数字与行为变更必须通过实测验证。
7. **路径引入规范 (Clean Imports)**：禁止在函数体、结构体初始化或测试代码内直接书写冗长的全限定路径（如 `std::sync::Arc::new(...)`、`crate::web::store::LocalFsDocumentStore::new(...)`、`axum::http::header::COOKIE`）；必须统一在文件顶部或 `mod tests` 顶部通过 `use` 引入后使用短名称（仅保留消除同名符号冲突的极少数情况）。
