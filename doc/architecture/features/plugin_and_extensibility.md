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

### 3.1 AST 桥：复用核心解析器（WP-AST，2026-11-13）

命令插件需要理解 `.http/.md` 文档时，**不要自行实现解析**（断言语义/变量约定极易与核心漂移）——经内核注入的 `RUPOST_BIN` 自调只读命令获取稳定 AST：

```bash
rupost "$RUPOST_BIN" ast suite.http        # → AstDocument JSON v1.0（stdout 纯 JSON）
```

* **契约**：顶层 4 键（`schema_version`/`source_path`/`dependencies`/`requests`）；请求级含 `method`/`url`（**未插值原文**，`{{var}}` 原样）/`headers`（保序）/`body`/`name`/`skip`/`assertions`（原文）/`captures`（原始声明）/`source_line`（方法行 1-based 行号）；
* **版本协调**：读后第一步校验 `schema_version` 前缀 `"1."`；1.x 内字段只增不改——按已知键白名单取用，容忍未知字段；作者六条守则与错位场景见评估报告 §8；
* **`RUPOST_BIN`**：内核执行插件命令时注入（`current_exe()`，取不到时退化 `rupost`）——永远用它而不是裸 `rupost`，杜绝 PATH 陈旧二进制漂移；
* **`ast` 是只读命令**：不发网络、不写历史；插件内自调时必须捕获子进程 stdout（自己的 stdout 只允许输出最终产物）。

### 3.2 命令型插件（WP-PL1，2026-11-13）

**两种插件形态矩阵**——先选形态再动手：

| | hook 型（请求管线） | command 型（CLI 工具） |
| :-- | :-- | :-- |
| 扩展什么 | 请求管线行为（签名/认证/改写） | CLI 工具（任意转换/生成） |
| 执行时机 | 每个请求发包前（prepare/transform/sign） | 用户敲 `rupost <命令名>` 时 |
| 能改什么 | headers/body/变量（**不可**改 URL/method，**不**消费响应） | 一切（独立进程，stdio 直通可交互） |
| 入口声明 | manifest `hooks`（`@<directive> <alias>`） | manifest `commands`（顶层子命令路由） |
| 典型用途 | SigV4、OAuth 注入 | 文档转换、代码生成 |

**命令插件五步**（最小示例：`examples/plugins/rupost-plugin-cmd-hello`，sh + manifest 两个文件）：

1. **写可执行文件**（任意语言；与 manifest 同目录、同名去 `.json`；Windows 作者提供 `.cmd`/`.exe` 同名包装或用 WSL——发现器探测三后缀）；
2. **写同名静态 manifest**（`manifest_version` 恒 "1.0"、description ≤80 字符、文件名必须 `rupost-plugin-*.json`——发现器按此约定扫描）；
3. **放置**：`$PATH` / `~/.rupost/plugins` / 项目 `./.rupost/plugins`（`--allow-project-plugins` 放行；验证命令用此形态时旗标写法为 `rupost plugin --allow-project-plugins list`）；
4. **验证**：`rupost plugin list`（✓ ready）；`rupost --help` Extensions 区块收录（PATH/home 安装形态）；
5. **首跑**：TOFU 交互确认一次（路径 + SHA-256 指纹入 `~/.rupost/tofu-commands.txt`）；CI/非 TTY 先在交互终端确认，否则 fail-closed。

**进阶（解析零分叉）**：需要理解 `.http/.md` 的工具，经 `$RUPOST_BIN ast <file>` 消费核心 AST（§3.1）——不要自行实现解析。

**扩展缝指引**：`import`/`export` 管线**不开放**插件扩展点（2026-11-13 评估裁决）；转换类工具一律走命令插件形态。

**社区扩展方向参考**（2026-11-13 范围裁决移交社区）：OpenAPI 导入（建议命令名 `openapi2md`）与 k6 场景草稿（`k6gen`）——设计骨架与核心侧现场核验（importer/exporter 字段、变量插值语义）见 `doc/plans/2026-11-13-phase01-remediation-plan.md` 暂缓节与 [插件承载能力评估](../../doc/plans/2026-11-13-plugin-readiness-assessment.md) §6；使用方案与版本协调见同报告 §7/§8。

## 4. 登记不做项（P2/生态期，有裁决背书）

`@pipeline`/`@use` 语法、在线插件市场、热重载、MCP Server、RLIMIT 配额、`plugin sync --regenerate`、`plugin validate`/doctor、TUI/Web 的 `@` 补全、`fallback.builtin` 降级链、Windows shebang 透明代理。
## 5. 容器运行边界（C-1 review 登记）

- TLS 后端为 aws-lc-rs（reqwest `rustls` feature）——**musl 构建需 cmake + C 工具链**（CI musl-static job 已含）；
- reqwest HTTPS 证书源为系统 store（rustls-platform-verifier）：**scratch 镜像内需自带 CA bundle 或设 `SSL_CERT_FILE`**；Alpine（装 ca-certificates）不受影响；
- WSS 走 webpki-roots 内嵌 CA，无系统依赖。

## 6. Web 工作台的辅助定位与扩展缝（2026-11-13 定位裁决）

**定位裁决**：Web 层（`src/web/`）是**辅助执行 + 辅助创建**的薄表现层——与 CLI/TUI 平级，
共享同一领域引擎（`execute_all` / `create_incident_bundle` 等领域函数为唯一事实来源），
自身永不拥有执行逻辑、永不演化为平行产品。预留扩展缝的目的：后续接入者挂载更复杂的
功能时**接在缝上**，而不是让 Web 层自我膨胀成平台。

### 6.1 两组既有职责

| 定位 | 既有端点/能力 |
|---|---|
| 辅助执行 | `/api/v1/run/block`（单块调试）、`/api/v1/run/suite`（整篇套件）、`/api/v1/mock/simulate`（规则仿真）、套件失败复现包（`incident` 开关，见 `doc/plans/2026-11-13-phase01-remediation-plan.md` WP-R2） |
| 辅助创建 | `/api/v1/files` + `/save`（活文档读写）、`/api/v1/import` + `/import/curl`（curl → 可执行 .http） |

### 6.2 四条预留扩展缝

| # | 缝 | 接口 | 现有实现 | 接入形态 |
|---|---|---|---|---|
| 1 | 前端整体可替换 | `web::AssetProvider` trait | `EmbeddedAssetProvider`（内嵌）/ `LocalDirAssetProvider`（`--static-dir`） | 零 Rust 改动挂载第三方前端 |
| 2 | 文档来源可替换 | `web::DocumentStore` trait | `LocalFsDocumentStore` | 新存储实现（Git/远端/DB），端点零改动 |
| 3 | HTTP API 本身 | `/api/v1/*` + schema 只追加纪律（`#[serde(default)]`，不改既有字段语义） | `ApiResponse` 信封 | 第三方程序（IDE 插件/CI 看板/脚本）直接消费 |
| 4 | 引擎级扩展 | 插件 Stdio IPC（本文 §2） | hooks/commands/失败分类学 | 执行期行为的唯一入口；经 `build_interactive_executor` 统一装配，**CLI/TUI/Web 三入口同时受益** |

### 6.3 接入路由准则

| 想加的能力 | 接哪条缝 | 反例（错误做法） |
|---|---|---|
| 更丰富的 UI/交互 | 缝 1（`--static-dir` 换前端） | 往内嵌 `app.js` 堆功能 |
| 文档存取新来源 | 缝 2（实现 `DocumentStore`） | handler 内写死新存储 |
| 脚本/IDE/看板自动化 | 缝 3（消费 `/api/v1`） | 把看板内嵌进 RuPost |
| 执行期行为（签名/认证/改写/钩子） | 缝 4（插件） | runner/web 加特判 |
| 新工件格式/导出 | 领域层新模块（`history/incident.rs` 先例） | 在 web handler 落盘 |
| 需要客户端给路径/密钥的能力 | CLI（信任边界正确的入口） | 塞进 Web API |

**核心准则一句话**：能力放进领域层或插件，入口挂在表现层，Web 永远只做"已有能力的薄封装"。

### 6.4 两条守门红线

1. **Web API 不接收文件系统路径、不接受密钥值**——任何新端点想要这两样，说明能力属于 CLI 或信任边界需要重设计。范例：复现包开关（WP-R2）把能力压缩为 1 bit，路径由服务端管辖（`<working_dir>/.rupost/incidents/`），密钥值通道恒关（浏览器不是可信 secret 通道）。
2. **表现层不许出现第二份领域逻辑**——依赖方向机械化守卫：`history` 不得依赖 `web/cli/tui`；handler 只解析与调用领域函数；接手时用 `rg` 抽查依赖方向。
