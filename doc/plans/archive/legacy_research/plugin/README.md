# RuPost 插件系统文档

> 📚 **文档版本**: v1.0  
> 📅 **最后更新**: 2026-02-03

---

## 📖 文档导航

### 🎯 规划与设计
- [可行性报告](feasibility_report.md) - 技术可行性、商业价值、实施路线图
- [架构设计](architecture.md) - 插件系统整体架构与技术选型
- [安全机制](security.md) - 沙箱隔离、权限系统、代码签名
- [生命周期管理](lifecycle.md) - 插件加载、初始化、卸载流程

### 🔧 API 参考
- [核心 API 参考](api_reference.md) - 插件开发核心接口定义
- [权限系统](permissions.md) - 权限模型与声明规范
- [元数据规范](metadata_spec.md) - `plugin.toml` 配置格式

### 🚀 开发指南
- [快速开始](quickstart.md) - 5 分钟创建第一个插件
- [示例插件](examples/) - 官方示例代码

---

## 📂 目录结构

```
doc/plugin/
├── README.md               # 本文档（文档索引）
├── feasibility_report.md   # 可行性报告
├── architecture.md         # 架构设计
├── security.md             # 安全机制
├── lifecycle.md            # 生命周期管理
├── api_reference.md        # API 参考
├── permissions.md          # 权限系统
├── metadata_spec.md        # 元数据规范
├── quickstart.md           # 快速开始
└── examples/               # 示例代码
    ├── custom-formatter/
    ├── header-injector/
    └── auth-helper/
```

---

## 🎯 文档状态

| 文档 | 状态 | 优先级 |
|------|------|--------|
| 可行性报告 | ✅ 已完成 | P0 |
| 架构设计 | ✅ 已完成 | P0 |
| API 参考 | ✅ 已完成 | P0 |
| 安全机制 | ✅ 已完成 | P1 |
| 权限系统 | ✅ 已完成 | P1 |
| 元数据规范 | ✅ 已完成 | P1 |
| 生命周期管理 | ✅ 已完成 | P2 |
| 快速开始 | ✅ 已完成 | P0 |
| 示例代码 | 🔄 计划中 | P1 |

---

## 🔗 相关资源

### 外部参考
- [Wasmtime 官方文档](https://docs.wasmtime.dev/)
- [WASM Component Model](https://github.com/WebAssembly/component-model)
- [wit-bindgen 指南](https://github.com/bytecodealliance/wit-bindgen)

### 项目相关
- [项目规划](../project_next_state.md)
- [RuPost README](../../README.md)

---

## 🤝 贡献指南

如果你想为插件系统文档做贡献：
1. 遵循现有文档的 Markdown 风格
2. 使用清晰的代码示例
3. 保持文档简洁且实用
4. 提交 PR 前运行文档链接检查

---

**文档维护者**: RuPost 核心团队  
**反馈渠道**: GitHub Issues
