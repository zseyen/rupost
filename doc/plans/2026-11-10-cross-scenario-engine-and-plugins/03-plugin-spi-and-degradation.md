# 03 - 插件 SPI 体系、统一注册表与生命周期管线规范

- **状态**: Draft (v2.1.0 — 插件发现/IPC 协议/降级语义/执行仲裁唯一权威 SSOT；评审通过后回标 Approved)
- **归属任务**: `cross-scenario-engine-and-plugins`
- **权威声明**: **本文件是插件发现机制、Stdio IPC 协议、降级语义与执行仲裁的唯一事实来源**。04 的测试设计、05 的相关表述、06 的实现任务以本文件为准。
- **核心原则**: **"开发期全能扩展，调试期优雅降级不静默，运行期硬性零 AI，架构级单一事实来源"**
- **范围标签约定**: `[已有]` / `[待建 M<N>]` / `[P2]`；MVP 范围以 06 §3 为唯一裁决。

---

## 1. 异构环境冲突场景与降级核心矛盾

在真实研发旅程中经常发生以下断裂：
* 开发者在本地 Mac 上写了测试用例，使用了外部插件（如 `@auth aws-sigv4`，依赖本地 Python 和 boto3）；
* 生产报警后，运维或开发把这套 `.http` 用例直接扔上了一台极简的 Linux 生产跳板机，使用 `rupost` 单二进制快速排查；
* **致命矛盾**：如果因为生产机器没有 Python 或没装 `rupost-plugin-aws-sigv4`，`rupost` 就直接 Panic 报错退出，会导致**整个现场排障直接瘫痪**！

---

## 2. 声明式降级策略与自愈机制（唯一权威语义）

为了让测试用例具备跨环境的自愈能力，`rupost` 提供两层降级防线：

```
                              [ 执行带有插件调用的测试用例 ]
                                             │
                                             ▼
                       [ PluginRegistry 缓存/目录 stat 探测 (零子进程) ]
                                             │
                      ┌──────────────────────┴──────────────────────┐
                      ▼                                             ▼
                【探测成功】                                   【插件未命中 / 产出失败】
                      │                                             │
                      ▼                                             ▼
             [ 执行外部 Stdio IPC 管道 ]              ┌───────────────────────────┐
             计算动态签名，注入 Header                │ 检查降级开关:              │
                                                      │ 1. 用例显式 ?optional     │
                                                      │ 2. CLI --allow-missing-   │
                                                      │    plugins                │
                                                      └─────────────┬─────────────┘
                                                                    │
                                     ┌──────────────────────────────┴──────────────────┐
                                     ▼                                                 ▼
                               【允许降级】                                     【未声明降级 (默认)】
                                     │                                                 │
                                     ▼                                                 ▼
     [ 插件不注入任何变更；请求按 .http 文件原样外发 ]              [ 友好报错退出 (exit 1)， ]
     [ 终端/报告/快照三处 WARNING；快照记 DegradedPass ]             列出插件搜索路径清单 ]
     [ 结束时汇总："N 个请求以降级形态发送" ]
```

### 2.1 统一降级语义（裁决，消除多文档漂移）

1. **触发条件**：插件未命中（缺失）**或**插件产出失败（超时/崩溃/非法输出/exit≠0，见 §8.3 失败分类学）——**两类共享同一组降级开关**；
2. **降级行为 = 三件事，且只有三件事**：
   * 该插件**不注入任何** Header/Body/变量变更（请求按 `.http` 文件原样外发）；
   * 终端黄色 `[WARN]` + 报告与快照 `warnings` 各记录一条：`插件 '<alias>' <缺失|失败:<kind>>，已降级跳过（现场数据已记录至快照）`；
   * 快照单条结果标记 `ExecutionOutcome::DegradedPass`（§10.5 三态判定），套件结束输出降级计数汇总。
3. **禁止事项（对抗性审查裁决）**：
   * **禁止"静默"降级**——本项目反静默哲学（`doc/dev/development_standards.md`）适用于降级路径；
   * **禁止引擎注入"预留 Token / 预置兜底 Header"等魔法值**——兜底鉴权只有一种合法机制：**用户在 `.http` 中手写的静态 Header**（如 `Authorization: Bearer {{DEBUG_FALLBACK_TOKEN}}`）；
   * **含未解析 `{{var}}` 的 Header 值不得原样外发**——变量解析失败的兜底 Header 视为"兜底缺失"，该请求**跳过**并 WARNING（防止把字面量 `{{DEBUG_FALLBACK_TOKEN}}` 当 Bearer Token 发往生产）。
4. **残余风险声明（05 ADR-D5）**：降级后的未签名请求发往生产 API，可能触发匿名主体行为差异（401/200 差分）。接受该风险的理由：降级在两个显式层级（用例 `?optional` 手写声明 + CLI `--allow-missing-plugins` 显式旗标）均为用户主动选择，且"发包看响应"正是现场排障的核心价值；备选方案"降级默认跳过请求 + `--degraded-mode=send` 放行"因违背任务使命第 4 条（绝不因缺失插件导致测试阻塞）被否决。CI 侧由 `--strict-plugins` 硬门禁兜底（§10.5）。

### 2.2 用例级声明降级：`@auth <NAME> [params] ?optional` `[待建 M1]`

```http
### 获取受保护的订单详情
POST https://api.my-cloud.com/v1/orders
# 开发环境自动调用插件签名；调试环境若无插件，自动跳过插件并不阻断测试
@auth aws-sigv4 region=us-east-1 ?optional

# 配合静态兜底 Header（现场调试时直接使用临时的运维 Token）
Authorization: Bearer {{DEBUG_FALLBACK_TOKEN}}
Content-Type: application/json

{"order_id": 123}
```

> 语法唯一性裁决：`?optional` 后缀是**唯一**声明语法；`optional=true` 与 `@use ... optional: true` 两种拼写废除。

### 2.3 CLI 级全局降级开关：`--allow-missing-plugins` `[待建 M3]`

```bash
rupost test production_suite.http --allow-missing-plugins --save-snapshot /tmp/live-dump.json
```
* 行为：所有未命中/失败的插件调用按 §2.1 统一语义降级；单条结果标 `DegradedPass`；默认（未传旗标）时未声明 `?optional` 的插件缺失按 §2 流程图右支**报错退出（exit 1）**并列出搜索路径。

---

## 3. DeepSeek Harness 统一注册表架构 (PluginRegistry) `[待建 M2]`

借鉴 **DeepSeek Harness**（以及先进基准评测框架）的 Registry 模式，`rupost` 将全系统的插件资产统一收敛至 `PluginRegistry`，作为全系统插件发现、元数据自省、CLI 路由与生命周期的**单一事实来源 (SSOT)**。

```
+----------------------------------------------------------------------------------------------------+
|                                      统一插件注册表 (PluginRegistry)                                |
+----------------------------------------------------------------------------------------------------+
|      插件根: $PATH 各目录  +  ~/.rupost/plugins  +  ./.rupost/plugins (闸门控制)                    |
|      发现单元: 每个插件 = 二进制 + 同目录静态 manifest (rupost-plugin-<name>.json)                  |
|                                                                                                    |
|   ┌────────────────────────────────────────────────────────────────────────────────────────────┐   |
|   │ 1. CLI Commands 注册表      : 路由 rupost <subcommand> -> 绑定执行入口与 TTY 规则          │   |
|   │ 2. Lifecycle Hooks 注册表   : 复合键 (name, Stage) 路由 @auth/@transform/@sign -> Schema   │   |
|   │ 3. Offline Miners 注册表    : [P2] 路由 snapshot.json 离线二次挖掘（MVP 走 02 §3 直挂载）   │   |
|   └────────────────────────────────────────────────────────────────────────────────────────────┘   |
+----------------------------------------------------------------------------------------------------+
```

### 3.0 发现机制裁决：静态 Manifest 文件（安全根基，v2.1.0 修订）

**发现 = readdir + JSON 解析，零子进程。**

1. 插件以**静态 manifest 文件**分发：`rupost-plugin-<name>.json`（与插件二进制同目录）；
2. 扫描插件根（`$PATH` 各目录、`~/.rupost/plugins`、放行后的 `./.rupost/plugins`）时**只读取 manifest 文件，绝不执行任何二进制**；
3. **旧协议废除**：v2.0.0 的"插件在安装或探测时响应 `--manifest` 命令"废除——该协议使"扫描"变成"执行陌生二进制"（npm/pip postinstall 向 `$PATH` 丢一个 `rupost-plugin-*` 文件，受害者运行任何触发发现的命令即获代码执行）。`--manifest` 自省仅保留给显式高级动作 `plugin sync --regenerate`（用户 opt-in，`[P2]`）；
4. **新鲜度闭环**：运行时对插件根做目录级 stat（readdir + mtime/size，无子进程，维持 ≤5ms 预算）；manifest 文件集合或指纹变化 → 对应缓存条目失效并从静态 manifest 重建。**新装插件在 sync 前即可被路由发现**（消除"假降级"窗口）；
5. **信任分级修正**：`$PATH` 属用户级可写位置（包管理器 postinstall 天然写入），不再无条件"全局受信任"——加载前校验 manifest 属主与目录权限（§3.2），并在文档明示该信任假设。

### 3.1 插件元数据清单契约 (Static Plugin Manifest)

```json
{
  "manifest_version": "1.0",
  "name": "aws-sigv4",
  "version": "0.3.1",
  "description": "AWS Signature Version 4 动态认证与加签插件",
  "commands": [
    {
      "name": "sigv4",
      "usage": "rupost sigv4 --inspect [options]",
      "description": "检查并校验本地 AWS 凭据与 STS 状态",
      "requires_tty": false
    }
  ],
  "hooks": [
    {
      "directive": "auth",
      "alias": "aws-sigv4",
      "stage": "prepare",
      "cache_scope": "suite",
      "requires_env": ["AWS_PROFILE"],
      "parameters_schema": {
        "type": "object",
        "properties": {
          "region": { "type": "string", "description": "AWS 区域，如 us-east-1" },
          "service": { "type": "string", "description": "AWS 服务名，如 s3, execute-api" },
          "profile": { "type": "string", "description": "本地 AWS profile 名称" }
        },
        "required": ["region"]
      }
    }
  ],
  "miners": []
}
```

字段裁决（v2.1.0 新增/修订）：
* **`stage` 合法枚举**：MVP 仅 `prepare | transform | sign`；`post-response | on-failure | harvest` 为保留字（`[P2]`，§5.1 阶段 5~7 未放行注册路径）；
* **`cache_scope`**：`none | request | suite`（`ttl` 为 `[P2]`）。OAuth2 取 Token 类插件声明 `suite`，内核跨请求复用其 Patch 结果（消除"100 请求拉起 100 次取 Token 子进程"的进程风暴，量化验收见 06 §5）；
* **`requires_env`**：环境白名单声明（§6.2）；
* **`requires_tty`**：仅 commands 可为 true；**hooks 一律禁止声明 TTY**（hook 走 §8 管道）；
* v2.0.0 的 `default_priority` 字段随 priority 魔数废除而删除（§5.2）；
* **alias 冲突规则**：项目级覆盖全局，冲突启动 WARNING；插件命令与内置子命令/HTTP 方法重名：内置 > 插件 > 方法名，安装期告警（§6.1）。

### 3.2 发现、复合键索引与极速轻量缓存机制 (<5ms 启动守护)

1. **复合键无冲突索引 (`(plugin_name, PluginStage)`)**：
   * 单个插件（如 `aws-sigv4`）可同时提供 `@auth`（`Prepare` 阶段拉取 STS 临时凭据）与 `@sign`（`Sign` 阶段计算请求体签名）多个钩子；
   * `PluginRegistry` 内部强制使用 `(String, PluginStage)` 复合键（或 `Vec<HookCapability>`）存储钩子，严禁仅以插件名作为单一 Key 导致后注册的 Hook 静默覆盖先注册的 Hook。
2. **聚合缓存**：`~/.rupost/registry.json` 是静态 manifest 的**可重建索引**（非权威数据源），目标读取耗时 ≤5ms（bench 落 M2，口径不含首轮目录扫描）；
3. **零冷启动延迟**：日常运行 `rupost --help`、`rupost test` 或补全时，微内核**仅读取本地 JSON 缓存**，绝不在日常运行中启动插件子进程；
4. **缓存安全硬化（v2.1.0 新增）**：
   * `~/.rupost/` 目录强制 `0700`、`registry.json` 强制 `0600`，加载前校验属主 uid == euid；
   * **路径白名单**：manifest 引用的可执行路径必须 canonicalize 后落在已知插件根内；拒绝 `..`、拒绝逃逸插件根的 symlink（防缓存投毒后任意路径执行）；
   * **项目级 `./.rupost/registry.json` 文件废除**（v2.1.0 裁决）——恶意仓库可直接携带该文件绕过插件目录闸门；项目级插件只认 `./.rupost/plugins/` 目录 + 静态 manifest，且必须携带 `--allow-project-plugins` 或首次交互确认后放行（防恶意仓库 RCE）；
5. **安全隔离策略**：工作区目录 `./.rupost/plugins` 默认处于沙箱警戒状态，必须携带 `--allow-project-plugins` 或在首次交互确认后才允许加载。

---

## 4. 插件友好语法与智能提示 (Directives & IntelliSense)

### 4.1 语义化领域语法糖 (Domain Directives)

用户直接根据业务意图书写领域注解，**100% 屏蔽 `stage=` 与 `priority=` 等底层调度魔数**：

| 注解语法 | 归属生命周期 Stage | MVP 状态 | 示例 |
| :--- | :--- | :--- | :--- |
| **`@auth <name> [params]`** | `Prepare` 阶段 (固定不可逆) | `[待建 M1]` | `@auth aws-sigv4 region=us-east-1 ?optional` |
| **`@transform <name> [params]`** | `Transform` 阶段 (固定不可逆) | `[待建 M1]` | `@transform aes-gcm key=env.AES_KEY` |
| **`@sign <name> [params]`** | `Sign` 阶段 (固定不可逆) | `[待建 M1]` | `@sign hmac-sha256 key=env.SECRET` |
| **`@pipeline a -> b -> c`** | 跨阶段链式编排 | **`[P2 预览，MVP 不实现]`** | `@pipeline oauth2 -> aes-gcm -> aws-sigv4` |

#### 为什么彻底废除 `@plugin <name> stage=... priority=...`？
- **内部实现泄露 (Leaky Abstraction)**：用户在写测试用例时，只关心"我要鉴权"或"我要签名"。强迫用户指定 `stage` 和 `priority` 属于把微内核内部调度术语甩给用户；
- **消除魔数与认知负担**：用户无需猜测 `priority=80` 与 `priority=99` 哪个先执行；
- **消灭解析器顺序依赖 Bug**：彻底避免了扁平解析参数时 `stage` 与 `priority` 互相覆盖的脆弱性。

#### 引号感知的参数分词器 (Quote-Aware Tokenizer)
当参数值包含空格（如 OAuth2 `scope="read:orders write:orders"`）时，解析器必须采用引号感知状态机（支持双引号 `"..."` 与单引号 `'...'` 及转义 `\"`），严禁使用裸 `split_whitespace()` 导致带空格的参数值被错误撕裂。

#### `@pipeline` 语义裁决（P2 实现时生效）
阶段顺序不可逆（§5.2 规则 1）：`@pipeline` 引用与阶段序冲突（如 `@pipeline sign -> auth`）视为**语法错误（exit 2）**，绝不静默重排。

#### 复杂多行参数块（`[P2]` Phase 2 演进规划，针对长参数场景）
```http
POST https://api.my-cloud.com/v1/orders
@use aws-sigv4:
  region: us-east-1
  service: execute-api
  profile: staging

Content-Type: application/json

{"order_id": 123}
```

### 4.2 智能提示与补全体系 (IntelliSense Architecture) `[P2，MVP 仅保留 schema 导出]`

1. **MVP 交付**：`rupost plugin schema` 导出全量 JSON Schema（06 M5）——VSCode / IntelliJ 加载该 Schema，用户键入 `@auth ` 时自动提示本地已安装的插件名，选中后 Snippet 展开必填参数并在 Hover 时展示参数释义。
2. **`[P2]` 登记**：TUI 编辑器（`ratatui-textarea`）与 Web 工作台键入 `@` 字符的原位补全；CLI 脚手架模板 `rupost template <name>`。
   > 现状冲突处理：`template` 当前是 `init` 命令的别名（`src/cli/commands/mod.rs:58`），与"插件模板生成"语义无关；P2 实现插件模板系统时剥离该别名（任务登记于 06）。

---

## 5. 多插件生命周期管线与执行顺序控制规约

### 5.1 顺序控制第一性原理：不可逆单向阶段流

为彻底杜绝"先加签后加密导致签名失效"等逻辑灾难，微内核将生命周期严格划分为**不可逆的阶段（Stages）**：

```
                              [ 单请求用例触发执行 ]
                                         │
    ┌────────────────────────────────────┴────────────────────────────────────┐
    ▼                                    ▼                                    ▼
[ 1. Prepare 阶段 (@auth) ]      [ 2. Transform 阶段 (@transform) ]   [ 3. Sign 阶段 (@sign) ]
  • OAuth/Token 动态获取           • 请求体/参数动态变形 (AES/压缩)      • 报文摘要与防篡改 (SigV4/HMAC)
  • 阶段内严格按书写行序 (FIFO)     • 阶段内严格按书写行序 (FIFO)         • 阶段内严格按书写行序 (FIFO)
    └────────────────────────────────────┬────────────────────────────────────┘
                                         ▼
                             [ 4. 物理网络发送 (Reqwest) ]
                                         ▼
    ┌────────────────────────────────────┴────────────────────────────────────┐
    ▼                                    ▼                                    ▼
[ 5. Post-Response 阶段 ]         [ 6. On-Failure 阶段 ]               [ 7. Harvest 阶段 ]
  [P2] 响应报文解密/反脱敏          [P2] 故障时触发 (现场指标增强)        [P2] 离线黑匣子聚合挖掘
```

> 阶段 5~7 在 manifest `stage` 枚举中为保留字，MVP 不提供注册路径（06 §3）。

### 5.2 仲裁规则 (Arbitration Rules)

1. **阶段不可逆原则 (Stage Invariance)**：
   无论在 `.http` 文件中的书写顺序如何，`Prepare` 阶段插件必然先于 `Transform` 执行，`Transform` 必然先于 `Sign` 执行；
2. **同阶段所见即所发 (FIFO Order & No Magic Numbers)**：
   在同一个 Stage 内，100% 严格依照用户在 `.http` 文件中**从上到下的视觉书写行序（Stable Order）**推进，**彻底消灭 `priority=99` 等底层魔数**（v2.1.0 起为唯一权威规则，不存在 directive 默认 priority 与跨阶段 priority）；
3. **显式管道链式表达 (`@pipeline`)** `[P2]`：
   若需显式声明数据流动关系，直接使用 `@pipeline auth -> transform -> sign`，直观反映输入输出流向；与阶段序冲突时为语法错误（§4.1）。

### 5.3 纯函数式不可变补丁管道契约 (Immutable Patching Pipeline)

内核在内存中维持请求对象的不可变性，每个插件通过标准 Stdio IPC 接收当前流转的 Context，并输出增量 `PluginPatch`：

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginPatch {
    /// 增量追加或大小写不敏感覆盖 (Upsert) 的请求头
    #[serde(default)]
    pub extra_headers: Vec<(String, String)>,
    /// 需要显式移除的请求头名称列表 (大小写不敏感匹配)
    #[serde(default)]
    pub remove_headers: Vec<String>,
    /// 经变形/加密后的请求体 (若为 None 则保持原样)
    #[serde(default)]
    pub mutated_body: Option<String>,
    /// 动态回传给测试上下文的局部变量
    #[serde(default)]
    pub set_variables: HashMap<String, String>,
    /// 非致命警告信息 (录入报告与快照；渲染前净化，§6.4)
    #[serde(default)]
    pub warnings: Vec<String>,
}
```

* **Header 合并语义 (Case-Insensitive Upsert)**：当后序插件（或插件覆盖请求预留兜底 Header，如 `Authorization`）输出同名 Header 时，`PluginPatch` 按 HTTP 规范执行**大小写不敏感覆盖更新（Upsert）**，并先执行 `remove_headers` 清理，避免产生重复冲突的 `Authorization` 头。
* **链式折叠**：前序插件产生的 Patch 经微内核校验后合入临时上下文中，后续插件能够读取到前序插件产生的变更结果（`set_variables` 经内核在 IPC 输入 `variables` 中下发，§8.1），形成无数据竞争的纯净函数式流水线。
* **`env.*` 解析裁决**：`key=env.AES_KEY` 中的 `env.*` 由**内核**解析（变量解析 SSOT），解析后的明文值仅经 Stdio IPC 传给插件进程；`plugin_args` 与解析值**永不落盘快照/报告**（02 §2.7）。

---

## 6. CLI 自定义命令扩展规范 (Custom Commands SPI) `[待建 M4]`

### 6.1 命名契约与路由裁决序（唯一权威）

* **命名契约**：插件二进制统一遵循 `rupost-plugin-<name>`（推荐）或兼容 `rupost-<name>`；静态 manifest 同名 `.json`；
* **路由前提修正（v2.1.0）**：根命令 `args` 配置 `trailing_var_arg = true, allow_hyphen_values = true`（`src/cli/mod.rs:29`，受 AGENTS.md §5 位置契约锁定）——`rupost flamegraph x.json` 会被整体吸入 `cli.args`，**clap 不会产生"未知子命令"事件**。因此路由不依赖 clap 未知命令分支，而是**主动拦截 `args[0]` 查表**：
  1. `args[0]` ∈ 内置子命令集合 → 内置派发（最高优先级）；
  2. ∈ 注册表命令集合（精确匹配）→ 插件子进程拉起；
  3. ∈ HTTP 方法集合（大小写不敏感）→ 内联请求模式；
  4. 否则 → 报错退出（exit 2）；
* 内联请求模式的位置契约不变（全局参数必须紧跟主程序名之后，AGENTS.md §5）；
* 插件命令与内置命令/HTTP 方法重名时按 §3.1 冲突规则处理（内置/方法名赢 + 安装期告警）。

### 6.2 TTY 直通、环境白名单与退出码穿透

* **TTY 独占直通**（仅 commands）：直接继承终端的 `stdin`、`stdout`、`stderr`，使外部插件能够原生运行交互式 TUI、密码输入或进度动画；**每次拉起前打印一行来源横幅**（`[plugin] <路径> sha256:<前16位>`），**首次执行的"命令+路径+哈希"组合要求 TOFU 确认**（缓存于 registry）——防注册表投毒后的 UI 伪造钓鱼；
* **环境变量白名单（v2.1.0 裁决）**：子进程默认仅继承 `PATH/HOME/LANG/TERM/TMPDIR + RUPOST_*`；manifest `requires_env` 声明额外需要的变量，运行时可用 `--plugin-env <VAR>` 显式放行。**全量环境继承废除**（防止堡垒机上的 AWS 凭据/API Key 经第三方插件外泄）；
* **注入的上下文环境变量**：
  * `RUPOST_WORKSPACE`：当前工作区绝对路径；
  * `RUPOST_ENV`：当前激活的环境标签（如 `--env staging`）；
  * `RUPOST_CONFIG_DIR`：配置与插件注册表目录路径；
* **退出码穿透（信号感知）**：`Some(code)` → 原值穿透（不受 01 §2.1 三值契约约束，该契约仅约束内置命令）；`None`（信号终止）→ `128 + signal`（Unix 惯例），废除 `status.code().unwrap_or(1)` 折叠。

### 6.3 双模路由流程

1. 用户在终端执行 `rupost flamegraph snapshot.json --theme dark`；
2. 派发器按 §6.1 裁决序拦截 `args[0]`，命中注册表命令 `flamegraph`；
3. Tokio 异步子进程拉起，TTY 直通 + 来源横幅 + 环境白名单；
4. 退出码按 §6.2 穿透，完全遵守 CI/CD 流水线退出码判定契约；
5. `rupost --help` 末尾优雅展示 `EXTENSIONS` 区块及其单行说明。

### 6.4 manifest 文本净化（终端注入防线，v2.1.0 新增）

manifest 的 `name/description/usage` 与插件 `warnings` 渲染进终端前必须经**白名单净化**（仅可打印字符 + 长度上限，description ≤80 字符），与报告侧 `strip-ansi-escapes` 统一为同一个净化 SSOT——防止投毒 manifest 经 `--help`/`plugin list` 注入 ANSI 控制序列（清屏/OSC 伪造链接/剪贴板覆写/仿冒密码提示）。

---

## 7. AI 集成裁决：AI-as-Plugin（v2.1.0 修订，取代"四大内核 SPI"）

严格遵循 `doc/plans/archive/2026-q4/2026-10-27-ai-master-roadmap.md` 宪法原则（B0 运行期零 AI、B1/B2 兜底分级、五阶段管线模板、"AI 建议，引擎裁决"）。

### 7.1 裁决内容

**内核不定义 `AiContextProvider`/`AiSnapshotMiner`/`EngineArbiter`/`McpToolBridge` 四个 trait**（v2.0.0 的"预留接口"属无消费者的死代码，违反 Simplicity First）。AI 扩展机制 = 插件方案 D 本身：

1. **外部 AI 插件 = 普通 miner 插件**：`rupost-plugin-ai-<name>` 消费 `--input snapshot.json`（02 §3.1 协议），经注册表分发，内核无需感知"这是 AI 插件"；
2. **内核只承诺三个确定产物**：
   * **快照契约**（02 §2，AI 素材 SSOT）；
   * **脱敏 SSOT**（`src/ai/redactor.rs` `[已有]`——任何数据进 AI 管道前强制过此路径，v2.0.0 §7.1 的"强制脱敏"承诺由此兑现，无需 trait）；
   * **首个内置 miner 消费者**：`ai triage --snapshot` `[待建 M5]`，复用 `crates/rupost-ai` 的 `FailureAttributor` 适配 `SnapshotEntry`（05 ADR-D9）；
3. **EngineArbiter / McpToolBridge**：停泊中期登记（MCP Server 待 ai 命令族稳定后按 master roadmap 排期；"AI 建议语法产物必须过引擎 AST 校验"原则届时随实现落地，不预置抽象）。

### 7.2 AI 红线（AI 插件准入检查，缺一不立项）

1. **脱敏先于 AI**：进入 AI 管道的任何上下文必须先过 redactor；
2. **plugin_args 永不入快照**（02 §2.7）；
3. **masking 感知**：AI miner 必须读取 `masking` 标记，掩码产物上的结论 MUST 附带数据不完整警告；
4. **不可信输入校验**（02 §3.3）；
5. 按 master roadmap 治理规则声明 B1/B2 级别 + 降级用例（无 Key / AI 故障 / 输出非法）强制入测试方案；AI 插件不可用（断网、无 Key、超时）时降级为基于状态码和网络时序指标的**确定性机械归因**（`src/ai/rules.rs` `[已有]`），绝不崩溃挂起。

---

## 8. 通信底层守护 (Stdio IPC Protocol & Guard)（唯一权威）

外部插件与微内核通信遵循无死锁、强类型、单 JSON 对象的管道规范。

### 8.1 运行时 Hook 协议

* **输入 (`stdin`)**:
  ```json
  {
    "protocol_version": "1.0",
    "stage": "sign",
    "plugin": "aws-sigv4",
    "request": {
      "method": "POST",
      "url": "https://api.my-cloud.com/v1/orders",
      "headers": [["Content-Type", "application/json"]],
      "body": "{\"order_id\": 123}"
    },
    "variables": { "order_id": "123" },
    "params": {"region": "us-east-1"}
  }
  ```
* **输出 (`stdout`, 单个 JSON 对象，直接对应 `PluginPatch` 契约)**:
  ```json
  {
    "protocol_version": "1.0",
    "extra_headers": [["Authorization", "AWS4-HMAC-SHA256 ..."]],
    "remove_headers": [],
    "mutated_body": null,
    "set_variables": {},
    "warnings": []
  }
  ```
* **失败信封**：插件自身可检错误输出 `{"protocol_version": "1.0", "ok": false, "error_kind": "invalid_output|plugin_error", "message": "..."}`；进程级失败（超时/崩溃/exit≠0）由内核判定（§8.3）；
* **版本协商**：输出必须回显 `protocol_version`；不兼容版本按 `invalid_output` 处理并 WARNING。

### 8.2 管道纪律

1. **stdout 单对象纪律**：stdout 仅允许输出一个 JSON 对象；协议级校验，任何多余输出判为 `invalid_output`（插件日志必须走 stderr）；
2. **stderr 内核托管**：hook 调用时 stderr 一律 piped，由内核异步排空（**stderr 洪泛同样会触发管道缓冲死锁**，§10.1 的独立 Task 排水覆盖 stdout 与 stderr）；内容截断至 16KB 后并入 `warnings`；CI 模式下 stderr 不直通终端；
3. **`plugin_args` 安全**：`plugin_args`（含内核解析的 `env.*` 值）永不序列化进快照/报告（02 §2.7）；`warnings`、`set_variables` 入库前过 redactor + 文本净化（§6.4）。

### 8.3 失败分类学（默认行为唯一权威，v2.1.0 新增）

| kind | 判定 | 默认行为 |
| :--- | :--- | :--- |
| `timeout` | 超过超时窗 | kill 整个进程组（§10.2），按 §2.1 降级语义处理 |
| `invalid_output` | stdout 非单 JSON / 版本不兼容 / 失败信封 | 不注入 Patch，按降级语义处理 |
| `crashed` | 子进程信号终止 | 同上，错误信息含信号编号 |
| `exit_non_zero` | exit ≠ 0 且无合法输出 | 同上，错误信息含 stderr 摘要 |

* **降级联动**：四类失败与"缺失"共享 `?optional` / `--allow-missing-plugins` 降级开关（§2.1）；
* **超时可配置**：默认 5s 硬熔断，可用 `--plugin-timeout <sec>` 覆盖（SigV4 STS 冷启动等慢插件场景）；
* **时延隔离**：插件子进程耗时独立统计为 `plugin_duration_us`，落盘于快照 `ExecutionSnapshotEntry.plugin_duration_us`（02 §2），**绝不混入**物理网络的 `duration_us` 和 `ttfb`；
* **Hook 结果缓存**：按 manifest `cache_scope` 复用 Patch（suite 级缓存使 OAuth2 类插件全套件仅 1~2 次子进程，量化验收见 06 §5）。

---

## 9. 借鉴 DeepSeek Harness (dsh) 微内核架构实践

DeepSeek Harness (`dsh`) 是由 DeepSeek 官方构建的智能体运行时框架，其架构底层基于 **Cordis 微内核元框架**。`rupost` 从中汲取了五大核心工程设计：

```
 ┌─────────────────────────────────────────────────────────┐
 │               1. 表现扩展层 (Extension Layer)           │
 │      (CLI 命令行 / TUI 终端 / Web UI / Desktop / MCP)   │
 ├─────────────────────────────────────────────────────────┤
 │               2. 运行时控制层 (Framework Control)       │
 │   • Cordis 微内核 IoC 容器      • 统一工具注册表 (Registry)│
 │   • 可插拔执行循环 (Loop)       • Append-Only 事件溯源日志  │
 ├─────────────────────────────────────────────────────────┤
 │               3. 基础适配层 (Foundation Layer)          │
 │      (Model Adapters / Stdio 进程管道 / OS 沙箱驱动)    │
 └─────────────────────────────────────────────────────────┘
```

1. **统一注册表作为单一事实来源 (Registry SSOT)**：
   * 将 CLI 扩展命令、生命周期钩子与离线挖掘器统一归口管理，平常只读静态 Manifest（$\le 5\text{ms}$），运行时按需懒执行；
2. **"执行现场与深度分析解耦"的事件溯源思想**：
   * 现场只负责高保真记录网络微秒级时序与原始数据黑匣子，复杂挖掘全部留待事后离线消费；
3. **上下文作用域安全注入 (Scoped Context)**：
   * 杜绝隐式全局变量，插件只能通过微内核安全上下文（经过 `redactor.rs` 脱敏清洗 + 环境白名单，§6.2）获取信息；
4. **严格策略门禁拦截 (Policy Enforcement Gate)**：
   * 在发生文件写入或子进程调用前设置安全策略门禁，本地 `./.rupost/plugins` 默认隔离防 RCE；
5. **纯函数式单向流与不可变补丁 (Immutable Patch Pipeline)**：
   * 插件只返回不可变的变更 Delta/Patch，消灭跨线程、跨进程的数据竞争。

---

## 10. 架构审查高危风险防范与硬核工程实现规约 (Review Defense Specs)

结合架构审查组针对系统死锁、孤儿进程、并发争用与平台差异提出的风险，确立以下**强制性工程落地防御规约**：

### 10.1 管道死锁防御与载荷硬上限 (10MB Cap & Async Drain)
* **载荷上限硬约束**：Stdio IPC 管道单次传递的 Request/Response 载荷上限强制设定为 **10MB**。超限时拒绝管道传输并降级跳过，杜绝内存和管道缓冲区打满挂起；
* **异步双工独立 Task 排水**：
  在 `StdioDriver` 中，必须将 `stdin` 写入与 `stdout/stderr` 异步消费拆分为独立的并发 `tokio::spawn` 任务；使用 `tokio::io::AsyncReadExt::take(10 * 1024 * 1024)` 限制读取量；在写入 `stdin` 完毕后，必须显式执行 `stdin.shutdown().await` 并立即 drop 句柄。**stderr 同样必须被排水**（洪泛的 stderr 与 stdout 一样会触发 64KB 管道缓冲死锁）。

### 10.2 进程树完全清剿机制 (Process Group Termination)
* **Unix 平台 (PGID 树状清剿)**：
  在启动插件子进程时调用 `std::os::unix::process::CommandExt::process_group(0)`，将子进程设置为新的进程组 Leader；超时熔断时向 `-pgid` 发送 `SIGKILL`，确保连同孙子进程彻底杀绝，防止 PID/FD 耗尽与僵尸进程；fail-fast 熔断时对在跑插件进程执行同样的组级 kill 传播；
* **Windows 平台 (Job Object)**：
  使用 Windows `CreateJobObjectW` 并配置 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`，绑定子进程生命周期，父进程退出时系统自动强杀所有派生进程；
* **`[P2]` 登记**：Unix `pre_exec` RLIMIT（CPU/AS/NPROC/FSIZE）资源配额。

### 10.3 注册表并发写保护与防幽灵缓存 (Atomic Rename & Hash Fingerprint)
* **多进程与多线程原子重命名覆盖**：刷新 `~/.rupost/registry.json` 时，必须先写入同目录带 `PID + 原子递增序号 (AtomicU64)` 的临时文件（`registry.json.tmp.<pid>.<seq>`），再通过原子重命名（`fs::rename`）覆盖目标文件，彻底杜绝多进程及同进程多线程并行测试时的并发踩踏；
* **极速指纹增强**：缓存校验条件由单纯比对 `mtime` 升级为：
  $$\text{Cache Valid} \iff \text{mtime matches} \land \text{Hash}(\text{First 4KB}) \text{ matches}$$
  彻底避免 1 秒内文件修改造成的"幽灵虚假命中"；`[P2]` 增强：对超过 4KB 的 manifest 或安全敏感场景改用全量 sha256 指纹；
* **配合 §3.2 路径白名单与属主校验**共同构成缓存投毒防线。

### 10.4 Windows 平台脚本透明代理 (Shebang Runner Adapter)
在 Windows 环境下探测插件时，若发现目标是 `.py`、`.js`、`.sh` 脚本，微内核自动将其封装为对应宿主环境解释器命令（如 `python.exe <script>`），消灭无扩展名脚本在 Windows 上的 `OS error 193` 执行崩溃。

### 10.5 三态测试判定模型 (Three-State Outcome & Strict Gate)
* **状态明确拆分**：快照与测试报告中引入 `ExecutionOutcome::DegradedPass`（02 §2），严禁将自愈降级混淆为纯粹的成功；
* **CI 门禁模式分流**：
  * CLI 引入 `--strict-plugins` 标志（发布环境 CI 推荐默认开启）：一旦发生任何插件降级跳过，无论 HTTP 返回码为何，整体退出码强制为 `1`；
  * 只有在现场调试排障且显式指定 `--allow-missing-plugins` 时，才允许打印高亮 `[WARN: PLUGIN DEGRADED]` 并返回 `0`。

### 10.6 快照防 OOM 截断限额 (Snapshot Lightweight Limit)
* 默认单响应体录入上限设为 `256KB`（可通过 `--max-snapshot-body-bytes` 调整）。超限响应体执行截断，并标记 `body_truncated: true`，同时记录完整报文的 `body_sha256` 校验和，确保快照文件轻盈（控制在 5MB 以内）。
