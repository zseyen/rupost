# 03 - 插件 SPI 体系、统一注册表与生命周期管线规范

- **状态**: Approved (Updated with DeepSeek Harness Architecture & Review Defense Specs)
- **归属任务**: `cross-scenario-engine-and-plugins`
- **核心原则**: **“开发期全能扩展，调试期优雅降级，运行期硬性零 AI，架构级单一事实来源”**

---

## 1. 异构环境冲突场景与降级核心矛盾

在真实研发旅程中经常发生以下断裂：
* 开发者在本地 Mac 上写了测试用例，使用了外部插件（如 `@auth aws-sigv4`，依赖本地 Python 和 boto3）；
* 生产报警后，运维或开发把这套 `.http` 用例直接扔上了一台极简的 Linux 生产跳板机，使用 `rupost` 单二进制快速排查；
* **致命矛盾**：如果因为生产机器没有 Python 或没装 `rupost-plugin-aws-sigv4`，`rupost` 就直接 Panic 报错退出，会导致**整个现场排障直接瘫痪**！

---

## 2. 声明式降级策略与自愈机制

为了让测试用例具备跨环境的自愈能力，`rupost` 提供**两层降级防线**：

```
                              [ 执行带有插件调用的测试用例 ]
                                             │
                                             ▼
                             [ PluginRegistry 本地/PATH 探测 ]
                                             │
                      ┌──────────────────────┴──────────────────────┐
                      ▼                                             ▼
                【探测成功】                                   【插件未找到】
                      │                                             │
                      ▼                                             ▼
             [ 执行外部 Stdio IPC 管道 ]              ┌───────────────────────────┐
             计算动态签名，注入 Header                │ 检查降级标志:              │
                                                      │ 1. 用例显式 ?optional     │
                                                      │ 2. CLI --allow-missing ?  │
                                                      └─────────────┬─────────────┘
                                                                    │
                                     ┌──────────────────────────────┴──────────────────────────────┐
                                     ▼                                                             ▼
                               【允许降级】                                                  【未声明降级 (默认)】
                                     │                                                             │
                                     ▼                                                             ▼
             [ 记录 Warning，使用预置兜底 Header 发包 ]                                   [ 友好报错退出，列出插件搜寻路径 ]
             基础功能照常发包，录制完整原始现场至 dump.json
```

### 2.1 用例级声明降级：`@auth <NAME> [params] ?optional`
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

### 2.2 CLI 级全局降级开关：`--allow-missing-plugins`
在现场排障或即拷即走场景中，用户无需修改已有的 `.http` 文件，直接传参：
```bash
rupost test production_suite.http --allow-missing-plugins --save-snapshot /tmp/live-dump.json
```
* **行为**：
  * 所有找不到对应二进制的插件调用均被静默降级为“跳过”；
  * 向最终的测试报告与快照产物中追加明确警告：
    `[Warning] 插件 'aws-sigv4' 未安装，已降级跳过（现场数据已记录至快照）`；
  * 基础请求正常外发，完整的网络时序与响应内容被完整录入快照，保证现场排查任务能够顺利完成。

---

## 3. DeepSeek Harness 统一注册表架构 (PluginRegistry)

借鉴 **DeepSeek Harness**（以及先进基准评测框架）的 Registry 模式，`rupost` 将全系统的插件资产统一收敛至 `PluginRegistry`，作为全系统插件发现、元数据自省、CLI 路由与生命周期的**单一事实来源 (SSOT)**。

```
+----------------------------------------------------------------------------------------------------+
|                                      统一插件注册表 (PluginRegistry)                                |
+----------------------------------------------------------------------------------------------------+
|                                                                                                    |
|    ~/.rupost/registry.json (全局缓存)  +  ./.rupost/registry.json (项目级)  +  $PATH 探测          |
|                                                                                                    |
|   ┌────────────────────────────────────────────────────────────────────────────────────────────┐   |
|   │ 1. CLI Commands 注册表      : 路由 rupost <subcommand> -> 绑定执行入口与 TTY 规则          │   |
|   │ 2. Lifecycle Hooks 注册表   : 复合键 (name, Stage) 路由 @auth/@transform/@sign -> Schema   │   |
|   │ 3. Offline Miners 注册表    : 路由 snapshot.json 离线二次挖掘 -> 绑定分析驱动器            │   |
|   └────────────────────────────────────────────────────────────────────────────────────────────┘   |
+----------------------------------------------------------------------------------------------------+
                                      │
               ┌──────────────────────┴──────────────────────┐
               ▼                                             ▼
       [ CLI 顶层分派器 ]                             [ HTTP 运行引擎 ]
  (rupost flamegraph snapshot.json)                 (@auth aws-sigv4 ...)
```

### 3.1 插件元数据清单契约 (Plugin Manifest)

任何语言（Python、Node、Go、Shell、Rust）编写的插件均在安装或探测时响应 `--manifest` 命令，输出标准化 JSON：

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
      "requires_tty": true
    }
  ],
  "hooks": [
    {
      "directive": "auth",
      "alias": "aws-sigv4",
      "stage": "prepare",
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

### 3.2 发现、复合键索引与极速轻量缓存机制 (<5ms 启动守护)
1. **复合键无冲突索引 (`(plugin_name, PluginStage)`)**：
   * 单个插件（如 `aws-sigv4`）可同时提供 `@auth`（`Prepare` 阶段拉取 STS 临时凭据）与 `@sign`（`Sign` 阶段计算请求体签名）多个钩子；
   * `PluginRegistry` 内部强制使用 `(String, PluginStage)` 复合键（或 `Vec<HookCapability>`）存储钩子，严禁仅以插件名作为单一 Key 导致后注册的 Hook 静默覆盖先注册的 Hook。
2. **轻量清单缓存**：微内核在扫描到插件后，将 Manifest 缓存于 `~/.rupost/registry.json`（带有文件指纹校验）；
3. **零冷启动延迟**：日常运行 `rupost --help`、`rupost test` 或补全时，微内核**仅读取本地 JSON 缓存**，耗时 $\le 5\text{ms}$，绝不在日常运行中轮询启动 Python/Node 插件子进程；
4. **安全隔离策略**：
   * `$PATH` 与 `~/.rupost/plugins` 目录下的插件视为全局受信任；
   * 当前工作区目录 `./.rupost/plugins` 默认处于沙箱警戒状态，必须携带 `--allow-project-plugins` 或在首次交互确认后才允许加载，彻底杜绝恶意仓库的 RCE（远程代码执行）攻击。

---

## 4. 插件友好语法与智能提示 (Directives & IntelliSense)

为了消灭生硬的底层配置感，提升用例编写的流畅度与美感，`rupost` 提供语义化语法糖与 IDE 智能提示机制。

### 4.1 语义化领域语法糖 (Domain Directives)

用户直接根据业务意图书写领域注解，**100% 屏蔽 `stage=` 与 `priority=` 等底层调度魔数**：

| 注解语法 | 归属生命周期 Stage | 典型业务场景 | 示例 |
| :--- | :--- | :--- | :--- |
| **`@auth <name> [params]`** | `Prepare` 阶段 (固定不可逆) | OAuth2 获取 Token、动态鉴权、STS 会话拉取 | `@auth aws-sigv4 region=us-east-1 ?optional` |
| **`@transform <name> [params]`** | `Transform` 阶段 (固定不可逆) | 请求体 AES 加密、动态数据格式转换、Gzip 压缩 | `@transform aes-gcm key=env.AES_KEY` |
| **`@sign <name> [params]`** | `Sign` 阶段 (固定不可逆) | 报文哈希计算、防篡改签名、支付开放网关加签 | `@sign hmac-sha256 key=env.SECRET` |
| **`@pipeline a -> b -> c`** | 跨阶段链式编排 | 严格保证多插件时序依赖流转 | `@pipeline oauth2 -> aes-gcm -> aws-sigv4` |

#### 为什么彻底废除 `@plugin <name> stage=... priority=...`？
- **内部实现泄露 (Leaky Abstraction)**：用户在写测试用例时，只关心“我要鉴权”或“我要签名”。强迫用户指定 `stage` 和 `priority` 属于把微内核内部调度术语甩给用户；
- **消除魔数与认知负担**：用户无需猜测 `priority=80` 与 `priority=99` 哪个先执行；
- **消灭解析器顺序依赖 Bug**：彻底避免了扁平解析参数时 `stage` 与 `priority` 互相覆盖的脆弱性。

#### 引号感知的参数分词器 (Quote-Aware Tokenizer)
当参数值包含空格（如 OAuth2 `scope="read:orders write:orders"`）时，解析器必须采用引号感知状态机（支持双引号 `"..."` 与单引号 `'...'` 及转义 `\"`），严禁使用裸 `split_whitespace()` 导致带空格的参数值被错误撕裂。

#### 复杂多行参数块（Phase 2 演进规划，针对长参数场景）
当插件参数过多时，支持直观的缩进块语法：
```http
POST https://api.my-cloud.com/v1/orders
@use aws-sigv4:
  region: us-east-1
  service: execute-api
  profile: staging
  optional: true

Content-Type: application/json

{"order_id": 123}
```

### 4.2 智能提示与补全体系 (IntelliSense Architecture)

```
                    ┌────────────────────────────┐
                    │  统一插件注册表 (Registry) │
                    └─────────────┬──────────────┘
                                  │ 导出 Schema / 补全定义
         ┌────────────────────────┼────────────────────────┐
         ▼                        ▼                        ▼
【1. VSCode / IDE 扩展】    【2. TUI / Web 工作台】    【3. CLI 交互模板】
 • JSON Schema 契约关联    • 原位 Tab / @ 补全弹窗    • rupost template
 • 字段 Hover 说明与校验   • 实时渲染参数占位符       • 终端代码片段即插即用
```

1. **JSON Schema 导出联动 (`rupost plugin schema`)**：
   * 注册表聚合所有已安装插件的 `parameters_schema`，输出标准 Schema；
   * VSCode / IntelliJ 插件加载该 Schema，用户键入 `@auth ` 时，自动提示本地已安装的插件名；
   * 选中插件后，自动触发 Snippet 展开必填参数（如 `region=${1:us-east-1}`），并在悬浮（Hover）时展示参数释义。
2. **TUI 与 Web 工作台的原位补全**：
   * 在 TUI 编辑器（`ratatui-textarea`）和 Web 编辑器中，当用户键入 `@` 字符时，弹窗自动显示候选注解与插件清单。
3. **CLI 脚手架模板 (`rupost template <name>`)**：
   * 运行 `rupost template aws-sigv4`，终端原位输出最佳实践代码片段，支持一键追加至当前 `.http` 文件。

---

## 5. 多插件生命周期管线与执行顺序控制规约

### 5.1 顺序控制第一性原理：不可逆单向阶段流

为彻底杜绝“先加签后加密导致签名失效”等逻辑灾难，微内核将生命周期严格划分为**不可逆的阶段（Stages）**：

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
  • 响应报文解密/反脱敏            • 故障时触发 (现场指标增强)           • 离线黑匣子聚合挖掘
```

### 5.2 仲裁规则 (Arbitration Rules)
1. **阶段不可逆原则 (Stage Invariance)**：
   无论在 `.http` 文件中的书写顺序如何，`Prepare` 阶段插件必然先于 `Transform` 执行，`Transform` 必然先于 `Sign` 执行；
2. **同阶段所见即所发 (FIFO Order & No Magic Numbers)**：
   在同一个 Stage 内，100% 严格依照用户在 `.http` 文件中**从上到下的视觉书写行序（Stable Order）**推进，**彻底消灭 `priority=99` 等底层魔数**；
3. **显式管道链式表达 (`@pipeline`)**：
   若需显式声明数据流动关系，直接使用 `@pipeline auth -> transform -> sign`，直观反映输入输出流向。

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
    /// 非致命警告信息 (录入报告与快照)
    #[serde(default)]
    pub warnings: Vec<String>,
}
```

* **Header 合并语义 (Case-Insensitive Upsert)**：当后序插件（或插件覆盖请求预留兜底 Header，如 `Authorization`）输出同名 Header 时，`PluginPatch` 按 HTTP 规范执行**大小写不敏感覆盖更新（Upsert）**，并先执行 `remove_headers` 清理，避免产生重复冲突的 `Authorization` 头。
* **链式折叠**：前序插件产生的 Patch 经微内核校验后合入临时上下文中，后续插件能够读取到前序插件产生的变更结果，形成无数据竞争的纯净函数式流水线。

---

## 6. CLI 自定义命令扩展规范 (Custom Commands SPI)

插件不仅能在请求内作为钩子运行，还可以作为子命令无缝扩展 `rupost` CLI。

### 6.1 命名契约与双模动态路由
* **命名契约**：插件二进制统一遵循 `rupost-plugin-<name>`（推荐）或兼容 `rupost-<name>`；
* **双模路由流程**：
  1. 用户在终端执行 `rupost flamegraph snapshot.json --theme dark`；
  2. Clap 匹配内置子命令（`test`, `mock`, `diagnose`, `tui` 等）；
  3. 若内置命令未命中，CLI 派发器通过 `PluginRegistry` 查找对应的命令 `flamegraph`；
  4. 查找到 `rupost-plugin-flamegraph` 后，启动子进程拉起执行。

### 6.2 零损耗 TTY 直通与上下文注入
* **TTY 独占直通**：直接继承终端的 `stdin`、`stdout`、`stderr`，使外部插件能够原生运行交互式 TUI、密码输入或进度动画；
* **上下文环境变量注入**：
  * `RUPOST_WORKSPACE`：当前工作区绝对路径；
  * `RUPOST_ENV`：当前激活的环境标签（如 `--env staging`）；
  * `RUPOST_CONFIG_DIR`：配置与插件注册表目录路径；
* **退出码 100% 穿透**：子进程退出后，微内核将子进程的 Exit Code 作为自身的退出码返回，完全遵守 CI/CD 流水线退出码判定契约。

---

## 7. AI 预留接口体系 (AI Integration SPI)

严格遵循 `doc/plans/archive/2026-q4/2026-10-27-ai-master-roadmap.md` 中的宪法原则（B0~B4 分级、五阶段管线模板），插件注册表为未来的 AI 扩展预留以下四大核心接口：

### 7.1 `AiContextSPI`：安全脱敏上下文提取与注入接口
AI 插件在排查故障时需要上下文，但严禁泄露生产凭据。微内核保证在将数据推入 AI 管道前，强制经由 `src/ai/redactor.rs` 进行脱敏清洗：
```rust
#[async_trait]
pub trait AiContextProvider: Send + Sync {
    async fn extract_context(
        &self,
        request: &SnapshotRequest,
        response: Option<&SnapshotResponse>,
        error: Option<&ExecutionErrorDetail>,
    ) -> AiSanitizedContext;
}
```

### 7.2 `AiMinerSPI`：离线快照黑匣子分诊与归因接口
对应路线图中的 **E2（失败归因就地化）** 与 **E5（CI 失败聚类 triage）**。
在离线场景下，AI 插件作为独立的 `Miner` 消费 `snapshot.json`，产出结构化归因建议：
```rust
#[async_trait]
pub trait AiSnapshotMiner: Send + Sync {
    fn miner_id(&self) -> &str;
    async fn analyze_snapshot(
        &self,
        snapshot: &ExecutionSnapshotFile,
        options: &MinerOptions,
    ) -> Result<AiTriageReport>;
}
```
* **B1/B2 兜底机制**：AI 插件不可用（断网、无 Key、超时）时，微内核自动降级为基于状态码和网络时序指标的**确定性机械归因**，绝不崩溃挂起。

### 7.3 `EngineArbiter`：引擎裁决与建议校验接口
贯彻宪法 **“AI 建议，引擎裁决”**：
AI 插件生成的任何 `@assert` 表达式、`.http` 报文或 Mock 变体，必须先送入 `EngineArbiter` 进行 AST 语法检查。非法建议立即丢弃并输出警告，绝不污染代码库。

### 7.4 `McpToolBridge`：MCP 模型上下文协议双向桥梁
在统一注册表的驱动下，所有已注册的插件和内核命令均可一键导出为标准的 MCP (Model Context Protocol) Tools，使 `rupost` 原生充当**“AI Agent 的网络手脚”**。

---

## 8. 通信底层守护 (Stdio IPC Protocol & Guard)

外部插件与微内核通信遵循无死锁、强类型的 JSON 管道规范：

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
    "params": {"region": "us-east-1"}
  }
  ```
* **输出 (`stdout` - 直接对应 `PluginPatch` 契约)**:
  ```json
  {
    "extra_headers": [["Authorization", "AWS4-HMAC-SHA256 ..."]],
    "remove_headers": [],
    "mutated_body": null,
    "set_variables": {},
    "warnings": []
  }
  ```

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
   * 将 CLI 扩展命令、生命周期钩子与离线挖掘器统一归口管理，平常只读 Manifest（$\le 5\text{ms}$），运行时按需懒执行；
2. **“执行现场与深度分析解耦”的事件溯源思想**：
   * 现场只负责高保真记录网络微秒级时序与原始数据黑匣子，复杂挖掘全部留待事后离线消费；
3. **上下文作用域安全注入 (Scoped Context)**：
   * 杜绝隐式全局变量，插件只能通过微内核安全上下文（经过 `redactor.rs` 脱敏清洗）获取信息；
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
  在 `StdioDriver` 中，必须将 `stdin` 写入与 `stdout/stderr` 异步消费拆分为两个独立的并发 `tokio::spawn` 任务；使用 `tokio::io::AsyncReadExt::take(10 * 1024 * 1024)` 限制读取量；在写入 `stdin` 完毕后，必须显式执行 `stdin.shutdown().await` 并立即 drop 句柄。

### 10.2 进程树完全清剿机制 (Process Group Termination)
* **Unix 平台 (PGID 树状清剿)**：
  在启动插件子进程时调用 `std::os::unix::process::CommandExt::process_group(0)`，将子进程设置为新的进程组 Leader；超时熔断时向 `-pgid` 发送 `SIGKILL`，确保连同孙子进程彻底杀绝，防止 PID/FD 耗尽与僵尸进程；
* **Windows 平台 (Job Object)**：
  使用 Windows `CreateJobObjectW` 并配置 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`，绑定子进程生命周期，父进程退出时系统自动强杀所有派生进程。

### 10.3 注册表并发写保护与防幽灵缓存 (Atomic Rename & Hash Fingerprint)
* **多进程与多线程原子重命名覆盖**：刷新 `~/.rupost/registry.json` 时，必须先写入同目录带 `PID + 原子递增序号 (AtomicU64)` 的临时文件（`registry.json.tmp.<pid>.<seq>`），再通过原子重命名（`fs::rename`）覆盖目标文件，彻底杜绝多进程及同进程多线程并行测试时的并发踩踏；
* **极速指纹增强**：缓存校验条件由单纯比对 `mtime` 升级为：
  $$\text{Cache Valid} \iff \text{mtime matches} \land \text{Hash}(\text{First 4KB}) \text{ matches}$$
  彻底避免 1 秒内文件修改造成的“幽灵虚假命中”。

### 10.4 Windows 平台脚本透明代理 (Shebang Runner Adapter)
在 Windows 环境下探测插件时，若发现目标是 `.py`、`.js`、`.sh` 脚本，微内核自动将其封装为对应宿主环境解释器命令（如 `python.exe <script>`），消灭无扩展名脚本在 Windows 上的 `OS error 193` 执行崩溃。

### 10.5 三态测试判定模型 (Three-State Outcome & Strict Gate)
* **状态明确拆分**：快照与测试报告中新增 `ExecutionOutcome::DegradedPass`，严禁将自愈降级混淆为纯粹的成功；
* **CI 门禁模式分流**：
  * CLI 引入 `--strict-plugins` 标志（发布环境 CI 推荐默认开启）：一旦发生任何插件降级跳过，无论 HTTP 返回码为何，整体退出码强制为 `1`；
  * 只有在现场调试排障且显式指定 `--allow-missing-plugins` 时，才允许打印高亮 `[WARN: PLUGIN DEGRADED]` 并返回 `0`。

### 10.6 快照防 OOM 截断限额 (Snapshot Lightweight Limit)
* 默认单响应体录入上限设为 `256KB`（可通过 `--max-snapshot-body-bytes` 调整）。超限响应体执行截断，并标记 `body_truncated: true`，同时记录完整报文的 `body_sha256` 校验和，确保快照文件轻盈（控制在 5MB 以内）。
