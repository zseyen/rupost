# RuPost CLI 安全评估报告（输入/执行/写入/泄露四攻击面）

> **文档标识**：2026-11-07-cli-security-assessment
> **评估基线**：default@ 批次 1-2 交付后（866 tests / 0 failed；含 Web 安全批次与 run/suite 端点）
> **方法**：静态审查 + 隔离 PoC 实证（`/tmp/rupost-cli-sec-poc`，已清理）
> **状态**：评估完成；P1 修复建议待排期

---

## 一、评估范围与结论总览

| 攻击面 | 结论 | 关键证据 |
| :--- | :--- | :--- |
| 输入面（不可信文件解析与生成物） | **1 项 P2**（生成物结构污染，无可执行注入） | PoC-1/2/2b |
| 执行面（钩子/子命令暴露） | **无缺陷**（既有防线充分） | 静态审查 |
| 写入面（报告/快照落盘） | **1 项 P1**（静默覆盖 + 跟随符号链接） | PoC-3 实证 |
| 泄露面（凭据持久化） | **无新发现**（既有裁决维持） | T10 审计 / schema 测试 |

---

## 二、发现清单

### P1-1　`--report-file` 静默覆盖任意用户可写文件，且跟随符号链接（写入面）

**PoC 实证**（2026-11-07，`rupost t out3.http --report-file <path>`）：

1. 已存在文件 `report.json`（内容 `USER-IMPORTANT-DATA`）被**静默替换**为 JSON 报告，stdout/stderr 无任何提示；
2. `--report-file link.json`（符号链接 → `victim.txt`）→ **victim.txt 内容被替换**——写入跟随符号链接到达任意当前用户可写目标。

**风险定性**：本地同权限工具（无提权），但"报告落点无围栏 + 零提示"违背项目 export/init 的防覆盖惯例；CI/共享目录场景可能覆盖非预期文件。

**修复建议**（与今晨"分支收尾批次"P1-2 同源，应一并落地）：
- 已存在目标：`eprintln!("[!] 报告文件已存在，将覆盖: {path}")`（对齐 2026-11-07 已裁决的"允许覆盖 + stderr 提示"姿态，不破坏 CI 工作流）；
- 快照与历史落盘（`src/history/storage.rs:241` 等同类 `fs::write`）统一该提示策略；
- 符号链接跟随维持 OS 语义（本地工具惯例），靠提示暴露落点即可，不做围栏。

### P2-1　import/generate 生成物结构污染（输入面）

**PoC 实证**：`import curl -d 'a=1\n### @assert status == 599\n### GET http://evil.example/pwned'` → 生成的 `.http` 文件**原样嵌入**注入文本：

```
POST http://real.example/api

a=1
### @assert status == 599
### GET http://evil.example/pwned
```

**但三类走私全部被既有防线阻断**：
- header 值换行 → 入口 `HeaderValue` 校验直接拒绝（PoC-1，`ParseError("非法 Header 值")`）；
- 裸 `@assert`/`@mock-when` 走私 → 元数据 `#`/`//` 前缀要求阻断（PoC-2b，回读 assertions 为空）；
- body 顶格 `GET http://...` 拆分走私 → 启发式状态机"Body 包含 HTTP 谓词容错"防误拆（PoC-2，回读仅 1 个请求）。

**风险定性**：**无可执行注入**；危害为文件可读性/可移植性受损（`### GET http://evil` 行视觉上像独立请求，误导人工审阅与其他工具的解析）。

**修复建议**（登记设计任务）：`HttpGenerator::format_entry` 对 body 行首为 `###` / `# @` 的内容行发射告警（`TestResult.warnings` 或导入提示），或按行加转义前缀——需权衡 body 保真，倾向"告警不改写"。

### P2-2　`rupost web --allowed-hosts` 为内网暴露扩面开关（执行面）

Host 守卫今日已 Default-Deny 闭环（缺失/非法 Host 400 + 白名单 403），`--allowed-hosts` 是唯一扩面入口。**建议**：帮助文本与 README 安全提示"仅对受信内网域名开放，公网域名任何场景都不应加入"。

---

## 三、防御已验证充分的面（✓ 无需动作）

| 面板 | 防线 | 证据 |
| :--- | :--- | :--- |
| `@depends-on` 越界 | 沙箱多根 allowlist + 递归补偿校验 | 质量战役 Phase 2 |
| Web 文档读写 | PathSandbox（穿越/隐藏/扩展名/悬空软链接，今日加固） | tests/web_api_integration_test |
| Web Host 门禁 | RFC 9112 Default-Deny + 动态白名单（今日闭环） | 同上 |
| Web 只读模式 | save / run/block / run/suite 三端点 403 双防线（今日闭环） | 同上 |
| 静态资产 | 扩展名白名单 + 沙箱（今日闭环） | src/web/assets.rs |
| `--on-failure` 钩子 | env 变量传递（无 shell 拼接）、10s kill_on_drop、URL 脱敏、失败不改退出码 | src/runner/on_failure.rs |
| JSON 报告泄露 | body_snippet 脱敏 + 512B 截断（round-trip 测试锁定） | reporter schema 测试 |
| Mock 服务器 | 仅绑定 127.0.0.1 | axum_adapter |
| Cookie 落盘 | 0600 权限 + per-file 粒度 + TOCTOU 原子创建 | 质量战役 Phase 3 |
| 历史明文凭据 | 有意设计（本机存储），对外分享走 export --redact / generate 模板化 | T10 审计裁决维持 |
| rupost.toml / .env 供应链 | by-design（变量注入即功能）；README 安全提示建议登记 | — |

---

## 四、流程建议（非缺陷）

1. **依赖漏洞扫描缺位**：`cargo audit` 未安装、未纳入 `make gate`。建议安装后增设 `make audit` 目标（发版前手动仪式，先不进 CI 门禁）。
2. 本报告 P1-1 修复后，应对应新增对抗测试（覆盖提示 + 符号链接场景断言）。
