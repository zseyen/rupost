# 04 - 跨场景端到端测试方案与防护网 (Testing Strategy)

- **状态**: Draft (v2.1.0 — 测试方案落地性修正；评审通过后回标 Approved)
- **归属任务**: `cross-scenario-engine-and-plugins`
- **目标**: 针对"开发环境（富插件）、发布环境（CI 基础）、调试环境（即拷即走无依赖）"建立确定性的端到端测试守护。
- **语法与契约对齐声明**: 本文档所有测试用例以 02（快照 Schema SSOT）与 03（指令语法/降级语义 SSOT）为准；每个套件的归属里程碑在 §4 显式登记（与 06 对齐）。

---

## 1. 测试金字塔与场景矩阵

```
                ┌────────────────────────────────────────────────────────┐
                │        Layer 4: 跨环境产物消费测试 (Artifact Mining)    │
                │        (调试机 dump.json ──> 开发机富插件深度挖掘)      │
                ├────────────────────────────────────────────────────────┤
                │        Layer 3: 恶劣环境即拷即走测试 (Zero-Dep Portable)│
                │        (在 0 Python/Node 的裸容器内运行基础测试闭环)    │
                ├────────────────────────────────────────────────────────┤
                │        Layer 2: 插件协议与优雅降级集成测试 (Degradation)│
                │        (缺失插件跳过、未授权拦截、超时杀进程熔断)      │
                ├────────────────────────────────────────────────────────┤
                │        Layer 1: 基础单元测试 (Unit & Red-Green)        │
                │        (请求体保真、内置宏向量、JUnit 生成、时延采集)   │
                └────────────────────────────────────────────────────────┘
```

---

## 2. 核心场景专项测试用例设计

### 测试套件 1：调试环境"即拷即走"自包含验证 (Pure Standalone Isolation) — owner: 06 M5
* **语义重定义（v2.1.0）**：本套件要证明的不是"机器上没有解释器"（macOS `/usr/bin/python3` 与 ubuntu-latest CI 均预装，PATH 隔离无法证明），而是三件可证伪的事：
  1. **rupost 基础执行路径零子进程派生**：纯宏路径（无插件指令）的 `test` 全流程不启动任何外部命令；
  2. **隔离环境全绿**：`env -i`（或 PATH 指向空目录）+ 项目自带 mock server（`src/mock/`，零外部依赖）跑完整套件，断言退出码与快照产物；
  3. **真裸机叙事由 CI 容器 job 承担**：`debian-slim` 容器内运行 release 二进制回归 + `ldd` 静态链接检查；musl 静态构建 target 登记于 06（支撑 Alpine/Scratch 叙事）。
* **测试用例实现 (`tests/standalone_portable_test.rs`)**：
  ```rust
  #[tokio::test]
  async fn test_standalone_execution_without_external_runtime() {
      // 1. 隔离环境：PATH 指向空目录 + 清空 env（helper 为 tests/ 内真实实现，
      //    以子进程方式运行 release/debug rupost 二进制，而非进程内 set_var——避免并行测试竞态）
      // 2. 用例仅使用内置宏（宏求值顺序：外层 {{...}} 先于宏参数求值，语义由
      //    src/variable 模块守护测试锁定；嵌套插值以外的 {{$body}} 拼写废除——
      //    请求体引用属 @capture/变量域，不属于宏）
      let http_content = r#"
      POST http://127.0.0.1:8080/secure-api
      X-Timestamp: {{$timestamp_ms}}
      X-Sign: {{$hmac_sha256("secret", "{{$timestamp_ms}}")}}
      Content-Type: application/json

      {"order_id": 999}

      # @assert status == 200
      "#;

      // 3. 执行测试并保存快照（mock server 由 src/mock 提供，监听 127.0.0.1 随机端口）
      //    断言：退出码 0；快照存在且 0600 权限；审计子进程派生数为 0
      #;
  }
  ```
* **反假守护**：若把"内置宏"反转 stub 为"调用外部 openssl"，本套件必须变红（子进程派生数断言 + 宏向量值断言双重锁定）。

---

### 测试套件 2：插件缺失、自愈降级与三态门禁验证 (Graceful Degradation & Strict Gate Test) — owner: 06 M3
* **目标**：验证调试环境遇到未安装的插件时，测试流程不崩溃，基础发包与数据记录正常闭环；同时验证 CI `--strict-plugins` 门禁能拦截降级假阳性。
* **测试用例实现 (`tests/plugin_degradation_test.rs`)**：
  1. **未声明降级（默认安全拦截）**：
     - 用例包含 `@sign non_existent_signer`；
     - 未传 `--allow-missing-plugins`；
     - 断言：返回明确的错误提示（包含搜索过的路径清单），进程退出码 `1`。
  2. **声明 `?optional` 或 `--allow-missing-plugins`（现场排障优雅自愈）**：
     - 用例包含 `@sign non_existent_signer ?optional`，同时手写静态兜底 Header（如 `X-Debug-Token: fixed-value`）；
     - 断言：测试正常发送请求，**mock server 收到的 Header 与 `.http` 文件书写内容逐字一致**（裁决：降级 = 按文件原样外发，引擎绝不注入魔法 Header，03 §2.1）；单条结果标记 `ExecutionOutcome::DegradedPass`；
     - 导出的快照 JSON 中明确包含 Warning：`"插件 'non_existent_signer' 未安装，已降级跳过"`；
     - 附加断言：含未解析 `{{var}}` 的兜底 Header 场景下，该请求被**跳过**且 WARNING（03 §2.1 禁止事项）。
  3. **CI 严格门禁拦截（`--strict-plugins`）**：
     - 用例包含 `@sign non_existent_signer ?optional`，同时传入 `--strict-plugins`；
     - 断言：即使 HTTP 响应为 200，因发生插件降级，整体退出码强制为 `1`，阻断假阳性流入生产。
* **反假守护（正向对照组，v2.1.0 新增）**：同套件内包含一个真实存在的 mock 插件（tests/ 内编译的极简 Rust 二进制，注入特征 Header `X-Plugin-Proof: <magic>`）——断言 mock server 收到该 Header。两组对照保证：把生产逻辑反转成"缺失→静默忽略"时（wiremock 收不到请求 / 收不到 Header）与"缺失→假降级成功"时（DegradedPass 与 Header 不符）测试必红。
* **"存在但失败"四分类（与 03 §8.3 对齐）**：见 §2.5 负向矩阵。

---

### 测试套件 3：跨环境产物深度挖掘验证 (Artifact Mining & Consumption Test) — owner: 06 M1（Schema 契约）+ M5（录制保真）
* **目标**：验证调试环境生成的产物，拷回开发环境后能被插件 100% 消费并挖掘深层信息。
* **拆分裁决（v2.1.0）**：原单测混合"手工 JSON 反序列化"与"真实录制"，拆为两个测试：
  1. **Schema 契约测试**（M1，快照模型落地时）：手工构造符合 02 §2 的 JSON 做 serde round-trip，锁定字段名/类型/单位（`entries`、`_us` 后缀、`Option` 语义），并验证未知字段容忍（追加字段可反序列化）；
  2. **录制保真测试**（M5）：wiremock（Rust crate，dev-deps 已有）延迟 100ms 响应 + 构造 504 场景，断言快照留存了时序与错误现场。
  ```rust
  #[tokio::test]
  async fn test_artifact_can_be_mined_by_offline_plugins() {
      // 1. 真实录制：mock 返回 504 + 延迟响应
      let dump_file = record_real_traffic_dump().await;

      // 2. 消费该产物 (反序列化 ExecutionSnapshotFile —— 库外集成测试用 rupost:: 路径)
      let content = std::fs::read_to_string(&dump_file).unwrap();
      let snapshot: rupost::history::ExecutionSnapshotFile = serde_json::from_str(&content).unwrap();

      // 3. 验证关键性能指标与现场未丢失
      //    注：连接复用语义下 dns/tcp 可能为 None（02 §2），断言只锁 ttfb/total
      let entry = &snapshot.entries[0];
      assert_eq!(entry.status, Some(504));
      let timing = entry.timing_waterfall.as_ref().unwrap();
      assert!(timing.ttfb_us > 50_000); // 100ms 延迟 mock，证伪"时序丢失"足够；
                                        // 原 >4_000_000 断言需 mock sleep 4s，拖慢套件，废除

      // 4. 验证 AI 归因输入适配器能基于该快照提取 FailureContext [待建 M5]
      //    （函数由 crates/rupost-ai 提供，随 ai triage --snapshot 消费者落地）
      let ctx = rupost_ai::attribution::context_from_snapshot_entry(entry);
      assert!(ctx.error_message.contains("504"));
  }
  ```

---

### 测试套件 4：请求体字节保真度验证 (Byte-Level Fidelity Test) — owner: 06 M1（守护已有 raw_body）
* **目标**：验证 JSON 请求体在任何环境下均未被 `serde_json` 偷偷重排或抹除空白。
* **实现裁决（v2.1.0）**：**WireMock 不可用**——`wiremock-rs` 工作在 HTTP 语义层，拿不到原始 TCP 字节；引入 Java 版 WireMock 违反零依赖哲学。改用纯 Rust：
  * tokio `TcpListener` accept 后 `read_to_end` 收原始字节，按 `\r\n\r\n` 手工切分 header/body；
  * 发送带有换行、多级缩进、以及 `100.00` 尾随零的复杂 JSON；
  * 对 body 逐字节比对 + 校验和断言（MD5/SHA256 100% 一致）；断言锁定 body 字节而非整包（reqwest 的 Content-Length/分块行为不属本守护目标）。

---

### 测试套件 5：插件生命周期顺序、复合键与防死锁守护测试 (`tests/plugin_framework_test.rs`) — owner: 06 M3
* **目标**：验证阶段不可逆性、同阶段书写行序 FIFO、单插件多阶段复合键注册、带空格引号参数解析及进程管道防死锁。
* **核心断言覆盖**：
  1. **阶段不可逆 + 同阶段 FIFO**：乱序声明 `@sign s1`、`@transform t2`、`@auth a1`、`@transform t1`，断言最终执行序列恒为 `[a1, t2, t1, s1]`；
  2. **复合键 `(plugin_name, stage)` 防覆盖**：单插件同时注册 `@auth` 与 `@sign` 钩子，断言两个阶段钩子均可在 `PluginRegistry` 中独立寻址；
  3. **带空格引号参数解析**：`@auth oauth2 scope="read:orders write:orders" ?optional` 完整保留 `"read:orders write:orders"`；
  4. **Header 大小写不敏感 Upsert**：后序插件输出的 `authorization` 覆盖前序 `Authorization` 兜底头，不产生重复 Header。

---

### 2.5 负向测试矩阵 (Plugin Failure Taxonomy Matrix) — owner: 06 M3（与 03 §8.3 对齐）

Mock 插件矩阵（`tests/fixtures/` 下 **`/bin/sh` + `dd`/`head` 或编译型 Rust 小二进制实现，禁用 Python**——测试 harness 依赖解释器会破坏跨平台 CI）：

| # | Mock 插件行为 | 预期 (默认) | 预期 (`?optional` / `--allow-missing-plugins`) |
| :-- | :--- | :--- | :--- |
| 1 | 正常输出 Patch | 注入生效，请求成功 | 同左 |
| 2 | 输出超 64KB / 10MB 大报文 | 载荷截断/拒绝 → `invalid_output` 降级 | 同左 |
| 3 | sleep 超 5s（`--plugin-timeout` 可调） | 进程组 SIGKILL → `timeout` 降级 | 同左 |
| 4 | stdout 输出非法 JSON / 前置日志 | `invalid_output` 降级 + WARNING | 同左 |
| 5 | `exit 3` 无输出 | `exit_non_zero` 降级 + stderr 摘要 | 同左 |
| 6 | 向 stderr 写入洪泛数据 | 无死锁（异步排水），stderr 截断 16KB 入 warnings | 同左 |

补充组合与容错测试：
* **并发 × 插件**：`parallel` 模式 + suite 级 `cache_scope` 插件，断言子进程总数受缓存约束（100 请求 × 1 @auth ≤ 2 次）且无竞态；
* **快照容错**：目标文件只读/磁盘满/损坏 JSON 的错误路径（写失败必须报错，不得静默丢失现场）；
* **fail-fast 组级清剿**：熔断时在跑插件进程被整组 kill（03 §10.2），无孤儿。

---

## 3. 全量自动化执行门禁

在提交任何环境适配与插件代码前，必须在本地运行以下一键回归链：
```bash
# 1. 基础单测与代码规范
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# 2. 场景化测试套件专项验证
cargo test --test standalone_portable_test
cargo test --test plugin_degradation_test
cargo test --test artifact_mining_test
cargo test --test byte_fidelity_test
cargo test --test plugin_framework_test

# 3. 集成与全量示例回归
bash tests/verify_features.sh
bash examples/run_all.sh
```

## 4. 套件归属与里程碑映射（与 06 对齐，消除"防护网无 owner"缺口）

| 套件 | 文件 | 归属 Milestone |
| :--- | :--- | :--- |
| 1 即拷即走 | `tests/standalone_portable_test.rs` | M5 |
| 2 降级与三态门禁 | `tests/plugin_degradation_test.rs` | M3 |
| 3 产物挖掘 | `tests/artifact_mining_test.rs` | M1（契约）+ M5（录制保真） |
| 4 字节保真 | `tests/byte_fidelity_test.rs` | M1 |
| 5 管线框架 | `tests/plugin_framework_test.rs` | M3 |
| 2.5 负向矩阵 | `tests/fixtures/*` + 组合用例 | M3 |
