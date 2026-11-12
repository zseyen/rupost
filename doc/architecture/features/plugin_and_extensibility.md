# 插件体系与开放扩展全貌 (Plugin Architecture & Extensibility)

> **状态更新（2026-11-13）**：本档案已按**已落地的 Stdio IPC 体系**全面重写。
> 旧版的 Wasm 运行时 / `plugin.toml` 权限模型 / network-fs-env 分级授权设计**已废弃**，
> 被 2026-11-10 立项、M1–M7 全量落地的 **静态 manifest + Unix Stdio IPC 进程管道**
> 体系取代（权威设计：`doc/plans/2026-11-10-cross-scenario-engine-and-plugins/03-plugin-spi-and-degradation.md`）。

---

## 1. 权威设计文档索引

| 主题 | 唯一事实来源 |
|---|---|
| MVP 范围与里程碑（M1–M7） | `doc/plans/2026-11-10-cross-scenario-engine-and-plugins/06-mvp-implementation-plan-personas-and-scenarios.md` |
| 插件 SPI / IPC 协议 / 降级语义 / 执行仲裁 | 同目录 `03-plugin-spi-and-degradation.md` |
| 黑匣子快照契约（Schema v1.0） | 同目录 `02-artifact-contract-for-plugins.md` |
| P0/P1 与 M6 对账底账 | 同目录 `p0-p1-remediation-plan.md` |
| 免重填/重放收口底账 | `doc/plans/2026-11-12-replay-reuse-backlog.md` |

## 2. 当前架构（已落地，M1–M7 全量交付）

### 2.1 插件形态（方案 D：Unix Stdio IPC 进程管道）

每个插件 = **可执行二进制/脚本 + 同目录静态 manifest（`rupost-plugin-<name>.json`）**：
- 发现 = readdir + JSON 解析，**零子进程**（绝不执行陌生二进制，防供应链 RCE）；
- 执行 = Stdio IPC 单 JSON 对象协议（stdin 请求上下文 → stdout 单 JSON Patch；`protocol_version: 1.0`）；
- 任意语言可写（Python/Go/Shell…），进程级故障隔离。

### 2.2 生命周期钩子（不可逆阶段流）

```
@auth (Prepare=10) → @transform (Transform=20) → @sign (Sign=30) → 物理网络发送
```
- 同阶段严格按 `.http` 书写行序 FIFO（无 priority 魔数）；
- `post_response`/`on_failure`/`harvest` 为 P2 保留字（manifest 声明即拒绝注册）；
- `cache_scope: none|request|suite`（suite 消除 OAuth2 类子进程风暴）。

### 2.3 双模入口

- **Hook 模式**：请求管线内 Stdio IPC（10MB 载荷上限、5s 进程组 SIGKILL 熔断、stderr 64KB 环形缓冲）；
- **Command 模式**：`rupost <custom-cmd>` 三向 TTY 直通 + TOFU 首次指纹确认 + 退出码/信号原值穿透（128+sig）。

### 2.4 运行时就绪（M6）

- manifest `runtime{interpreter,min_version,modules}` 声明 + readdir 级解释器预检（零子进程）；
- 三态状态机：`ready` / `pending`（解释器未找到，附三选一处方）/ `failed`（manifest 非法）——`plugin list` 可视化，响应式重算绝不持久化；
- 失败分类学：`timeout / invalid_output / crashed / exit_non_zero / runtime_missing / dependency_missing`（stderr 特征表提取缺失模块名入处方）。

### 2.5 安全不变量（全部落地）

| 防线 | 机制 |
|---|---|
| 发现即执行 | 静态 manifest，零子进程 |
| 项目级 RCE | `./.rupost/plugins` 默认隔离（`--allow-project-plugins`/env 通道放行） |
| 路径逃逸 | canonicalize 白名单 + 拒绝 symlink/`..` |
| 凭据外泄 | 子进程 env 白名单（PATH/HOME/LANG/TERM/TMPDIR/TZ + RUPOST_* + requires_env/`--plugin-env`） |
| 缓存投毒 | 0700 目录/0600 文件 + 属主校验 + mtime+前 4KB 双指纹 + 原子重命名 |
| 降级滥用 | 统一降级语义（原样外发 + 三处 WARNING + DegradedPass + `--strict-plugins` 门禁） |
| 终端注入 | manifest 文本 ANSI/OSC 净化 |

## 3. 插件作者指南（编写 → 安装 → 验证 → 调试）

1. **编写**：参考 `examples/plugins/rupost-plugin-echo-sign`（Python）与同名 manifest；
2. **放置**：`$PATH` 各目录 / `~/.rupost/plugins` / `./.rupost/plugins`（需放行）；
3. **验证**：`rupost plugin list`（三态 + 处方）、`rupost plugin schema`（指令/宏/manifest schema，供 IDE 补全）；
4. **调试**：插件 stderr 在失败时经 16KB 截断入 warnings（终端/报告/快照三处同文）；
5. **降级约定**：`?optional` 唯一声明语法；兜底鉴权唯一合法机制 = 用户手写静态 Header。

## 4. 登记不做项（P2/生态期，有裁决背书）

`@pipeline`/`@use` 语法、在线插件市场、热重载、MCP Server、RLIMIT 配额、`plugin sync --regenerate`、`plugin validate`/doctor、TUI/Web 的 `@` 补全、`fallback.builtin` 降级链、Windows shebang 透明代理。
## 5. 容器运行边界（C-1 review 登记）

- TLS 后端为 aws-lc-rs（reqwest `rustls` feature）——**musl 构建需 cmake + C 工具链**（CI musl-static job 已含）；
- reqwest HTTPS 证书源为系统 store（rustls-platform-verifier）：**scratch 镜像内需自带 CA bundle 或设 `SSL_CERT_FILE`**；Alpine（装 ca-certificates）不受影响；
- WSS 走 webpki-roots 内嵌 CA，无系统依赖。
